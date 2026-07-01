//! Creating new PDF documents.
//!
//! When using krilla, the starting point is always the creation of a [`Document`]. A document
//! represents _one_ PDF document, to which you can add pages or configure them in any
//! other way you want.
//!
//! Unfortunately, creating PDFs always requires some kind of global state to keep track
//! of different aspects in the creation process, meaning that it is not possible to
//! generate multiple pages at the same time. Instead, you need to add pages separately
//! by calling the [`Document::start_page`] method, which returns a new [`Page`] object that mutably
//! borrows the global state from the document. Once the page is dropped, the global
//! state is passed back to the original document, which you can then use to add even
//! more pages.
//!
//! [`Page`]: Page

use crate::chunk_container::ChunkContainer;
use crate::destination::NamedDestination;
use crate::error::KrillaResult;
use crate::interactive::signature::DigitalSignature;
use crate::interchange::embed::EmbeddedFile;
use crate::interchange::metadata::Metadata;
use crate::interchange::outline::Outline;
use crate::interchange::tagging::TagTree;
use crate::page::{Page, PageSettings};
#[cfg(feature = "pdf")]
use crate::pdf::PdfDocument;
use crate::serialize::{SerializeContext, SerializeSettings};
use crate::surface::Location;

/// A PDF document.
pub struct Document {
    pub(crate) serializer_context: SerializeContext,
    pub(crate) chunk_container: ChunkContainer,
    /// Digital signature to apply on `finish`. `None` leaves any
    /// [`SignatureField`](crate::annotation::SignatureField)
    /// widgets unsigned (placeholder structure only). When `Some`,
    /// `finish` emits a real `/Sig` indirect dictionary, wires the
    /// widget's `/V`, sets `/AcroForm /SigFlags 3` and patches the
    /// signature bytes into the buffer after serialisation.
    pub(crate) digital_signature: Option<DigitalSignature>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Create a new document with default serialize settings.
    pub fn new() -> Self {
        let serializer_context = SerializeContext::new(SerializeSettings::default());
        let chunk_container = ChunkContainer::new(&serializer_context);

        Self {
            serializer_context,
            chunk_container,
            digital_signature: None,
        }
    }

    /// Create a new document with custom serialize settings.
    pub fn new_with(serialize_settings: SerializeSettings) -> Self {
        let serializer_context = SerializeContext::new(serialize_settings);
        let chunk_container = ChunkContainer::new(&serializer_context);

        Self {
            serializer_context,
            chunk_container,
            digital_signature: None,
        }
    }

    /// Attach a digital signature to this document.
    ///
    /// Configures krilla to emit a real `/Sig` indirect dictionary
    /// (ISO 32000-2 §12.8) wired to every
    /// [`SignatureField`](crate::annotation::SignatureField)
    /// widget the document already carries via
    /// [`crate::page::Page::add_annotation`]. The signer
    /// callback inside `signature` is invoked exactly once during
    /// [`Self::finish`], after the PDF byte buffer has been
    /// produced but before it is returned to the caller — it
    /// receives the bytes covered by `/ByteRange` and must
    /// produce a DER-encoded PKCS#7 / CMS `SignedData` structure
    /// that fits inside the reservation declared by
    /// `DigitalSignature::placeholder_size_bytes`.
    ///
    /// Calling this method twice on the same document is not
    /// supported — the second call replaces the first signer.
    pub fn with_digital_signature(mut self, signature: DigitalSignature) -> Self {
        self.serializer_context.enable_signing();
        self.digital_signature = Some(signature);
        self
    }

