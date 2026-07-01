use pdf_writer::{Chunk, Finish, Name, Pdf, Ref, Str, TextStr};
use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::OnceLock;
use xmp_writer::{RenditionClass, XmpWriter};

use crate::configure::{PdfVersion, ValidationError};
use crate::error::KrillaResult;
use crate::interchange::metadata::Metadata;
use crate::metadata::{PageLayout, PageMode};
use crate::serialize::SerializeContext;
use crate::util::{stable_hash_base64, Deferred};

type DChunk = Deferred<Chunk>;

/// Collects all chunks that we create while building
/// the PDF and then writes them out in an orderly manner.
pub(crate) struct ChunkContainer {
    pub(crate) streams: StreamChunks,
    pub(crate) mixed: MixedChunks,
    pub(crate) metadata: Option<Metadata>,
    pub(crate) non_stream: NonStreamChunks,
    /// Settings copied from [`crate::Document`]'s
    /// [`DigitalSignature`](crate::interactive::signature::DigitalSignature)
    /// just before [`Self::finish`] runs. `None` when the document
    /// carries no digital signature — the catalogue-emit path then
    /// omits the `/Sig` dictionary and leaves
    /// `/AcroForm /SigFlags` absent.
    pub(crate) signature_settings: Option<SignatureEmissionSettings>,
}

/// Subset of [`DigitalSignature`](crate::interactive::signature::DigitalSignature)
/// that the chunk container needs to write the indirect `/Sig`
/// dictionary. The signer callback itself stays on
/// [`crate::Document`] and is invoked by
/// [`crate::Document::finish`] after `pdf-writer` has produced the
/// final byte buffer.
#[derive(Debug, Clone)]
pub(crate) struct SignatureEmissionSettings {
    pub(crate) sub_filter: crate::interactive::signature::SignatureSubFilter,
    pub(crate) placeholder_size_bytes: usize,
    pub(crate) reason: Option<String>,
    pub(crate) location: Option<String>,
    pub(crate) contact_info: Option<String>,
    pub(crate) signer_name: Option<String>,
    pub(crate) signing_time: Option<String>,
}

pub(crate) struct StreamChunks {
    pub(crate) fonts: Vec<Chunk>,
    pub(crate) shading_functions: Vec<Chunk>,
    pub(crate) patterns: Vec<Chunk>,
    pub(crate) pages: Vec<DChunk>,
    pub(crate) embedded_files: Vec<Chunk>,
    pub(crate) icc_profiles: Vec<Chunk>,
    pub(crate) x_objects: Vec<Chunk>,
    pub(crate) images: Vec<Deferred<KrillaResult<Chunk>>>,
}

pub(crate) struct MixedChunks {
    pub(crate) embedded_pdfs: Vec<Deferred<KrillaResult<EmbeddedPdfChunk>>>,
}

pub(crate) struct NonStreamChunks {
    pub(crate) page_tree: Option<(Ref, Chunk)>,
    pub(crate) outline: Option<(Ref, Chunk)>,
    pub(crate) page_label_tree: Option<(Ref, Chunk)>,
    pub(crate) destination_profiles: Option<(Ref, Chunk)>,
    pub(crate) struct_tree_root: Option<(Ref, Chunk)>,
    pub(crate) struct_elements: Option<Chunk>,
    pub(crate) page_labels: Chunk,
    pub(crate) annotations: Chunk,
    pub(crate) color_spaces: Chunk,
    pub(crate) destinations: Chunk,
    pub(crate) ext_g_states: Chunk,
    pub(crate) resource_dictionaries: Chunk,
    pub(crate) masks: Chunk,
    pub(crate) fonts: Chunk,
    pub(crate) shading_functions: Chunk,
    pub(crate) patterns: Chunk,
    pub(crate) pages: Chunk,
    pub(crate) embedded_files: Chunk,
}

impl ChunkContainer {
    /// Apply the digital-signature emission settings copied from
    /// [`crate::Document::with_digital_signature`]. Called by
    /// [`crate::Document::finish`] just before [`Self::finish`]
    /// runs so the catalogue-emit path can pick up the placeholder
    /// reservation and metadata to write into the `/Sig`
    /// dictionary.
    pub(crate) fn set_signature_emission_settings(
        &mut self,
        sub_filter: crate::interactive::signature::SignatureSubFilter,
        placeholder_size_bytes: usize,
        reason: Option<String>,
        location: Option<String>,
        contact_info: Option<String>,
        signer_name: Option<String>,
        signing_time: Option<String>,
    ) {
        self.signature_settings = Some(SignatureEmissionSettings {
            sub_filter,
            placeholder_size_bytes,
            reason,
            location,
            contact_info,
            signer_name,
            signing_time,
        });
    }

    pub(crate) fn new(sc: &SerializeContext) -> Self {
        Self {
            streams: StreamChunks {
                fonts: vec![],
                shading_functions: vec![],
                patterns: vec![],
                pages: vec![],
                embedded_files: vec![],
                icc_profiles: vec![],
                x_objects: vec![],
                images: vec![],
            },
            mixed: MixedChunks {
                embedded_pdfs: vec![],
            },
            metadata: None,
            non_stream: NonStreamChunks {
                page_tree: None,
                outline: None,
                page_label_tree: None,
                destination_profiles: None,
                struct_tree_root: None,
                struct_elements: None,
                page_labels: sc.new_chunk(),
                annotations: sc.new_chunk(),
                color_spaces: sc.new_chunk(),
                destinations: sc.new_chunk(),
                ext_g_states: sc.new_chunk(),
                resource_dictionaries: sc.new_chunk(),
                masks: sc.new_chunk(),
                fonts: sc.new_chunk(),
                shading_functions: sc.new_chunk(),
                patterns: sc.new_chunk(),
                pages: sc.new_chunk(),
                embedded_files: sc.new_chunk(),
            },
            signature_settings: None,
        }
    }

