//! Working with pages of a PDF document.

use std::cell::OnceCell;
use std::num::NonZeroU32;
use std::ops::DerefMut;

use pdf_writer::types::TabOrder;
use pdf_writer::writers::NumberTree;
use pdf_writer::{Chunk, Finish, Name, Ref, TextStr};

use crate::chunk_container::ChunkContainer;
use crate::configure::validate::VersionedFeature;
use crate::configure::{PdfVersion, ValidationError};
use crate::content::ContentBuilder;
use crate::error::KrillaResult;
use crate::geom::{Rect, Size, Transform};
use crate::interactive::annotation::{
    Annotation, RadioGroupChild, RadioGroupField, WidgetAnnotation, WidgetField,
};
use crate::interchange::tagging::{Identifier, PageTagIdentifier};
use crate::resource::ResourceDictionary;
use crate::serialize::{PageInfo, SerializeContext};
use crate::stream::{FilterStreamBuilder, Stream};
use crate::surface::Surface;
use crate::tagging::AnnotationIdentifier;
use crate::util::Deferred;

#[derive(Clone, Debug)]
/// The settings of a page.
pub struct PageSettings {
    /// The media box of the page, which defines the visible area of the surface.
    media_box: Option<Rect>,
    /// The page label of the page.
    page_label: PageLabel,
    /// The size of the surface.
    surface_size: Size,
    /// The crop box of the page
    crop_box: Option<Rect>,
    /// The bleed box of the page
    bleed_box: Option<Rect>,
    /// The trim box of the page
    trim_box: Option<Rect>,
    /// The actual content boundaries
    art_box: Option<Rect>,
    /// The number of degrees the page should be rotated clockwise when displayed.
    /// Must be a multiple of 90.
    rotate: Option<i32>,
}

impl PageSettings {
    /// Create new page settings and define the size of the page surface.
    pub fn new(size: Size) -> Self {
        Self {
            media_box: Some(Rect::from_xywh(0.0, 0.0, size.width(), size.height()).unwrap()),
            surface_size: size,
            ..Default::default()
        }
    }

    /// Try to create new page settings with the specified width and height.
    ///
    /// Returns `None` if either the width or the height is not > 0.
    pub fn from_wh(width: f32, height: f32) -> Option<Self> {
        Some(Self::new(Size::from_wh(width, height)?))
    }

    /// Change the media box.
    ///
    /// The media box defines the visible area of the page when opening the PDF,
    /// so it can be distinct from the size of the surface, but in the majority
    /// of the cases you want them to match in size and align the media box
    /// at the origin of the coordinate system.
    ///
    /// If set to `None`, the dimensions will be chosen in such a way that all
    /// contents fit on the page.
    pub fn with_media_box(mut self, media_box: Option<Rect>) -> PageSettings {
        self.media_box = media_box;
        self
    }

    /// Change the page label.
    pub fn with_page_label(mut self, page_label: PageLabel) -> PageSettings {
        self.page_label = page_label;
        self
    }

    /// The current media box.
    pub(crate) fn media_box(&self) -> Option<Rect> {
        self.media_box
    }

    /// The current surface size.
    pub(crate) fn surface_size(&self) -> Size {
        self.surface_size
    }

    /// The current page label.
    pub(crate) fn page_label(&self) -> &PageLabel {
        &self.page_label
    }

    /// Change the crop box.
    ///
    /// The crop box defines the region to which the page contents are to be clipped
    /// when displayed or printed. Default is the media box.
    ///
    /// If `None`, no /CropBox attribute will be written to the page.
    pub fn with_crop_box(mut self, crop_box: Option<Rect>) -> PageSettings {
        self.crop_box = crop_box;
        self
    }

    /// The current crop box.
    pub(crate) fn crop_box(&self) -> Option<Rect> {
        self.crop_box
    }

    /// Change the bleed box.
    ///
    /// The bleed box defines the region to which the page contents needs to be clipped
    /// when output in a production environment. It includes any extra bleed area needed
    /// for printing.
    ///
    /// If `None`, no /BleedBox attribute will be written to the page.
    pub fn with_bleed_box(mut self, bleed_box: Option<Rect>) -> PageSettings {
        self.bleed_box = bleed_box;
        self
    }

    /// The current bleed box.
    pub(crate) fn bleed_box(&self) -> Option<Rect> {
        self.bleed_box
    }

