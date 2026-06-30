use pdf_writer::{Chunk, Finish, Name, Pdf, Ref, Str, TextStr};
use std::collections::HashMap;
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
        }
    }

    pub(crate) fn finish(self, sc: &mut SerializeContext) -> KrillaResult<Pdf> {
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
            let effective_display_doc_title = vp_struct
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

            let settings = sc.serialize_settings();
            let validators = settings.validators();
            let write_embedded_files = self.non_stream.embedded_files.len() != 0
                || validators.requires_embedded_files_when_empty();

            if !named_destinations.is_empty() || write_embedded_files {
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

                    for (name, _ref) in &embedded_files {
                        embedded_name_entries.insert(Str(name.as_bytes()), remapper[_ref]);
                    }
                }
            }

            if !embedded_files.is_empty() && settings.supports_associated_files() {
                let mut associated_files = catalog.insert(Name(b"AF")).array().typed();
                for _ref in embedded_files.values() {
                    associated_files.item(remapper[_ref]).finish();
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
            if !widget_fields.is_empty() {
                let mut acro_form = catalog.insert(Name(b"AcroForm")).dict();
                let mut fields = acro_form.insert(Name(b"Fields")).array();
                for field_ref in &widget_fields {
                    fields.item(remapper[field_ref]);
                }
                fields.finish();
                acro_form.pair(Name(b"NeedAppearances"), true);
                acro_form.finish();
            }

            catalog.finish();
        }

        Ok(pdf)
    }
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