    pub(crate) fn finish(self, sc: &mut SerializeContext) -> KrillaResult<(Pdf, Option<Ref>)> {
        let mut remapped_ref = Ref::new(1);
        let mut remapper = HashMap::new();

        // Allows us to estimate the capacity we will need for the new PDF.
        let mut chunks_byte_len = 0;

        // This traverses the chunks in the order that we will write them to the PDF and assigns new
        // references as we go. This gives us the advantage that the PDF will be numbered with
        // monotonically increasing numbers, which, while it is not a strict requirement for a valid
        // PDF, makes it a lot cleaner and might make implementing features like object streams
        // easier down the road.
        //
        // It also allows us to estimate the capacity we will need for the new PDF.
        self.visit(sc, &mut |chunk| {
            for object_ref in chunk.refs() {
                let existing = remapper.insert(object_ref, remapped_ref.bump());
                debug_assert!(existing.is_none());
            }
            chunks_byte_len += chunk.len();
        })?;

        // Reserve final renumbered refs for every optional-content
        // group (`Document::add_layer`) and update the remapper so
        // that any `/OC <build_ref>` written into a content stream's
        // BDC property dict gets remapped to the matching `OCG` dict
        // when the chunk is renumbered. The build-time refs live on
        // `global_objects.layers`; the renumbered refs are kept here
        // alongside the original layer descriptors so the catalogue
        // writer can later reference them directly.
        let layers_taken = sc.global_objects.layers.take();
        let layer_final_refs: Vec<(Ref, crate::optional_content::Layer)> = layers_taken
            .into_iter()
            .map(|record| {
                let final_ref = remapped_ref.bump();
                remapper.insert(record.ref_, final_ref);
                (final_ref, record.layer)
            })
            .collect();

        // Reserve an indirect ref for the `/Encrypt` dictionary if
        // the document is to be encrypted. Allocated here — after the
        // chunk refs have all been mapped but before any object is
        // written — so the slot never collides with a chunk's
        // renumbered ref or with a downstream metadata/info object.
        let encrypt_ref = sc
            .serialize_settings()
            .encryption
            .as_ref()
            .map(|_| remapped_ref.bump());

        // Reserve the indirect ref for the cross-reference stream
        // (`/Type /XRef`) when `xref_streams` is enabled. Allocated
        // from the SAME final-numbering counter as everything else
        // emitted into the PDF so it can't collide with chunk refs,
        // layer refs, the encrypt ref or downstream metadata refs.
        // pdf-writer's `Pdf::finish_with_xref_stream` consumes it on
        // the way out.
        let xref_stream_ref = sc
            .serialize_settings()
            .xref_streams
            .then(|| remapped_ref.bump());

        // Reserve a final-numbering ref for the `/Sig` indirect
        // dictionary. When at least one `SignatureField` widget
        // pre-allocated the build-time ref via
        // `SerializeContext::signature_dict_ref`, we remap it through
        // the same `remapper` used for every other indirect reference
        // so the widget's `/V <ref>` resolves correctly. When the
        // document is configured for signing but no widget allocated
        // the ref (PDFreactor's `signPDF: true` does not require an
        // explicit author-supplied widget), we allocate one here so
        // `chunk_container::finish` still emits the `/Sig` dict body
        // and the AcroForm catalogue carries it directly in
        // `/Fields`.
        let standalone_sig_ref = if sc.signing_enabled {
            if let Some(old_sig_ref) = sc.signature_dict_ref {
                let final_sig_ref = remapped_ref.bump();
                remapper.insert(old_sig_ref, final_sig_ref);
                None
            } else {
                Some(remapped_ref.bump())
            }
        } else {
            None
        };

        // Chunk length is not an exact number because the length might change as we renumber,
        // so we add a bit of a padding by multiplying with 1.1. The 200 is additional padding
        // for the document catalog. This hopefully allows us to avoid re-alloactions in the general
        // case, and thus give us better performance.
        let capacity = (chunks_byte_len as f32 * 1.1 + 200.0) as usize;
        let mut pdf = sc.new_pdf_with_capacity(capacity);
        sc.serialize_settings().pdf_version().set_version(&mut pdf);

        if sc.serialize_settings().ascii_compatible
            && !sc
                .serialize_settings()
                .validators()
                .requires_binary_header()
        {
            pdf.set_binary_marker(b"AAAA")
        }

        // Apply AES-256 encryption to the underlying `Pdf` BEFORE any
        // indirect object body is written. From this point on,
        // pdf-writer transparently encrypts every string and stream
        // emitted into the buffer with a per-object IV under the
        // document's file encryption key. The `/Encrypt` dict (written
        // by `Pdf::encrypt` itself), the trailer `/ID` strings, and —
        // when the caller disables `encrypt_metadata` — the metadata
        // stream remain in clear per ISO 32000-2 §7.6.
        if let (Some(ref_), Some(enc)) = (encrypt_ref, sc.serialize_settings().encryption.as_ref())
        {
            // ISO 19005 (PDF/A) and ISO 15930 (PDF/X) both ban
            // `/Encrypt`. Surface that mismatch through the standard
            // validation-error channel — the file is still encrypted
            // (it would otherwise silently lose its security setting),
            // but the caller now receives a hard error at finish time
            // explaining why their archival/print configuration is
            // inconsistent with the encryption request. PDF/UA is
            // silent on encryption, so combinations with that
            // validator pass through unflagged.
            sc.register_validation_error(ValidationError::ContainsEncryption);
            pdf.encrypt(ref_, enc.to_pdf_writer());
        }

        // Write the chunks in all the fields.
        self.visit(sc, &mut |chunk| {
            chunk.renumber_into(&mut pdf, |old| remapper[&old]);
        })?;

        // Whether to run the Info-dict / XMP metadata serialization. We do so
        // when the user supplied metadata, or when a PDF/X validator is active:
        // PDF/X requires `/GTS_PDFXVersion` and `/Trapped` (and the XMP
        // identification) even when the caller set no metadata object. For
        // every other case with no metadata, we skip these paths entirely so
        // that no spurious `MissingDocumentDate` is raised (matching the
        // behaviour for documents without a metadata object).
        let serialize_metadata =
            self.metadata.is_some() || sc.serialize_settings().validators().is_pdf_x();
        // Default `Metadata` has no title; the missing-title validation below
        // catches that case explicitly.
        let metadata = self.metadata.unwrap_or_default();
        let missing_title = metadata.title.is_none();

        if missing_title {
            sc.register_validation_error(ValidationError::NoDocumentTitle);
        }

        // Write the PDF document info metadata.
        if serialize_metadata {
            metadata.serialize_document_info(
                &mut remapped_ref,
                &mut pdf,
                sc.serialize_settings().configuration,
            );
        }

        let instance_id = stable_hash_base64(pdf.as_bytes());

        let document_id = if let Some(document_id) = &metadata.document_id {
            stable_hash_base64(&(sc.serialize_settings().pdf_version().as_str(), document_id))
        } else if metadata.title.is_some() && metadata.authors.is_some() {
            stable_hash_base64(&(
                sc.serialize_settings().pdf_version().as_str(),
                &metadata.title,
                &metadata.authors,
            ))
        } else {
            instance_id.clone()
        };

        let mut xmp = XmpWriter::new();
        if serialize_metadata {
            metadata.serialize_xmp_metadata(&mut xmp, sc, &instance_id);
        }

        let settings = sc.serialize_settings();
        let validators = settings.validators();
        validators.write_xmp(&mut xmp);

        xmp.num_pages(sc.page_infos().len() as u32);
        xmp.format("application/pdf");
        xmp.instance_id(&instance_id);
        xmp.document_id(&document_id);
        pdf.set_file_id((
            document_id.as_bytes().to_vec(),
            instance_id.as_bytes().to_vec(),
        ));

        xmp.rendition_class(RenditionClass::Proof);
        sc.serialize_settings().pdf_version().write_xmp(&mut xmp);

        let named_destinations = sc.global_objects.named_destinations.take();
        let embedded_files = sc.global_objects.embedded_files.take();
        let widget_fields = sc.global_objects.widget_fields.take();

        // Emit one `/Type /OCG` indirect object per registered layer.
        // The refs were pre-allocated above so any `/OC <ref>` in a
        // content stream's BDC property dict resolves correctly after
        // chunk renumbering.
        for (ref_, layer) in &layer_final_refs {
            let mut ocg = pdf.optional_content_group(*ref_);
            ocg.name(TextStr(&layer.name));
            ocg.intent(layer.intent.to_pdf_writer());
        }

        // G64 — emit one indirect `/S /JavaScript` action dictionary
        // per document-level script (`Metadata::document_javascript`)
        // and one per catalogue-level event script
        // (`Metadata::document_event_script`). The refs are allocated
        // here from the same final-numbering counter as the layer /
        // metadata refs, so they don't collide with chunk-renumbered
        // refs. Both sets are referenced from inside the catalog block
        // (named-JavaScript entries land on `/Names /JavaScript`,
        // event-keyed entries on `/AA /WC` / `/WS` / `/DS` / `/WP` /
        // `/DP`). ISO 32000-2 §12.6.4.16 (JavaScript action) plus
        // §12.6.3 Table 200 (catalogue additional-actions).
        let document_js_refs: Vec<(String, Ref)> = metadata
            .document_javascripts
            .iter()
            .map(|(name, source)| {
                let ref_ = remapped_ref.bump();
                let mut action = pdf.indirect(ref_).start::<pdf_writer::writers::Action>();
                action
                    .action_type(pdf_writer::types::ActionType::JavaScript)
                    .js_string(TextStr(source));
                action.finish();
                (name.clone(), ref_)
            })
            .collect();

        let document_event_refs: Vec<(crate::interchange::metadata::DocumentEvent, Ref)> = metadata
            .document_event_scripts
            .iter()
            .map(|(event, source)| {
                let ref_ = remapped_ref.bump();
                let mut action = pdf.indirect(ref_).start::<pdf_writer::writers::Action>();
                action
                    .action_type(pdf_writer::types::ActionType::JavaScript)
                    .js_string(TextStr(source));
                action.finish();
                (*event, ref_)
            })
            .collect();

        // We only write a catalog if a page tree exists. Every valid PDF must have one
        // and krilla ensures that there always is one, but for snapshot tests, it can be
        // useful to not write a document catalog if we don't actually need it for the test.
        if self.non_stream.page_tree.is_some()
            || self.non_stream.outline.is_some()
            || self.non_stream.page_label_tree.is_some()
            || self.non_stream.destination_profiles.is_some()
            || self.non_stream.struct_tree_root.is_some()
        {
            // Raw-XMP override: when the caller supplied a verbatim XMP
            // packet via `Metadata::raw_xmp`, write those bytes into the
            // `/Metadata` stream instead of finishing the in-memory
            // [`XmpWriter`]. This is an explicit opt-in, so it forces the
            // catalogue to carry a `/Metadata` entry even when
            // `SerializeSettings::xmp_metadata` is `false`.
            let meta_ref = if let Some(raw) = metadata.raw_xmp.as_deref() {
                let meta_ref = remapped_ref.bump();
                pdf.stream(meta_ref, raw)
                    .pair(Name(b"Type"), Name(b"Metadata"))
                    .pair(Name(b"Subtype"), Name(b"XML"));
                Some(meta_ref)
            } else if sc.serialize_settings().xmp_metadata {
                let meta_ref = remapped_ref.bump();
                let xmp_buf = xmp.finish(None);
                pdf.stream(meta_ref, xmp_buf.as_bytes())
                    .pair(Name(b"Type"), Name(b"Metadata"))
                    .pair(Name(b"Subtype"), Name(b"XML"));
                Some(meta_ref)
            } else {
                None
            };

            let catalog_ref = remapped_ref.bump();

            let mut catalog = pdf.catalog(catalog_ref);

            if let Some(pt) = &self.non_stream.page_tree {
                catalog.pages(remapper[&pt.0]);
            }

            if let Some(meta_ref) = meta_ref {
                catalog.metadata(meta_ref);
            }

            if let Some(pl) = &self.non_stream.page_label_tree {
                catalog.pair(Name(b"PageLabels"), remapper[&pl.0]);
            }

            if let Some(oi) = &self.non_stream.destination_profiles {
                catalog.pair(Name(b"OutputIntents"), remapper[&oi.0]);
            }

            if let Some(lang) = metadata.language.as_ref() {
                catalog.lang(TextStr(lang));
            } else {
                sc.register_validation_error(ValidationError::NoDocumentLanguage);
            }

            if let Some(st) = &self.non_stream.struct_tree_root {
                catalog.pair(Name(b"StructTreeRoot"), remapper[&st.0]);
                let mut mark_info = catalog.mark_info();
                mark_info.marked(true);
                if sc.serialize_settings().pdf_version() >= PdfVersion::Pdf16
                    && sc.serialize_settings().pdf_version() < PdfVersion::Pdf20
                {
                    // We always set suspects to false because it's required by PDF/UA.
                    mark_info.suspects(false);
                }
                mark_info.finish();
            }

            let validator_requires_display_doc_title = sc
                .serialize_settings()
                .validators()
                .requires_display_doc_title();
            let text_direction = metadata.text_direction;
            // `DisplayDocTitle` may be requested by the validator
            // (PDF/UA-1 mandates `true`) or by the author via
            // `ViewerPreferences::display_doc_title`. The latter wins
            // when set explicitly; a `false` from the author when the
            // validator demands `true` is a documented author error
            // but is still emitted faithfully so the validator's own
            // post-emit check surfaces the violation.
            let vp_struct = metadata.viewer_preferences.clone();
            let effective_display_doc_title =
                vp_struct
                    .display_doc_title
                    .or(if validator_requires_display_doc_title {
                        Some(true)
                    } else {
                        None
                    });

            let needs_viewer_prefs = effective_display_doc_title.is_some()
                || text_direction.is_some()
                || !vp_struct.is_empty();

            if needs_viewer_prefs {
                let mut vp = catalog.viewer_preferences();

                if let Some(display) = effective_display_doc_title {
                    vp.display_doc_title(display);
                }

                if let Some(dir) = text_direction {
                    vp.direction(dir.to_pdf());
                }

                if let Some(hide) = vp_struct.hide_toolbar {
                    vp.hide_toolbar(hide);
                }
                if let Some(hide) = vp_struct.hide_menubar {
                    vp.hide_menubar(hide);
                }
                if let Some(hide) = vp_struct.hide_window_ui {
                    // pdf-writer's ViewerPreferences derefs to Dict;
                    // /HideWindowUI is a plain boolean in the spec.
                    vp.pair(Name(b"HideWindowUI"), hide);
                }
                if let Some(fit) = vp_struct.fit_window {
                    vp.fit_window(fit);
                }
                if let Some(center) = vp_struct.center_window {
                    vp.center_window(center);
                }
                if let Some(mode) = vp_struct.non_fullscreen_page_mode {
                    vp.non_full_screen_page_mode(mode.to_pdf());
                }
                if let Some(scaling) = vp_struct.print_scaling {
                    vp.pair(Name(b"PrintScaling"), scaling.to_pdf_name());
                }
                if let Some(duplex) = vp_struct.duplex {
                    vp.pair(Name(b"Duplex"), duplex.to_pdf_name());
                }
                if let Some(enabled) = vp_struct.pick_tray_by_pdf_size {
                    vp.pair(Name(b"PickTrayByPDFSize"), enabled);
                }
                // K13 — `/NumCopies` (ISO 32000-2 §12.4.4 Table 168).
                // The spec requires a positive integer; clamp a `0`
                // request to `1` rather than emitting an invalid PDF.
                if let Some(copies) = vp_struct.num_copies {
                    let copies = if copies == 0 { 1 } else { copies };
                    vp.pair(Name(b"NumCopies"), copies as i32);
                }
                // K13 — `/PrintPageRange` (ISO 32000-2 §12.4.4
                // Table 168): even-length array of inclusive
                // 1-indexed `[from, to]` pairs. An empty author
                // vector means "no preference" — the entry is
                // omitted so the viewer falls back to "all pages".
                if let Some(ref ranges) = vp_struct.print_page_range {
                    if !ranges.is_empty() {
                        let mut arr = vp.deref_mut().insert(Name(b"PrintPageRange")).array();
                        for &(from, to) in ranges {
                            arr.item(from as i32);
                            arr.item(to as i32);
                        }
                        arr.finish();
                    }
                }
                // K14 — `/ViewArea`, `/ViewClip`, `/PrintArea`,
                // `/PrintClip` (ISO 32000-2 §12.4.4 Table 168).
                // Each selects one of the document's authored page
                // boxes; omitted entries default to `MediaBox`.
                if let Some(sel) = vp_struct.view_area {
                    vp.pair(Name(b"ViewArea"), sel.to_pdf_name());
                }
                if let Some(sel) = vp_struct.view_clip {
                    vp.pair(Name(b"ViewClip"), sel.to_pdf_name());
                }
                if let Some(sel) = vp_struct.print_area {
                    vp.pair(Name(b"PrintArea"), sel.to_pdf_name());
                }
                if let Some(sel) = vp_struct.print_clip {
                    vp.pair(Name(b"PrintClip"), sel.to_pdf_name());
                }
            }

            let page_layout = metadata.page_layout;
            if let Some(layout) = page_layout {
                // TwoPageLeft and TwoPageRight are only available PDF 1.5+
                if sc.serialize_settings().pdf_version() >= PdfVersion::Pdf15
                    || !matches!(layout, PageLayout::TwoPageLeft | PageLayout::TwoPageRight)
                {
                    catalog.page_layout(layout.to_pdf());
                }
            }
            if let Some(mode) = metadata.page_mode {
                // `UseOC` is PDF 1.5+; `UseAttachments` is PDF 1.6+.
                // Versions below that silently fall back to UseNone.
                let pdf_version = sc.serialize_settings().pdf_version();
                let supports_mode = match mode {
                    PageMode::UseOC => pdf_version >= PdfVersion::Pdf15,
                    PageMode::UseAttachments => pdf_version >= PdfVersion::Pdf16,
                    _ => true,
                };
                if supports_mode {
                    catalog.page_mode(mode.to_pdf());
                }
            }

            if let Some(ol) = &self.non_stream.outline {
                catalog.outlines(remapper[&ol.0]);
            }

            // G5b — `/OpenAction` document action (ISO 32000-2
            // §12.6.4.3). Three flavours are supported (see
            // `Metadata::open_action`):
            //
            // - `OpenAction::GoToPage { page_index, zoom }` — the
            //   direct-link `[<page-ref> <destination>]` form. The
            //   destination flavour is mapped through `OpenZoom`;
            //   out-of-range page indices are clamped at the caller
            //   boundary, so resolving against `page_infos()` here
            //   is infallible-by-construction.
            // - `OpenAction::Named(NamedAction)` — the named-action
            //   form `<< /Type /Action /S /Named /N /<name> >>`
            //   (ISO 32000-2 §12.6.4.9 Table 200). Used by the
            //   PDFreactor `printDialogPrompt` parity surface
            //   (NamedAction::Print).
            // - `OpenAction::JavaScript(script)` — the JavaScript
            //   action form
            //   `<< /Type /Action /S /JavaScript /JS (<script>) >>`
            //   (ISO 32000-2 §12.6.4.16). The script is written
            //   verbatim through `TextStr` so PDFDocEncoding /
            //   UTF-16BE escaping is handled by `pdf_writer`.
            if let Some(open_action) = metadata.open_action.as_ref() {
                use crate::interchange::metadata::{OpenAction, OpenZoom};
                use crate::serialize::PageInfo;
                match open_action {
                    OpenAction::GoToPage { page_index, zoom } => {
                        let page_info = sc.page_infos().get(*page_index).expect(
                            "Metadata::open_action page_index out of range; \
                             the embedder must clamp before calling",
                        );
                        let page_ref = match page_info {
                            PageInfo::Krilla { ref_, .. } => *ref_,
                            PageInfo::Pdf { ref_, .. } => *ref_,
                        };
                        let mut array = catalog.deref_mut().insert(Name(b"OpenAction")).array();
                        array.item(page_ref);
                        match zoom {
                            OpenZoom::Xyz(zoom) => {
                                array.item(Name(b"XYZ"));
                                array.item(pdf_writer::Null);
                                array.item(pdf_writer::Null);
                                array.item(*zoom);
                            }
                            OpenZoom::FitPage => {
                                array.item(Name(b"Fit"));
                            }
                            OpenZoom::FitHorizontalToWidth => {
                                array.item(Name(b"FitH"));
                                array.item(pdf_writer::Null);
                            }
                            OpenZoom::FitVerticalToHeight => {
                                array.item(Name(b"FitV"));
                                array.item(pdf_writer::Null);
                            }
                            OpenZoom::FitBoundingBox => {
                                array.item(Name(b"FitB"));
                            }
                            OpenZoom::FitBoundingBoxHorizontal => {
                                array.item(Name(b"FitBH"));
                                array.item(pdf_writer::Null);
                            }
                            OpenZoom::FitBoundingBoxVertical => {
                                array.item(Name(b"FitBV"));
                                array.item(pdf_writer::Null);
                            }
                        }
                        array.finish();
                    }
                    OpenAction::Named(named) => {
                        // `<< /Type /Action /S /Named /N /<name> >>`
                        // per ISO 32000-2 §12.6.4.9 Table 200. pdf-writer
                        // does not expose a Named ActionType today so
                        // the dictionary is written directly.
                        let mut dict = catalog.deref_mut().insert(Name(b"OpenAction")).dict();
                        dict.pair(Name(b"Type"), Name(b"Action"));
                        dict.pair(Name(b"S"), Name(b"Named"));
                        dict.pair(Name(b"N"), named.to_name());
                        dict.finish();
                    }
                    OpenAction::JavaScript(script) => {
                        // `<< /Type /Action /S /JavaScript /JS (<script>) >>`
                        // per ISO 32000-2 §12.6.4.16. The script is
                        // emitted verbatim via `TextStr`, matching the
                        // `Action::JavaScript` widget-annotation path
                        // in `interactive/action.rs`.
                        let mut dict = catalog.deref_mut().insert(Name(b"OpenAction")).dict();
                        dict.pair(Name(b"Type"), Name(b"Action"));
                        dict.pair(Name(b"S"), Name(b"JavaScript"));
                        dict.pair(Name(b"JS"), TextStr(script.as_str()));
                        dict.finish();
                    }
                }
            }

            let settings = sc.serialize_settings();
            let validators = settings.validators();
            let write_embedded_files = self.non_stream.embedded_files.len() != 0
                || validators.requires_embedded_files_when_empty();

            if !named_destinations.is_empty()
                || write_embedded_files
                || !document_js_refs.is_empty()
            {
                // Cannot use pdf-writer API here because it requires Ref's, while
                // we write our destinations directly into the array.
                let mut names = catalog.names();

                if !named_destinations.is_empty() {
                    let mut dest_name_tree = names.destinations();
                    let mut dest_name_entries = dest_name_tree.names();

                    // "The Names entries in the leaf (or root) nodes shall
                    // contain the tree’s keys and their associated values,
                    // arranged in key-value pairs and shall be sorted lexically
                    // in ascending order by key. Shorter keys shall appear
                    // before longer ones beginning with the same byte sequence.
                    // Any encoding of the keys may be used as long as it is
                    // self-consistent; keys shall be compared for equality on
                    // a simple byte-by-byte basis."
                    let mut sorted = named_destinations.into_iter().collect::<Vec<_>>();
                    // Note that named destinations are guaranteed to be unique,
                    // hence just comparing by the name is enough.
                    sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

                    for (name, (dest_ref, _)) in sorted {
                        dest_name_entries.insert(Str(name.as_bytes()), remapper[&dest_ref]);
                    }

                    dest_name_entries.finish();
                    dest_name_tree.finish();
                }

                if write_embedded_files {
                    let mut embedded_files_name_tree = names.embedded_files();
                    let mut embedded_name_entries = embedded_files_name_tree.names();

                    // The PDF name tree MUST be alphabetically
                    // sorted (ISO 32000-1 §7.9.6); the
                    // `BTreeMap<String, _>` iteration order matches
                    // that requirement directly. The per-attachment
                    // `EmbedLocation` is ignored here — partitioning
                    // is applied only to `/AF` (the explicitly
                    // ordered array) below.
                    for (name, (ref_, _location)) in &embedded_files {
                        embedded_name_entries.insert(Str(name.as_bytes()), remapper[ref_]);
                    }
                }

                // G64 — `/Names /JavaScript` name tree (ISO 32000-2
                // §12.6.4.16). Document-level JavaScript actions
                // declared via `Metadata::document_javascript`. Each
                // entry is written as a leaf-level (`/Names [key val
                // ...]`) name tree; the keys MUST be sorted lexically
                // per ISO 32000-1 §7.9.6 so a single leaf node is
                // valid. Author duplicates are pre-deduplicated at
                // the Metadata builder boundary.
                if !document_js_refs.is_empty() {
                    let mut js_name_tree = names.javascript();
                    let mut js_entries = js_name_tree.names();

                    let mut sorted: Vec<&(String, Ref)> = document_js_refs.iter().collect();
                    sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
                    for (name, ref_) in sorted {
                        js_entries.insert(Str(name.as_bytes()), *ref_);
                    }
                    js_entries.finish();
                    js_name_tree.finish();
                }
            }

            if !embedded_files.is_empty() && settings.supports_associated_files() {
                let mut associated_files = catalog.insert(Name(b"AF")).array().typed();
                // ISO 32000-2 §14.13: `/AF` is an array, not a name
                // tree — the order it preserves is what surfaces in
                // viewers that key the attachment panel off `/AF`.
                // Partition: every `EmbedLocation::Before` entry is
                // written ahead of every `EmbedLocation::After`
                // entry, with alphabetical order preserved within
                // each partition (BTreeMap iteration order).
                for (_name, (ref_, location)) in &embedded_files {
                    if matches!(location, crate::embed::EmbedLocation::Before) {
                        associated_files.item(remapper[ref_]).finish();
                    }
                }
                for (_name, (ref_, location)) in &embedded_files {
                    if matches!(location, crate::embed::EmbedLocation::After) {
                        associated_files.item(remapper[ref_]).finish();
                    }
                }
            }

            // AcroForm dictionary (ISO 32000-2 §12.7.3). Written whenever
            // the document emitted at least one widget annotation. We do
            // not currently emit `/AP` appearance streams; setting
            // `/NeedAppearances true` directs conforming viewers
            // (Acrobat in particular) to regenerate appearances from
            // each field's `/V` and `/DA` on first save, which is the
            // standard fallback for engines that emit field values
            // without bundled appearances.
            if !widget_fields.is_empty() || standalone_sig_ref.is_some() {
                let mut acro_form = catalog.insert(Name(b"AcroForm")).dict();
                let mut fields = acro_form.insert(Name(b"Fields")).array();
                for field_ref in &widget_fields {
                    fields.item(remapper[field_ref]);
                }
                // PDFreactor `signPDF: true` without an explicit
                // signature widget: emit the `/Sig` dict ref
                // directly as a field on the AcroForm so consumers
                // see the signature even when no widget annotation
                // anchors it to a visible page region.
                if let Some(sig_ref) = standalone_sig_ref {
                    fields.item(sig_ref);
                }
                fields.finish();
                acro_form.pair(Name(b"NeedAppearances"), true);
                // ISO 32000-2 §12.7.3 Table 224 — `/SigFlags`. Bit 1
                // (SignaturesExist) is set whenever the AcroForm
                // contains a `/Sig` field; bit 2 (AppendOnly)
                // instructs viewers to require incremental-update
                // saves so the signature byte range stays valid.
                // We emit `3` (both bits set) whenever the document
                // is configured with a real digital signature —
                // krilla itself does not enforce incremental saves
                // but conforming consumers will.
                if sc.signing_enabled {
                    acro_form.pair(Name(b"SigFlags"), 3_i32);
                }
                acro_form.finish();
            }

            // /OCProperties (ISO 32000-2 §8.11.4). Required whenever
            // the document declares at least one optional content
            // group; `/OCGs` enumerates every registered layer and
            // `/D` carries the default configuration that drives
            // initial visibility.
            if !layer_final_refs.is_empty() {
                let mut oc = catalog.oc_properties();
                {
                    let mut ocgs = oc.groups();
                    for (ref_, _) in &layer_final_refs {
                        ocgs.item(*ref_);
                    }
                }
                let mut default = oc.default_config();
                {
                    let mut on = default.on();
                    for (ref_, layer) in &layer_final_refs {
                        if layer.default_visible {
                            on.item(*ref_);
                        }
                    }
                }
                {
                    let mut off = default.off();
                    for (ref_, layer) in &layer_final_refs {
                        if !layer.default_visible {
                            off.item(*ref_);
                        }
                    }
                }
                {
                    let mut order = default.order();
                    for (ref_, _) in &layer_final_refs {
                        order.item(*ref_);
                    }
                }
                default.finish();
                oc.finish();
            }

            // G64 — catalogue-level `/AA` additional-actions
            // dictionary (ISO 32000-2 §12.6.3 Table 200). One slot
            // per event keyword (`/WC`, `/WS`, `/DS`, `/WP`, `/DP`)
            // pointing at the indirect JavaScript-action dict
            // written above. pdf-writer exposes typed setters for
            // each catalogue-level event; we emit them by indirect
            // reference rather than building a fresh action dict
            // inline so the actions can be shared with future
            // viewer-side callers that need a stable ref.
            if !document_event_refs.is_empty() {
                use crate::interchange::metadata::DocumentEvent;
                let mut aa = catalog.additional_actions();
                for (event, ref_) in &document_event_refs {
                    let key: &[u8] = match event {
                        DocumentEvent::WillClose => b"WC",
                        DocumentEvent::WillSave => b"WS",
                        DocumentEvent::DidSave => b"DS",
                        DocumentEvent::WillPrint => b"WP",
                        DocumentEvent::DidPrint => b"DP",
                    };
                    // `pdf-writer` does not expose a typed setter for
                    // a Ref reference on the catalogue `/AA` keys
                    // (each `cat_before_close()` / `cat_before_save()`
                    // / etc. starts a fresh inline `Action` writer).
                    // We need the indirect-ref shape so the action
                    // dict can be shared and validators that crawl
                    // `/Names /JavaScript` plus `/AA` see one source
                    // of truth. The deref escape hatch keeps the
                    // emitted bytes spec-conformant (`/AA /<KEY>` is
                    // either an inline dict or an indirect ref per
                    // ISO 32000-2 §12.6.3).
                    aa.deref_mut().pair(Name(key), *ref_);
                }
                aa.finish();
            }

            // K15 — `/PieceInfo` catalogue entry (ISO 32000-2 §14.5).
            // Application-private metadata; one sub-dictionary per
            // owning application name. Each value carries the
            // `/LastModified` timestamp plus a `/Private` sub-
            // dictionary holding the caller-supplied name → text
            // entries.
            {
                // `self.metadata` was moved into the `metadata` local
                // above; a defaulted metadata yields an empty
                // `piece_info` and no `legal_content`, so nothing is
                // emitted when the document carried no metadata object
                // — matching the original `Some(metadata)` guard.
                let metadata = &metadata;
                if !metadata.piece_info.is_empty() {
                    let mut pi = catalog.deref_mut().insert(Name(b"PieceInfo")).dict();
                    for (app, entry) in &metadata.piece_info {
                        let mut sub = pi.insert(Name(app.as_bytes())).dict();
                        sub.pair(
                            Name(b"LastModified"),
                            crate::interchange::metadata::pdf_date(entry.last_modified),
                        );
                        let mut private = sub.insert(Name(b"Private")).dict();
                        for (k, v) in &entry.private {
                            private.pair(Name(k.as_bytes()), TextStr(v));
                        }
                        private.finish();
                        sub.finish();
                    }
                    pi.finish();
                }

                // K15 — `/LegalContent` catalogue entry (ISO 32000-2
                // §14.11.2). Document-level legal-attestation block;
                // every field is optional and emitted only when set
                // by the author.
                if let Some(legal) = metadata.legal_content.as_ref() {
                    if !legal.is_empty() {
                        let mut lc = catalog.deref_mut().insert(Name(b"LegalContent")).dict();
                        if let Some(v) = legal.javascript_actions {
                            lc.pair(Name(b"JavaScriptActions"), v);
                        }
                        if let Some(v) = legal.launch_actions {
                            lc.pair(Name(b"LaunchActions"), v);
                        }
                        if let Some(v) = legal.uri_actions {
                            lc.pair(Name(b"URIActions"), v);
                        }
                        if let Some(v) = legal.movie_actions {
                            lc.pair(Name(b"MovieActions"), v);
                        }
                        if let Some(v) = legal.sound_actions {
                            lc.pair(Name(b"SoundActions"), v);
                        }
                        if let Some(v) = legal.hidden_annotations {
                            lc.pair(Name(b"HiddenAnnotations"), v);
                        }
                        if let Some(v) = legal.external_ref_xobjects {
                            lc.pair(Name(b"ExternalRefXobjects"), v);
                        }
                        if let Some(v) = legal.external_opi_dicts {
                            lc.pair(Name(b"ExternalOPIdicts"), v);
                        }
                        if let Some(v) = legal.non_embedded_fonts {
                            lc.pair(Name(b"NonEmbeddedFonts"), v as i32);
                        }
                        if let Some(v) = legal.optional_content {
                            lc.pair(Name(b"OptionalContent"), v as i32);
                        }
                        if let Some(text) = legal.attestation.as_ref() {
                            lc.pair(Name(b"Attestation"), TextStr(text));
                        }
                        lc.finish();
                    }
                }
            }

            catalog.finish();
        }

        // Digital-signature dictionary (`/Sig`) — ISO 32000-2 §12.8.1
        // Table 252. Written as a top-level indirect object so the
        // widget annotation's `/V` can point at it from any page.
        // The dict carries placeholder `/ByteRange` and `/Contents`
        // values; the [`crate::Document::finish`] post-processor
        // replaces them after `pdf-writer` returns the final byte
        // buffer.
        //
        // The dict is emitted only when:
        // - The document was configured with
        //   [`Document::with_digital_signature`] (`sc.signing_enabled`),
        // - And at least one `SignatureField` widget caused the lazy
        //   allocation of [`SerializeContext::signature_dict_ref`].
        //
        // We allocate the ref lazily so PDFs that opt into signing
        // but emit no `/Sig` widget on any page do not consume an
        // indirect-object slot.
        // Emit the `/Sig` indirect dictionary when signing is on.
        // The ref source depends on whether at least one
        // `SignatureField` widget pre-allocated the build-time ref
        // (via `SerializeContext::signature_dict_ref`) — in which
        // case we resolve through the remapper — or whether the
        // standalone-signing branch above pre-allocated a fresh
        // final-numbering ref.
        if let Some(sig_ref) = sc.signature_dict_ref {
            let remapped_sig_ref = remapper.get(&sig_ref).copied().ok_or_else(|| {
                crate::error::KrillaError::DigitalSignature(
                    "signature dict ref was not present in the chunk remapper — \
                     widget arm allocated it but renumbering dropped the entry"
                        .into(),
                )
            })?;
            Self::write_signature_dict(
                self.signature_settings.as_ref(),
                &mut pdf,
                remapped_sig_ref,
            )?;
        } else if let Some(standalone_ref) = standalone_sig_ref {
            Self::write_signature_dict(self.signature_settings.as_ref(), &mut pdf, standalone_ref)?;
        }

        Ok((pdf, xref_stream_ref))
    }