    /// Change the trim box.
    ///
    /// The trim box defines the intended dimensions of the finished page after trimming.
    /// It may be smaller than the media box and bleed box to accommodate bleed for printing.
    ///
    /// If `None`, no /TrimBox attribute will be written to the page.
    pub fn with_trim_box(mut self, trim_box: Option<Rect>) -> PageSettings {
        self.trim_box = trim_box;
        self
    }

    /// The current trim box.
    pub(crate) fn trim_box(&self) -> Option<Rect> {
        self.trim_box
    }

    /// Change the art box.
    ///
    /// The art box defines the extent of the page's meaningful content (including
    /// potential white space) as intended by the page's creator.
    ///
    /// If `None`, no /ArtBox attribute will be written to the page.
    pub fn with_art_box(mut self, art_box: Option<Rect>) -> PageSettings {
        self.art_box = art_box;
        self
    }

    /// The current art box.
    pub(crate) fn art_box(&self) -> Option<Rect> {
        self.art_box
    }

    /// Change the page rotation.
    ///
    /// The number of degrees the page should be rotated clockwise when
    /// displayed. Must be a multiple of 90. Common values: 0, 90, 180, 270.
    ///
    /// If `None`, no `/Rotate` attribute will be written to the page.
    pub fn with_rotate(mut self, rotate: Option<i32>) -> PageSettings {
        self.rotate = rotate;
        self
    }

    /// The current rotation.
    pub(crate) fn rotate(&self) -> Option<i32> {
        self.rotate
    }
}

impl Default for PageSettings {
    fn default() -> Self {
        // Default for A4.
        let width = 595.0;
        let height = 842.0;

        Self {
            media_box: Some(Rect::from_xywh(0.0, 0.0, width, height).unwrap()),
            surface_size: Size::from_wh(width, height).unwrap(),
            page_label: PageLabel::default(),
            crop_box: None,
            bleed_box: None,
            trim_box: None,
            art_box: None,
            rotate: None,
        }
    }
}

/// A single page.
///
/// You cannot create an instance of this type yourself. Instead, you should use the
/// [`Document::start_page`] (or a related method) to add a new page to a document. In most cases, all
/// you need to do is to call the [`Page::surface`] method, so you can start drawing on the page.
/// However, there are a few other operations you can perform, such as adding annotations
/// to a page.
///
/// [`Document::start_page`]: crate::Document::start_page
pub struct Page<'a> {
    sc: &'a mut SerializeContext,
    chunk_container: &'a mut ChunkContainer,
    page_settings: PageSettings,
    page_index: usize,
    page_stream: Stream,
    num_mcids: i32,
    annotations: Vec<Annotation>,
    radio_groups: Vec<RadioGroupPayload>,
}

/// Internal record produced by [`Page::add_radio_group`] — the
/// pre-allocated parent ref together with the field metadata needed
/// to emit the parent `Btn` dict during page serialisation. Child
/// widget annotations are pushed onto `Page::annotations` and carry
/// the same `parent_ref` via [`WidgetField::RadioGroupChild`].
pub(crate) struct RadioGroupPayload {
    pub(crate) parent_ref: Ref,
    pub(crate) name: String,
    pub(crate) selected_export: Option<String>,
    pub(crate) default_selected_export: Option<String>,
    pub(crate) ff_bits: u32,
    /// Indirect refs of the child widget annotations in `/Kids` order.
    /// Populated by [`InternalPage::serialize`] once each child has
    /// been allocated its annotation ref.
    pub(crate) kid_refs: Vec<Ref>,
}

impl<'a> Page<'a> {
    pub(crate) fn new(
        sc: &'a mut SerializeContext,
        chunk_container: &'a mut ChunkContainer,
        page_index: usize,
        page_settings: PageSettings,
    ) -> Self {
        Self {
            sc,
            chunk_container,
            page_settings,
            page_index,
            num_mcids: 0,
            page_stream: Stream::empty(),
            annotations: vec![],
            radio_groups: vec![],
        }
    }

    pub(crate) fn root_transform(&self) -> Transform {
        page_root_transform(self.page_settings.surface_size().height())
    }

    /// Add an annotation to the page.
    pub fn add_annotation(&mut self, annotation: Annotation) {
        self.annotations.push(annotation);
    }