    /// Start a new page with default settings.
    pub fn start_page(&mut self) -> Page<'_> {
        let page_index = self.serializer_context.page_infos().iter().len();
        Page::new(
            &mut self.serializer_context,
            &mut self.chunk_container,
            page_index,
            PageSettings::default(),
        )
    }

    /// Start a new page with specific page settings.
    pub fn start_page_with(&mut self, page_settings: PageSettings) -> Page<'_> {
        let page_index = self.serializer_context.page_infos().iter().len();
        Page::new(
            &mut self.serializer_context,
            &mut self.chunk_container,
            page_index,
            page_settings,
        )
    }

    /// Embed the pages (0-indexed) from the given
    /// PDF document.
    #[cfg(feature = "pdf")]
    pub fn embed_pdf_pages(&mut self, pdf: &PdfDocument, page_indices: &[usize]) {
        self.serializer_context.embed_pdf_pages(pdf, page_indices);
    }

    /// Set the location that should be assumed for subsequent operations.
    pub fn set_location(&mut self, location: Location) {
        self.serializer_context.set_location(location);
    }

    /// Reset the location that should be assumed for subsequent operations.
    pub fn reset_location(&mut self) {
        self.serializer_context.reset_location();
    }

    /// Set the outline of the document.
    pub fn set_outline(&mut self, outline: Outline) {
        self.serializer_context.set_outline(outline);
    }

    /// Set the metadata of the document.
    pub fn set_metadata(&mut self, metadata: Metadata) {
        self.chunk_container.metadata = Some(metadata);
    }

    /// Set the tag tree of the document.
    pub fn set_tag_tree(&mut self, tag_tree: TagTree) {
        self.serializer_context.set_tag_tree(tag_tree);
    }

    /// Register an external structure-element namespace (e.g.
    /// MathML, HTML 4, PDF Math) by URI. Returns a stable
    /// [`NamespaceHandle`](crate::tagging::NamespaceHandle) that
    /// can be passed through
    /// [`TagNamespace::Custom`](crate::tagging::TagNamespace::Custom)
    /// to bind a structure element to the namespace.
    ///
    /// PDF 2.0 (ISO 32000-2 §14.8.6) only — krilla allocates the
    /// indirect ref eagerly and writes the corresponding
    /// `Namespace` dict at finalise time. Pre-2.0 documents
    /// silently discard the registration (no `/Namespaces` array
    /// exists on those versions). Repeated calls with the same
    /// URI are idempotent — the same handle is returned and the
    /// on-disk PDF carries at most one `Namespace` dict per URI.
    pub fn register_namespace(
        &mut self,
        uri: impl Into<String>,
    ) -> crate::tagging::NamespaceHandle {
        self.serializer_context.register_namespace(uri)
    }

    /// Register an optional content group (PDF "layer") with the
    /// document. The returned [`LayerHandle`](crate::optional_content::LayerHandle)
    /// can then be passed to
    /// [`Surface::push_layer`](crate::surface::Surface::push_layer) to
    /// bracket drawing operations that should be hidden or shown
    /// together.
    ///
    /// krilla allocates the OCG's indirect ref eagerly and writes the
    /// catalogue's `/OCProperties` dictionary at finalise time; the
    /// caller does not need to write any further structure.
    pub fn add_layer(
        &mut self,
        layer: crate::optional_content::Layer,
    ) -> crate::optional_content::LayerHandle {
        self.serializer_context.add_layer(layer)
    }

    /// Embed a new file in the PDF document.
    ///
    /// Returns `None` if the file couldn't be embedded because a file
    /// with the same name has already been embedded.
    pub fn embed_file(&mut self, file: EmbeddedFile) -> Option<()> {
        self.serializer_context
            .embed_file(&mut self.chunk_container, file)
    }

    /// Manually register a global named destination.
    ///
    /// Named destinations used in link annotations are automatically registered, so you don't need
    /// to call this function for them.
    ///
    /// Returns `None` if a named destination with the same name and a different destination has
    /// already been registered, and therefore could not be registered again.
    #[must_use]
    pub fn register_named_destination(&mut self, dest: NamedDestination) -> Option<()> {
        self.serializer_context
            .register_named_destination(dest)
            .map(|_| ())
    }

    /// Attempt to export the document to a PDF file.
    pub fn finish(mut self) -> KrillaResult<Vec<u8>> {
        // Write empty page if none has been created yet.
        if self.serializer_context.page_infos().is_empty() {
            self.start_page();
        }

        let Self {
            serializer_context,
            mut chunk_container,
            digital_signature,
        } = self;

        // Stash the signature metadata onto the chunk container so the
        // catalogue-emit path can write the `/Sig` indirect dict body
        // and set `/AcroForm /SigFlags`. The signer callback itself
        // stays here — `finish` returns it (alongside the buffer) so
        // we can invoke it post-emit.
        if let Some(ref sig) = digital_signature {
            chunk_container.set_signature_emission_settings(
                sig.sub_filter,
                sig.placeholder_size_bytes,
                sig.reason.clone(),
                sig.location.clone(),
                sig.contact_info.clone(),
                sig.signer_name.clone(),
                sig.signing_time.clone(),
            );
        }

        let buffer = serializer_context.finish(chunk_container)?;

        if let Some(signature) = digital_signature {
            crate::interactive::signature::patch_signature(buffer, signature)
        } else {
            Ok(buffer)
        }
    }
}