    /// Emit the placeholder `/Sig` indirect dictionary. Called from
    /// [`Self::finish`] after the catalogue has been written but
    /// before `pdf-writer` finalises the buffer.
    ///
    /// We emit the dict through `pdf-writer`'s typed API and rely on
    /// two carefully-chosen placeholder shapes that the post-finish
    /// patcher can locate and rewrite without shifting downstream
    /// offsets:
    ///
    /// - `/ByteRange [0 1000000000 1000000000 1000000000]` — three
    ///   ten-digit integer placeholders that fit comfortably in
    ///   `i32`. After post-processing each placeholder is replaced
    ///   in place with the actual offset/length, left-padded with
    ///   zeros to ten characters so the array byte width stays
    ///   constant.
    /// - `/Contents (000000…)` — a literal-string placeholder of
    ///   exactly `placeholder_size_bytes * 2` `'0'` ASCII bytes
    ///   between `(` and `)`. After post-processing the entire
    ///   parenthesised literal is overwritten by a hex string of
    ///   the same total byte width (`<HEX…>`), the byte range is
    ///   recomputed against the `<` offset, and the actual DER
    ///   signature bytes are written into the hex slot.
    ///
    /// Both placeholder shapes are recognisable by unique byte
    /// patterns so the patcher can find them with a single
    /// substring scan.
    fn write_signature_dict(
        signature_settings: Option<&SignatureEmissionSettings>,
        pdf: &mut Pdf,
        sig_ref: Ref,
    ) -> KrillaResult<()> {
        use crate::error::KrillaError;
        use crate::interactive::signature::{
            BYTE_RANGE_PLACEHOLDER_VALUE, SIGNATURE_DICT_START_MARKER,
        };
        use pdf_writer::{Str, TextStr};

        let settings = signature_settings.ok_or_else(|| {
            KrillaError::DigitalSignature(
                "signature dict ref was allocated but no \
                     digital-signature emission settings were provided"
                    .into(),
            )
        })?;

        // The contents placeholder must round-trip cleanly through
        // `Str`'s ASCII literal-string path: every `'0'` byte is
        // 0x30, which is in the 32..=126 passthrough range, so
        // `Str(b"0000…")` writes `(0000…)` verbatim — the exact
        // byte pattern the post-finish patcher scans for.
        let contents_placeholder = vec![b'0'; settings.placeholder_size_bytes * 2];

        // No external marker is needed: the post-finish patcher
        // scans for two unique placeholder byte patterns —
        //
        // - `/ByteRange[0 1000000000 1000000000 1000000000]`
        // - `/Contents(0000…)`  (with exactly
        //   `placeholder_size_bytes * 2` zeros)
        //
        // Each appears at most once in any document krilla emits.
        // We retain [`SIGNATURE_DICT_START_MARKER`] as a unique
        // diagnostic substring that is never emitted under any
        // other path, in case a future patcher wants a coarse
        // existence check (currently unused).
        let _ = SIGNATURE_DICT_START_MARKER;

        // Now emit the real `/Sig` dictionary.
        let mut sig_dict = pdf.indirect(sig_ref).dict();
        sig_dict.pair(pdf_writer::Name(b"Type"), pdf_writer::Name(b"Sig"));
        sig_dict.pair(
            pdf_writer::Name(b"Filter"),
            pdf_writer::Name(b"Adobe.PPKLite"),
        );
        sig_dict.pair(
            pdf_writer::Name(b"SubFilter"),
            pdf_writer::Name(settings.sub_filter.as_pdf_name()),
        );
        // /ByteRange placeholder.
        let mut br = sig_dict.insert(pdf_writer::Name(b"ByteRange")).array();
        br.item(0_i32);
        br.item(BYTE_RANGE_PLACEHOLDER_VALUE);
        br.item(BYTE_RANGE_PLACEHOLDER_VALUE);
        br.item(BYTE_RANGE_PLACEHOLDER_VALUE);
        br.finish();
        // /Contents placeholder — ASCII literal-string path of
        // pdf-writer's `Str` writer.
        sig_dict.pair(pdf_writer::Name(b"Contents"), Str(&contents_placeholder));
        if let Some(reason) = &settings.reason {
            sig_dict.pair(pdf_writer::Name(b"Reason"), TextStr(reason));
        }
        if let Some(location) = &settings.location {
            sig_dict.pair(pdf_writer::Name(b"Location"), TextStr(location));
        }
        if let Some(contact_info) = &settings.contact_info {
            sig_dict.pair(pdf_writer::Name(b"ContactInfo"), TextStr(contact_info));
        }
        if let Some(signer_name) = &settings.signer_name {
            sig_dict.pair(pdf_writer::Name(b"Name"), TextStr(signer_name));
        }
        if let Some(signing_time) = &settings.signing_time {
            // `/M` is a date object literal (ISO 32000-2 §7.9.4
            // Table 7). pdf-writer's `Date` writer takes a year
            // and gradually adds month/day/etc. Parse the supplied
            // string conservatively: when it matches
            // `D:YYYYMMDDHHMMSS` (with optional `Z` / `+HH'MM'`
            // suffix) we route through `Date::new(year)`; otherwise
            // we surface the raw bytes via `Str` so the embedder
            // can supply non-standard date formats without
            // krilla rejecting them.
            if let Some(date) = parse_pdf_date(signing_time) {
                sig_dict.pair(pdf_writer::Name(b"M"), date);
            } else {
                sig_dict.pair(pdf_writer::Name(b"M"), Str(signing_time.as_bytes()));
            }
        }
        // Drop closes the dict and emits `>>\nendobj\n`.
        drop(sig_dict);

        let _ = settings;
        Ok(())
    }
}