    /// Attach a mutually-exclusive radio-button group (ISO 32000-2
    /// §12.7.5.2.3) to the page.
    ///
    /// HTML `<input type="radio">` elements that share a `name` form a
    /// single AcroForm field with a non-terminal `/Btn` parent and one
    /// child widget annotation per radio. krilla pre-allocates the
    /// parent's indirect reference, pushes each child onto the page's
    /// annotation list, and queues the parent dict for emission
    /// alongside the catalogue's `/AcroForm /Fields` array. The
    /// children appear in the page's `/Annots`; the parent is reached
    /// via `/Kids` traversal.
    ///
    /// Each child carries `/AS /<export>` when selected or `/AS /Off`
    /// otherwise; the parent's `/V` names the selected child (or
    /// `/Off` when no radio is checked). The Radio flag (`/Ff` bit
    /// 16) is forced on; the embedder controls `RadiosInUnison`
    /// (bit 26) and `ReadOnly` (bit 1) via
    /// [`RadioGroupField::flags`].
    pub fn add_radio_group(&mut self, group: RadioGroupField) {
        let parent_ref = self.sc.new_ref();
        // The Radio flag must always be set on a radio-group parent;
        // we OR it in regardless of how the caller configured `flags`
        // so a bare `ButtonFieldFlags::default()` still produces a
        // conforming field.
        let mut flags = group.flags;
        flags = flags.with_radio(true);
        let ff_bits = flags.to_bits();
        for child in group.children {
            let selected = group
                .selected_export
                .as_deref()
                .is_some_and(|sel| sel == child.export_value);
            let widget = WidgetAnnotation::new(
                child.rect,
                // Children have no /T of their own. We still pass a
                // partial name through the constructor for symmetry,
                // but `WidgetAnnotation::serialize_type` skips /T for
                // radio-group children.
                String::new(),
                WidgetField::RadioGroupChild(RadioGroupChild::new(
                    parent_ref,
                    child.export_value,
                    selected,
                )),
            );
            self.annotations.push(Annotation::new_widget(widget, None));
        }
        self.radio_groups.push(RadioGroupPayload {
            parent_ref,
            name: group.name,
            selected_export: group.selected_export,
            default_selected_export: group.default_selected_export,
            ff_bits,
            kid_refs: Vec::new(),
        });
    }

    /// Add a tagged annotation to the page.
    pub fn add_tagged_annotation(&mut self, mut annotation: Annotation) -> Identifier {
        let annot_index = self.annotations.len();
        let ai = AnnotationIdentifier::new(self.page_index, annot_index);
        let struct_parent = self.sc.register_annotation_parent(ai);
        annotation.struct_parent = struct_parent;
        self.add_annotation(annotation);

        match struct_parent {
            None => Identifier::dummy(),
            Some(_) => Identifier::new_annotation(self.page_index, annot_index),
        }
    }

    /// Get the surface of the page to draw on. Calling this multiple times
    /// on the same page will reset any previous drawings.
    pub fn surface(&mut self) -> Surface<'_> {
        let root_builder = ContentBuilder::new(
            self.root_transform(),
            self.page_settings.media_box.is_none(),
            self.sc,
        );

        let finish_fn = Box::new(|stream, num_mcids| {
            self.page_stream = stream;
            self.num_mcids = num_mcids;
        });

        let page_identifier = if self.sc.serialize_settings().enable_tagging {
            Some(PageTagIdentifier::new(self.page_index, 0))
        } else {
            None
        };

        Surface::new(
            self.sc,
            self.chunk_container,
            root_builder,
            page_identifier,
            finish_fn,
        )
    }

    /// A shorthand for `std::mem::drop`.
    pub fn finish(self) {}
}

pub(crate) fn page_root_transform(height: f32) -> Transform {
    Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, height)
}