/// Parse a PDF date string of the canonical form
/// `D:YYYYMMDDHHmmSS[Z|+HH'mm'|-HH'mm']` into a [`pdf_writer::Date`].
///
/// Returns `None` if the input does not match — the caller then
/// falls back to a `Str` literal so embedder-supplied date strings
/// outside the canonical grammar still land verbatim in `/M`.
fn parse_pdf_date(s: &str) -> Option<pdf_writer::Date> {
    let body = s.strip_prefix("D:").unwrap_or(s);
    if body.len() < 4 {
        return None;
    }
    let year: u16 = body.get(0..4)?.parse().ok()?;
    let mut date = pdf_writer::Date::new(year);
    let mut cursor = 4;
    if let Some(month) = body
        .get(cursor..cursor + 2)
        .and_then(|s| s.parse::<u8>().ok())
    {
        date = date.month(month);
        cursor += 2;
    }
    if let Some(day) = body
        .get(cursor..cursor + 2)
        .and_then(|s| s.parse::<u8>().ok())
    {
        date = date.day(day);
        cursor += 2;
    }
    if let Some(hour) = body
        .get(cursor..cursor + 2)
        .and_then(|s| s.parse::<u8>().ok())
    {
        date = date.hour(hour);
        cursor += 2;
    }
    if let Some(minute) = body
        .get(cursor..cursor + 2)
        .and_then(|s| s.parse::<u8>().ok())
    {
        date = date.minute(minute);
        cursor += 2;
    }
    if let Some(second) = body
        .get(cursor..cursor + 2)
        .and_then(|s| s.parse::<u8>().ok())
    {
        date = date.second(second);
        cursor += 2;
    }
    // Time-zone suffix (optional). `Z` is the no-op UTC marker.
    // `+HH'mm'` or `-HH'mm'` set the offset.
    if let Some(tz_byte) = body.as_bytes().get(cursor) {
        match tz_byte {
            b'Z' => {}
            b'+' | b'-' => {
                let sign: i8 = if *tz_byte == b'-' { -1 } else { 1 };
                cursor += 1;
                if let Some(hour) = body
                    .get(cursor..cursor + 2)
                    .and_then(|s| s.parse::<i8>().ok())
                {
                    date = date.utc_offset_hour(sign * hour);
                    cursor += 2;
                    // Skip the optional apostrophe separator.
                    if body.as_bytes().get(cursor) == Some(&b'\'') {
                        cursor += 1;
                    }
                    if let Some(minute) = body
                        .get(cursor..cursor + 2)
                        .and_then(|s| s.parse::<u8>().ok())
                    {
                        date = date.utc_offset_minute(minute);
                    }
                }
            }
            _ => {}
        }
    }
    Some(date)
}

pub(crate) struct EmbeddedPdfChunk {
    pub(crate) original_chunk: Chunk,
    pub(crate) root_ref_mappings: HashMap<Ref, Ref>,
    pub(crate) new_chunk: OnceLock<Chunk>,
}

/// Visits all chunks in a type.
trait Visit {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()>;
}

impl Visit for EmbeddedPdfChunk {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        // Now, we have a chunk that contains everything we need to fully embed the PDF, including
        // the pages we wanted to extract into, as well as all their dependencies. The
        // problem is: during the document creation, we already assigned references to the
        // pages (stored in `SerializerContex::page_infos`), but `hayro_write` created new references
        // for those (stored in `result.root_refs`).

        // Because of this, embedded PDF chunks will be renumbered twice: First, we preprocess the
        // chunk such that page/XObjects are reassigned their original references from the serialize
        // context, and all other objects are assigned new, unique references provided by the
        // serialize context. Then, we renumber them once again by treating them like any other chunk.

        // Since we are calling `visit` twice, we also cache the renumbered chunk.

        let renumbered = self.new_chunk.get_or_init(|| {
            let mut remapper = self.root_ref_mappings.clone();

            self.original_chunk
                .renumber(|old| *remapper.entry(old).or_insert_with(|| sc.new_ref()))
        });