/// Emit a non-terminal `/Btn` radio-group parent dict into `chunk`
/// per ISO 32000-2 §12.7.5.2.3.
///
/// The parent dict carries `/FT /Btn`, `/T <name>`, `/Ff <flags>`,
/// `/V /<selected_export | Off>`, `/DV /<default_export | Off>` and
/// `/Kids [<child refs>]`. No `/Rect`: the parent is not itself a
/// page annotation. Children are reached via tree traversal of
/// `/Kids` and carry `/Parent` pointing back here.
fn emit_radio_group_parent(chunk: &mut Chunk, group: &RadioGroupPayload) {
    let mut field = chunk.indirect(group.parent_ref).dict();
    field.pair(Name(b"FT"), Name(b"Btn"));
    field.pair(Name(b"T"), TextStr(&group.name));
    field.pair(Name(b"Ff"), group.ff_bits as i32);

    let selected_bytes: Vec<u8> = match group.selected_export.as_deref() {
        Some(name) => name.as_bytes().to_vec(),
        None => b"Off".to_vec(),
    };
    field.pair(Name(b"V"), Name(&selected_bytes));

    let default_bytes: Vec<u8> = match group.default_selected_export.as_deref() {
        Some(name) => name.as_bytes().to_vec(),
        None => b"Off".to_vec(),
    };
    field.pair(Name(b"DV"), Name(&default_bytes));

    let mut kids = field.insert(Name(b"Kids")).array();
    for kid in &group.kid_refs {
        kids.item(*kid);
    }
    kids.finish();
    field.finish();
}

impl Drop for Page<'_> {
    fn drop(&mut self) {
        // Since we cannot take ownership in `drop`, just make use `mem::take` to pick
        // what we need.
        let annotations = std::mem::take(&mut self.annotations);
        let radio_groups = std::mem::take(&mut self.radio_groups);
        let page_settings = std::mem::take(&mut self.page_settings);

        let struct_parent = self
            .sc
            .register_page_struct_parent(self.page_index, self.num_mcids);

        let stream = std::mem::replace(&mut self.page_stream, Stream::empty());
        let page = InternalPage::new(
            stream,
            self.sc,
            annotations,
            radio_groups,
            struct_parent,
            page_settings,
            self.page_index,
        );
        self.sc.register_page(page);
    }
}

/// The numbering style of a page label.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum NumberingStyle {
    /// Arabic numerals.
    Arabic,
    /// Lowercase Roman numerals.
    LowerRoman,
    /// Uppercase Roman numerals.
    UpperRoman,
    /// Lowercase letters (a-z, then aa-zz, ...).
    LowerAlpha,
    /// Uppercase letters (A-Z, then AA-ZZ, ...).
    UpperAlpha,
}

impl NumberingStyle {
    fn to_pdf(self) -> pdf_writer::types::NumberingStyle {
        match self {
            NumberingStyle::Arabic => pdf_writer::types::NumberingStyle::Arabic,
            NumberingStyle::LowerRoman => pdf_writer::types::NumberingStyle::LowerRoman,
            NumberingStyle::UpperRoman => pdf_writer::types::NumberingStyle::UpperRoman,
            NumberingStyle::LowerAlpha => pdf_writer::types::NumberingStyle::LowerAlpha,
            NumberingStyle::UpperAlpha => pdf_writer::types::NumberingStyle::UpperAlpha,
        }
    }
}

pub(crate) struct InternalPage {
    pub stream_ref: Ref,
    pub stream_resources: ResourceDictionary,
    pub stream_chunk: Deferred<Chunk>,
    pub page_settings: PageSettings,
    pub page_index: usize,
    pub struct_parent: Option<i32>,
    pub bbox: Rect,
    pub annotations: Vec<Annotation>,
    pub radio_groups: Vec<RadioGroupPayload>,
}

impl InternalPage {
    pub(crate) fn new(
        mut stream: Stream,
        sc: &mut SerializeContext,
        annotations: Vec<Annotation>,
        radio_groups: Vec<RadioGroupPayload>,
        struct_parent: Option<i32>,
        page_settings: PageSettings,
        page_index: usize,
    ) -> Self {
        for validation_error in stream.validation_errors {
            sc.register_validation_error(validation_error)
        }

        let stream_ref = sc.new_ref();
        let serialize_settings = sc.serialize_settings().clone();
        let stream_resources = std::mem::take(&mut stream.resource_dictionary);
        let mut chunk = sc.new_chunk();

        let stream_chunk = Deferred::new(move || {
            let page_stream =
                FilterStreamBuilder::new_from_content_stream(&stream.content, &serialize_settings)
                    .finish(&serialize_settings.clone());

            let mut stream = chunk.stream(stream_ref, page_stream.encoded_data());
            page_stream.write_filters(stream.deref_mut());

            stream.finish();
            chunk
        });

        Self {
            stream_resources,
            stream_ref,
            stream_chunk,
            struct_parent,
            bbox: stream.bbox,
            annotations,
            radio_groups,
            page_settings,
            page_index,
        }
    }