        renumbered.visit(sc, f)
    }
}

impl Visit for ChunkContainer {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        self.non_stream.visit(sc, f)?;
        self.mixed.visit(sc, f)?;
        self.streams.visit(sc, f)?;
        Ok(())
    }
}

impl Visit for StreamChunks {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        self.fonts.visit(sc, f)?;
        self.shading_functions.visit(sc, f)?;
        self.patterns.visit(sc, f)?;
        self.pages.visit(sc, f)?;
        self.embedded_files.visit(sc, f)?;
        self.icc_profiles.visit(sc, f)?;
        self.x_objects.visit(sc, f)?;
        self.images.visit(sc, f)?;

        Ok(())
    }
}

impl Visit for MixedChunks {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        self.embedded_pdfs.visit(sc, f)?;

        Ok(())
    }
}

impl Visit for NonStreamChunks {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        self.page_tree.visit(sc, f)?;
        self.outline.visit(sc, f)?;
        self.page_label_tree.visit(sc, f)?;
        self.destination_profiles.visit(sc, f)?;
        self.struct_tree_root.visit(sc, f)?;
        self.struct_elements.visit(sc, f)?;
        self.page_labels.visit(sc, f)?;
        self.annotations.visit(sc, f)?;
        self.color_spaces.visit(sc, f)?;
        self.destinations.visit(sc, f)?;
        self.ext_g_states.visit(sc, f)?;
        self.resource_dictionaries.visit(sc, f)?;
        self.masks.visit(sc, f)?;
        self.fonts.visit(sc, f)?;
        self.shading_functions.visit(sc, f)?;
        self.patterns.visit(sc, f)?;
        self.pages.visit(sc, f)?;
        self.embedded_files.visit(sc, f)?;

        Ok(())
    }
}

impl Visit for Chunk {
    fn visit(&self, _: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        f(self);
        Ok(())
    }
}

impl Visit for Option<Chunk> {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        if let Some(chunk) = self {
            chunk.visit(sc, f)?;
        }
        Ok(())
    }
}

impl Visit for Option<(Ref, Chunk)> {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        if let Some((_, chunk)) = self {
            chunk.visit(sc, f)?;
        }
        Ok(())
    }
}

impl<T: Visit + Send + Sync + 'static> Visit for Deferred<T> {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        self.wait().visit(sc, f)
    }
}

impl<T: Visit> Visit for KrillaResult<T> {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        self.as_ref().map_err(|e| e.clone())?.visit(sc, f)
    }
}

impl<T: Visit> Visit for Vec<T> {
    fn visit(&self, sc: &mut SerializeContext, f: &mut impl FnMut(&Chunk)) -> KrillaResult<()> {
        for field in self {
            field.visit(sc, f)?;
        }
        Ok(())
    }
}