    pub(crate) fn serialize(
        self,
        sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    ) -> KrillaResult<()> {
        // PDF/X (ISO 15930-* §6) requires every page to carry either a
        // TrimBox or an ArtBox. Surface the omission before emitting the
        // page dict so the error path catches it.
        if sc
            .serialize_settings()
            .validators()
            .requires_trim_or_art_box()
            && self.page_settings.trim_box().is_none()
            && self.page_settings.art_box().is_none()
        {
            sc.register_validation_error(ValidationError::MissingTrimOrArtBox(
                self.page_index,
                None,
            ));
        }

        let mut annotation_refs = vec![];
        let mut radio_groups = self.radio_groups;

        if !self.annotations.is_empty() {
            for annotation in &self.annotations {
                let annot_ref = sc.new_ref();

                annotation.serialize(
                    sc,
                    chunk_container,
                    annot_ref,
                    self.page_settings.surface_size().height(),
                )?;
                annotation_refs.push((annot_ref, OnceCell::new()));

                // Match radio-group child annotations back to their
                // parent payload so the parent's `/Kids` array can be
                // populated in insertion order.
                if let Some(parent_ref) = annotation.radio_group_parent_ref() {
                    if let Some(group) = radio_groups
                        .iter_mut()
                        .find(|g| g.parent_ref == parent_ref)
                    {
                        group.kid_refs.push(annot_ref);
                    }
                }
            }
        }

        // Emit one non-terminal `Btn` dict per radio group (ISO 32000-2
        // §12.7.5.2.3). The parent dict carries `/T`, `/V`, `/DV`,
        // `/Ff` (Radio flag) and `/Kids`; no `/Rect` because the
        // parent is not itself a page annotation. The parent ref is
        // registered with the document catalogue so it appears in
        // `/AcroForm /Fields` exactly once.
        for group in &radio_groups {
            emit_radio_group_parent(&mut chunk_container.non_stream.pages, group);
            sc.register_widget_field(group.parent_ref);
        }

        let chunk = &mut chunk_container.non_stream.pages;
        let mut page = chunk.page(root_ref);
        self.stream_resources.to_pdf_resources(
            &mut page,
            sc,
            &mut chunk_container.non_stream.resource_dictionaries,
        );

        let transform_rect = |rect: Rect| {
            rect.transform(page_root_transform(
                self.page_settings.surface_size().height(),
            ))
            .unwrap()
        };

        // media box is mandatory, so we need to fall back to the default bbox
        let media_box = transform_rect(self.page_settings.media_box().unwrap_or(self.bbox));
        page.media_box(media_box.to_pdf_rect());

        // the remaining type of box are not mandatory, so we only set them if they are present
        if let Some(crop_box) = self.page_settings.crop_box() {
            let crop_box = transform_rect(crop_box);
            page.crop_box(crop_box.to_pdf_rect());
        }

        if let Some(bleed_box) = self.page_settings.bleed_box() {
            let bleed_box = transform_rect(bleed_box);
            page.bleed_box(bleed_box.to_pdf_rect());
        }

        if let Some(trim_box) = self.page_settings.trim_box() {
            let trim_box = transform_rect(trim_box);
            page.trim_box(trim_box.to_pdf_rect());
        }

        if let Some(art_box) = self.page_settings.art_box() {
            let art_box = transform_rect(art_box);
            page.art_box(art_box.to_pdf_rect());
        }

        if let Some(rotate) = self.page_settings.rotate() {
            page.rotate(rotate);
        }

        if let Some(struct_parent) = self.struct_parent {
            page.struct_parents(struct_parent);
        }

        // Only required for PDF/UA, but might as well always set it if there
        // are annotations.
        //
        // Since the navigation order of annotations only has an effect if there
        // are annotations, we usually only set the key in that case. However,
        // the accessibility audit in Adobe Acrobat always [requires the key to
        // be set][1], even if there are no annotations (perhaps the rationale
        // is that the user can add some). Hence, we check if there is an
        // accessibility validator or one requiring tags to decide whether to
        // force the key.
        //
        // Since forcing `/Tabs S` unconditionally for PDF/A and PDF/UA files
        // targeting PDF 1.4 and below would unconditionally raise a
        // `RequiresNewerPdfVersion(StructureOrderTabbing, _)` error, we also
        // check the target version.
        //
        // [1]: https://helpx.adobe.com/acrobat/using/create-verify-pdf-accessibility.html#TabOrder "Create and verify PDF accessibility (Acrobat Pro): Tab order"
        if (!self.annotations.is_empty()
            || ((sc
                .serialize_settings()
                .validators()
                .accessibility()
                .is_some()
                || sc.serialize_settings().validators().requires_tagging())
                && sc.serialize_settings().pdf_version() >= PdfVersion::Pdf15))
            && sc.serialize_settings().enable_tagging
        {
            if sc.serialize_settings().pdf_version() >= PdfVersion::Pdf15 {
                page.tab_order(TabOrder::StructureOrder);
            } else {
                sc.register_validation_error(ValidationError::RequiresNewerPdfVersion(
                    VersionedFeature::StructureOrderTabbing,
                    sc.location,
                ));
            }
        }

        page.parent(sc.page_tree_ref());
        page.contents(self.stream_ref);

        if !annotation_refs.is_empty() {
            page.annotations(annotation_refs.iter().map(|(r, _)| *r));
        }

        // Populate the refs for each annotation in page infos.
        let PageInfo::Krilla { annotations, .. } = &mut sc.page_infos_mut()[self.page_index] else {
            unreachable!()
        };
        *annotations = annotation_refs;

        page.finish();
        chunk_container.streams.pages.push(self.stream_chunk);

        Ok(())
    }
}

/// A page label.
#[derive(Debug, Hash, Eq, PartialEq, Default, Clone)]
pub struct PageLabel {
    /// The numbering style of the page label.
    pub(crate) style: Option<NumberingStyle>,
    /// The prefix of the page label.
    pub(crate) prefix: Option<String>,
    /// The numeric value of the page label.
    pub(crate) offset: Option<NonZeroU32>,
}

impl PageLabel {
    /// Create a new page label.
    pub fn new(
        style: Option<NumberingStyle>,
        prefix: Option<String>,
        offset: Option<NonZeroU32>,
    ) -> Self {
        Self {
            style,
            prefix,
            offset,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.style.is_none() && self.prefix.is_none() && self.offset.is_none()
    }

    pub(crate) fn serialize(&self, chunk_container: &mut ChunkContainer, root_ref: Ref) {
        let chunk = &mut chunk_container.non_stream.page_labels;
        let mut label = chunk
            .indirect(root_ref)
            .start::<pdf_writer::writers::PageLabel>();

        if let Some(style) = self.style {
            label.style(style.to_pdf());
        }

        if let Some(prefix) = &self.prefix {
            label.prefix(TextStr(prefix));
        }

        if let Some(offset) = self.offset.and_then(|o| i32::try_from(o.get()).ok()) {
            label.offset(offset);
        }

        label.finish();
    }
}

#[derive(Hash)]
pub(crate) struct PageLabelContainer<'a> {
    labels: &'a [PageLabel],
}

impl<'a> PageLabelContainer<'a> {
    pub(crate) fn new(labels: &'a [PageLabel]) -> Option<Self> {
        if labels.iter().all(|f| f.is_empty()) {
            None
        } else {
            Some(PageLabelContainer { labels })
        }
    }

    pub(crate) fn serialize(
        &self,
        sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    ) {
        // Will always contain at least one entry, since we ensured that a PageLabelContainer cannot
        // be empty
        let mut filtered_entries = vec![];
        let mut prev: Option<PageLabel> = None;

        for (i, label) in self.labels.iter().enumerate() {
            if let Some(n_prev) = &prev {
                if n_prev.style != label.style
                    || n_prev.prefix != label.prefix
                    || n_prev.offset.map(|n| n.get()) != label.offset.map(|n| n.get() + 1)
                {
                    filtered_entries.push((i, label.clone()));
                    prev = Some(label.clone());
                }
            } else {
                filtered_entries.push((i, label.clone()));
                prev = Some(label.clone());
            }
        }

        let mut chunk = sc.new_chunk();
        let mut num_tree = chunk.indirect(root_ref).start::<NumberTree<Ref>>();
        let mut nums = num_tree.nums();

        for (page_num, label) in filtered_entries {
            let label_ref = sc.register_page_label(chunk_container, label);
            nums.insert(page_num as i32, label_ref);
        }

        nums.finish();
        num_tree.finish();
        chunk_container.non_stream.page_label_tree = Some((root_ref, chunk));
    }
}
