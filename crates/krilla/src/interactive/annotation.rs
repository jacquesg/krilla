//! PDF annotations, allowing you to add extra "content" to specific pages.
//!
//! PDF has the concept of annotations, which allow you to associate certain regions of
//! a page with an "annotation". krilla currently supports four families of annotations:
//!
//! - [`LinkAnnotation`]: hyperlinks targeting destinations or actions.
//! - [`TextAnnotation`]: sticky-note style comments (ISO 32000-2 §12.5.6.4).
//! - [`MarkupAnnotation`]: highlight / underline / strike-out / squiggly markup
//!   over a region of page content (ISO 32000-2 §12.5.6.10).
//! - [`WidgetAnnotation`]: AcroForm widget annotations for interactive form
//!   fields — text inputs, buttons (checkbox / radio / pushbutton) and choice
//!   fields (combo / list) per ISO 32000-2 §12.7.
//!
//! Additional annotation subtypes can be added on demand.

use core::f32;

use pdf_writer::types::{AnnotationFlags, FieldFlags};
use pdf_writer::{Chunk, Finish, Name, Ref, Str, TextStr};

use crate::chunk_container::ChunkContainer;
use crate::color::Color;
use crate::configure::{PdfVersion, ValidationError};
use crate::error::KrillaResult;
use crate::geom::{Quadrilateral, Rect};
use crate::interactive::action::Action;
use crate::interactive::destination::Destination;
use crate::page::page_root_transform;
use crate::serialize::SerializeContext;
use crate::surface::Location;

/// A single Form XObject the widget appearance pipeline emits as an
/// indirect object alongside the annotation dict. The widget's `/AP`
/// references one or two of these — single-state widgets use one;
/// checkbox / radio widgets use two (`/Yes` + `/Off`).
pub(crate) struct AppearanceStream {
    pub(crate) xobject_ref: Ref,
    pub(crate) bbox_w: f32,
    pub(crate) bbox_h: f32,
    pub(crate) content: Vec<u8>,
    /// Whether the content stream references the document-level
    /// Helvetica resource via `/Helv`. When `false` the XObject is
    /// emitted with an empty resource dict (used for vector-only
    /// strokes such as the radio dot or checkbox tick path).
    pub(crate) uses_helvetica: bool,
}

/// The set of Form XObjects produced by widget appearance generation
/// for one annotation. Consumed by [`Annotation::serialize`] after the
/// annotation dict has been finalised — every stream becomes one
/// indirect object in the same chunk as the annotation.
pub(crate) struct AppearanceJob {
    /// Document-level Helvetica font ref. Cached on the job so
    /// emission does not have to round-trip through
    /// [`SerializeContext`] a second time.
    pub(crate) helv_ref: Ref,
    /// "On" state stream (single-state widgets use this slot too).
    pub(crate) on: AppearanceStream,
    /// "Off" state stream — `Some` only for checkbox / radio.
    pub(crate) off: Option<AppearanceStream>,
}

/// An annotation.
pub struct Annotation {
    pub(crate) annotation_type: AnnotationType,
    pub(crate) alt: Option<String>,
    pub(crate) struct_parent: Option<i32>,
    pub(crate) location: Option<Location>,
}

impl Annotation {
    /// Create a new link annotation with some alt text.
    ///
    /// Note that the alt text might be required in some cases, for example
    /// when exporting to PDF/UA.
    pub fn new_link(annotation: LinkAnnotation, alt_text: Option<String>) -> Self {
        Self {
            annotation_type: AnnotationType::Link(annotation),
            alt: alt_text,
            struct_parent: None,
            location: None,
        }
    }

    /// Create a new text (sticky-note) annotation with some alt text.
    ///
    /// The alt text may be required by certain export profiles (e.g.
    /// PDF/UA). See [`TextAnnotation`] for the available fields.
    pub fn new_text(annotation: TextAnnotation, alt_text: Option<String>) -> Self {
        Self {
            annotation_type: AnnotationType::Text(annotation),
            alt: alt_text,
            struct_parent: None,
            location: None,
        }
    }

    /// Create a new markup annotation (highlight / underline / strike-out /
    /// squiggly) with some alt text.
    ///
    /// The alt text may be required by certain export profiles (e.g.
    /// PDF/UA). See [`MarkupAnnotation`] for the available fields.
    pub fn new_markup(annotation: MarkupAnnotation, alt_text: Option<String>) -> Self {
        Self {
            annotation_type: AnnotationType::Markup(annotation),
            alt: alt_text,
            struct_parent: None,
            location: None,
        }
    }

    /// Create a new AcroForm widget annotation.
    ///
    /// The widget is added to the page like any other annotation; its
    /// indirect reference is additionally registered with the document
    /// catalogue's `/AcroForm /Fields` array so PDF viewers expose it
    /// as a fillable form field.
    pub fn new_widget(annotation: WidgetAnnotation, alt_text: Option<String>) -> Self {
        Self {
            annotation_type: AnnotationType::Widget(annotation),
            alt: alt_text,
            struct_parent: None,
            location: None,
        }
    }

    /// Sets the location of the annotation.
    pub fn with_location(mut self, location: Option<Location>) -> Self {
        self.location = location;
        self
    }
}

impl From<LinkAnnotation> for Annotation {
    fn from(value: LinkAnnotation) -> Self {
        Self {
            annotation_type: AnnotationType::Link(value),
            alt: None,
            struct_parent: None,
            location: None,
        }
    }
}

impl From<TextAnnotation> for Annotation {
    fn from(value: TextAnnotation) -> Self {
        Self {
            annotation_type: AnnotationType::Text(value),
            alt: None,
            struct_parent: None,
            location: None,
        }
    }
}

impl From<MarkupAnnotation> for Annotation {
    fn from(value: MarkupAnnotation) -> Self {
        Self {
            annotation_type: AnnotationType::Markup(value),
            alt: None,
            struct_parent: None,
            location: None,
        }
    }
}

impl From<WidgetAnnotation> for Annotation {
    fn from(value: WidgetAnnotation) -> Self {
        Self {
            annotation_type: AnnotationType::Widget(value),
            alt: None,
            struct_parent: None,
            location: None,
        }
    }
}

impl Annotation {
    pub(crate) fn serialize(
        &self,
        sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
        page_height: f32,
    ) -> KrillaResult<()> {
        let chunk = &mut chunk_container.non_stream.annotations;
        let mut annotation = chunk
            .indirect(root_ref)
            .start::<pdf_writer::writers::Annotation>();

        let appearance_job = self
            .annotation_type
            .serialize_type(sc, &mut annotation, page_height)?;

        // Link annotations only set the /F PRINT flag when they have a visible
        // border (so borderless links don't print). Text, Markup and Widget
        // annotations are visible page artefacts and should always print —
        // except for `WidgetAnnotation` opted-in via `with_hidden(true)`,
        // which switches to `/F HIDDEN` so the field carries state without
        // a visible appearance (HTML `<input type="hidden">`).
        if let AnnotationType::Link(l) = &self.annotation_type {
            // TODO: No need to write the print flag even if it is `None`,
            // only for PDF/A.
            if l.border.is_none()
                || sc
                    .serialize_settings()
                    .configuration
                    .validators()
                    .requires_annotation_flags()
            {
                annotation.flags(AnnotationFlags::PRINT);
            }
        } else if let AnnotationType::Widget(w) = &self.annotation_type {
            annotation.flags(if w.hidden {
                AnnotationFlags::HIDDEN
            } else {
                AnnotationFlags::PRINT
            });
        } else {
            annotation.flags(AnnotationFlags::PRINT);
        }

        if let Some(struct_parent) = self.struct_parent {
            annotation.struct_parent(struct_parent);
        }

        // Widget annotations carry their value via /V (and friends) — the
        // /Contents key is not meaningful and the alt-text validator does
        // not apply (form fields are exposed via their /T partial name and
        // /TU alternate name, not /Contents).
        let is_widget = matches!(self.annotation_type, AnnotationType::Widget(_));
        if !is_widget {
            if let Some(alt_text) = &self.alt {
                annotation.contents(TextStr(alt_text));
            }
        }

        if !is_widget && self.alt.as_ref().is_none_or(String::is_empty) {
            sc.register_validation_error(ValidationError::MissingAnnotationAltText(self.location));
        }

        annotation.finish();

        // AcroForm catalogue wiring (ISO 32000-2 §12.7.3): every widget
        // annotation's indirect reference participates in the catalogue's
        // `/AcroForm /Fields` array. We register the ref *after* the
        // annotation chunk has been emitted; `ChunkContainer::finish`
        // remaps the ref through the same remapper used for every other
        // indirect object and writes the array entry.
        if is_widget {
            sc.register_widget_field(root_ref);
        }

        // Emit any Form XObjects produced by widget appearance generation
        // into a dedicated chunk in `chunk_container.x_objects`. The
        // widget's `/AP /N` indirect refs were allocated before the
        // annotation dict was written, so the cross-reference between
        // the annotation and the XObjects is already in place.
        if let Some(job) = appearance_job {
            let mut xchunk = Chunk::new();
            emit_appearance_xobjects(&mut xchunk, job);
            chunk_container.streams.x_objects.push(xchunk);
        }

        Ok(())
    }
}

/// Emit one or two Form XObject indirect objects into `chunk` —
/// referenced from the widget annotation's `/AP` entry. Each XObject
/// carries a widget-local `/BBox` (`[0 0 w h]`) and a `/Resources`
/// dict containing `/Font /Helv <helv_ref>` when the content stream
/// references Helvetica.
fn emit_appearance_xobjects(chunk: &mut Chunk, job: AppearanceJob) {
    write_form_xobject(chunk, &job.on, job.helv_ref);
    if let Some(off) = job.off {
        write_form_xobject(chunk, &off, job.helv_ref);
    }
}

fn write_form_xobject(chunk: &mut Chunk, stream: &AppearanceStream, helv_ref: Ref) {
    let mut xobj = chunk.form_xobject(stream.xobject_ref, &stream.content);
    xobj.bbox(pdf_writer::Rect::new(
        0.0,
        0.0,
        stream.bbox_w,
        stream.bbox_h,
    ));
    {
        let mut resources = xobj.resources();
        if stream.uses_helvetica {
            let mut fonts = resources.fonts();
            fonts.pair(Name(b"Helv"), helv_ref);
            fonts.finish();
        }
        resources.finish();
    }
    xobj.finish();
}

/// A type of annotation.
pub enum AnnotationType {
    /// A link annotation.
    Link(LinkAnnotation),
    /// A text (sticky-note) annotation.
    Text(TextAnnotation),
    /// A markup annotation (highlight, underline, strike-out, squiggly).
    Markup(MarkupAnnotation),
    /// A widget annotation (AcroForm interactive form field).
    Widget(WidgetAnnotation),
}

impl AnnotationType {
    fn serialize_type(
        &self,
        sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
    ) -> KrillaResult<Option<AppearanceJob>> {
        match self {
            AnnotationType::Link(l) => l.serialize_type(sc, annotation, page_height),
            AnnotationType::Text(t) => t.serialize_type(sc, annotation, page_height),
            AnnotationType::Markup(m) => m.serialize_type(sc, annotation, page_height),
            AnnotationType::Widget(w) => w.serialize_type(sc, annotation, page_height),
        }
    }
}

/// An annotation target.
pub enum Target {
    /// A destination within the document.
    Destination(Destination),
    /// An action to be performed.
    Action(Action),
}

/// Border of a link annotation.
pub struct LinkBorder {
    pub(crate) width: f32,
    pub(crate) color: Color,
}

impl LinkBorder {
    /// Create a new link annotation border.
    ///
    /// `width`: The width of the border in pt.
    /// `color`: The color of the border.
    pub fn new(width: f32, color: Color) -> Self {
        Self { width, color }
    }
}

/// A link annotation.
pub struct LinkAnnotation {
    pub(crate) rect: Rect,
    pub(crate) quad_points: Option<Vec<Quadrilateral>>,
    pub(crate) target: Target,
    pub(crate) border: Option<LinkBorder>,
}

impl LinkAnnotation {
    /// Create a new link annotation.
    ///
    /// `rect`: The bounding box of the link annotation that it should cover on the page.
    /// `target`: The target of the link annotation.
    pub fn new(rect: Rect, target: Target) -> Self {
        Self {
            rect,
            quad_points: None,
            target,
            border: None,
        }
    }

    /// Create a new link annotation.
    ///
    /// `target`: The target of the link annotation.
    /// `quad_points`: An array of quadrilaterals that define where the link
    /// annotation should be activated. This is useful if you for example have
    /// a link annotation that is broken to one or multiple lines.
    pub fn new_with_quad_points(quad_points: Vec<Quadrilateral>, target: Target) -> Self {
        assert!(!quad_points.is_empty());

        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for point in quad_points.iter().flat_map(|q| q.0) {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }

        // Expand the bounding box by a little. There is a bug in adobe acrobat
        // that sometimes prevents the quadpoints from being used if the quad
        // points lie exactly on the bounding rectangle.
        const EPSILON: f32 = 0.001;
        let rect = Rect::from_ltrb(
            min_x - EPSILON,
            min_y - EPSILON,
            max_x + EPSILON,
            max_y + EPSILON,
        )
        .unwrap();

        Self {
            rect,
            quad_points: Some(quad_points),
            target,
            border: None,
        }
    }

    /// Set a border for this link annotation. The border will be visible on
    /// screen but not when printed, unless when exporting with PDF/A standard.
    pub fn with_border(self, border: LinkBorder) -> Self {
        Self {
            border: Some(border),
            ..self
        }
    }

    fn serialize_type(
        &self,
        sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
    ) -> KrillaResult<Option<AppearanceJob>> {
        annotation.subtype(pdf_writer::types::AnnotationType::Link);

        let actual_rect = self
            .rect
            .transform(page_root_transform(page_height))
            .unwrap();
        annotation.rect(actual_rect.to_pdf_rect());
        annotation.border(
            0.0,
            0.0,
            self.border.as_ref().map_or(0.0, |x| x.width),
            None,
        );

        if let Some(border) = &self.border {
            write_color(annotation, &border.color);
        }

        if sc.serialize_settings().pdf_version() >= PdfVersion::Pdf16 {
            self.quad_points.as_ref().map(|p| {
                annotation.quad_points(p.iter().flat_map(|q| q.0).flat_map(|p| {
                    let mut p = p.to_tsp();
                    page_root_transform(page_height).to_tsp().map_point(&mut p);
                    [p.x, p.y]
                }))
            });
        }

        match &self.target {
            Target::Destination(destination) => {
                destination.serialize(sc, annotation.insert(Name(b"Dest")))?
            }
            Target::Action(action) => action.serialize(sc, annotation.action())?,
        };

        Ok(None)
    }
}

/// Icon glyph for a [`TextAnnotation`] (`/Name` entry, ISO 32000-2
/// §12.5.6.4 Table 172). Viewers display the corresponding pre-defined
/// glyph for the sticky note.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum TextAnnotationIcon {
    /// Sticky note. Default in most viewers.
    #[default]
    Note,
    /// Speech bubble.
    Comment,
    /// A key.
    Key,
    /// A question mark.
    Help,
    /// A "new paragraph" mark.
    NewParagraph,
    /// A paragraph mark.
    Paragraph,
    /// An insertion-point caret.
    Insert,
}

impl TextAnnotationIcon {
    fn to_pdf(self) -> pdf_writer::types::AnnotationIcon<'static> {
        use pdf_writer::types::AnnotationIcon;
        match self {
            TextAnnotationIcon::Note => AnnotationIcon::Note,
            TextAnnotationIcon::Comment => AnnotationIcon::Comment,
            TextAnnotationIcon::Key => AnnotationIcon::Key,
            TextAnnotationIcon::Help => AnnotationIcon::Help,
            TextAnnotationIcon::NewParagraph => AnnotationIcon::NewParagraph,
            TextAnnotationIcon::Paragraph => AnnotationIcon::Paragraph,
            TextAnnotationIcon::Insert => AnnotationIcon::Insert,
        }
    }
}

/// A text (sticky-note) annotation per ISO 32000-2 §12.5.6.4.
///
/// Text annotations display a small icon on the page; activating it
/// (e.g. clicking in a viewer) opens a pop-up with the annotation's
/// `contents` and `title`.
///
/// Build with [`TextAnnotation::new`] and the chainable setter methods;
/// wrap into an [`Annotation`] via [`Annotation::new_text`] or
/// [`From<TextAnnotation>`].
pub struct TextAnnotation {
    pub(crate) rect: Rect,
    pub(crate) contents: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) color: Option<Color>,
    pub(crate) icon: TextAnnotationIcon,
    pub(crate) open: bool,
    pub(crate) creation_date: Option<String>,
    pub(crate) modification_date: Option<String>,
}

impl TextAnnotation {
    /// Create a new text annotation with the given bounding rectangle.
    ///
    /// `rect` is in user-space (page) coordinates; krilla applies the
    /// same page-root transform that link annotations use.
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            contents: None,
            title: None,
            color: None,
            icon: TextAnnotationIcon::default(),
            open: false,
            creation_date: None,
            modification_date: None,
        }
    }

    /// Set the `/Contents` text — the body of the pop-up.
    pub fn with_contents(mut self, contents: impl Into<String>) -> Self {
        self.contents = Some(contents.into());
        self
    }

    /// Set the `/T` text — the title bar of the pop-up. Typically the
    /// author's name.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the `/C` colour — the colour of the icon background and
    /// pop-up title bar.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the icon glyph used to display the annotation (`/Name`).
    pub fn with_icon(mut self, icon: TextAnnotationIcon) -> Self {
        self.icon = icon;
        self
    }

    /// Set the `/Open` flag — whether the pop-up should be displayed
    /// initially when the page is opened.
    pub fn with_open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Set the `/CreationDate` entry — the date the annotation was
    /// created, formatted as a PDF date string per ISO 32000-2 §7.9.4
    /// (e.g. `D:20260515120000Z`). The caller is responsible for
    /// constructing a syntactically valid date string; krilla emits
    /// the value verbatim as a literal string.
    pub fn with_creation_date(mut self, date: impl Into<String>) -> Self {
        self.creation_date = Some(date.into());
        self
    }

    /// Set the `/M` entry — the date the annotation was last modified,
    /// formatted as a PDF date string per ISO 32000-2 §7.9.4. The
    /// caller is responsible for constructing a syntactically valid
    /// date string; krilla emits the value verbatim as a literal
    /// string.
    pub fn with_modification_date(mut self, date: impl Into<String>) -> Self {
        self.modification_date = Some(date.into());
        self
    }

    fn serialize_type(
        &self,
        _sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
    ) -> KrillaResult<Option<AppearanceJob>> {
        annotation.subtype(pdf_writer::types::AnnotationType::Text);

        let actual_rect = self
            .rect
            .transform(page_root_transform(page_height))
            .unwrap();
        annotation.rect(actual_rect.to_pdf_rect());
        annotation.icon(self.icon.to_pdf());
        annotation.pair(Name(b"Open"), self.open);

        if let Some(title) = &self.title {
            annotation.author(TextStr(title));
        }

        if let Some(contents) = &self.contents {
            annotation.contents(TextStr(contents));
        }

        if let Some(color) = &self.color {
            write_color(annotation, color);
        }

        write_annotation_dates(
            annotation,
            self.creation_date.as_deref(),
            self.modification_date.as_deref(),
        );

        Ok(None)
    }
}

/// Subtype for a [`MarkupAnnotation`] (ISO 32000-2 §12.5.6.10).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MarkupSubtype {
    /// `/Highlight` — highlights the marked region with a translucent
    /// colour overlay.
    Highlight,
    /// `/Underline` — draws an underline beneath the marked region.
    Underline,
    /// `/StrikeOut` — draws a strike-through line over the marked
    /// region.
    Strikeout,
    /// `/Squiggly` — draws a squiggly underline beneath the marked
    /// region (typically used for spell-check style emphasis).
    Squiggly,
}

impl MarkupSubtype {
    fn to_pdf(self) -> pdf_writer::types::AnnotationType {
        use pdf_writer::types::AnnotationType as PdfAnno;
        match self {
            MarkupSubtype::Highlight => PdfAnno::Highlight,
            MarkupSubtype::Underline => PdfAnno::Underline,
            MarkupSubtype::Strikeout => PdfAnno::StrikeOut,
            MarkupSubtype::Squiggly => PdfAnno::Squiggly,
        }
    }
}

/// A text-markup annotation per ISO 32000-2 §12.5.6.10.
///
/// Markup annotations decorate a region of page content — typically
/// runs of text — with a highlight overlay, underline, strike-through
/// or squiggly underline. The decorated region is specified by one or
/// more [`Quadrilateral`]s in user-space coordinates, with one quad
/// per text fragment (e.g. one per visual line for a multi-line
/// selection).
///
/// `/QuadPoints` is a PDF 1.6+ feature for non-link annotations;
/// krilla emits the array only when the configured PDF version is
/// 1.6 or higher. On earlier versions, only the `/Rect` is written
/// and viewers render the markup over the bounding box, which is the
/// upstream-defined fallback.
pub struct MarkupAnnotation {
    pub(crate) rect: Rect,
    pub(crate) subtype: MarkupSubtype,
    pub(crate) quad_points: Vec<Quadrilateral>,
    pub(crate) contents: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) color: Option<Color>,
    pub(crate) creation_date: Option<String>,
    pub(crate) modification_date: Option<String>,
}

impl MarkupAnnotation {
    /// Create a new markup annotation.
    ///
    /// `subtype` selects the visual treatment (highlight / underline /
    /// strike-out / squiggly). `quad_points` must contain at least one
    /// [`Quadrilateral`] describing the region to decorate, in
    /// user-space (page) coordinates. The annotation's `/Rect` is
    /// computed as the bounding box of the supplied quad points.
    ///
    /// # Panics
    ///
    /// Panics if `quad_points` is empty — markup annotations require
    /// a non-empty `/QuadPoints` array per ISO 32000-2 §12.5.6.10.
    pub fn new(subtype: MarkupSubtype, quad_points: Vec<Quadrilateral>) -> Self {
        assert!(
            !quad_points.is_empty(),
            "markup annotations require a non-empty quad_points array"
        );

        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for point in quad_points.iter().flat_map(|q| q.0) {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }

        // Same epsilon expansion as `LinkAnnotation::new_with_quad_points`:
        // Acrobat occasionally drops the QuadPoints overlay when they
        // sit exactly on the bounding rectangle edges.
        const EPSILON: f32 = 0.001;
        let rect = Rect::from_ltrb(
            min_x - EPSILON,
            min_y - EPSILON,
            max_x + EPSILON,
            max_y + EPSILON,
        )
        .unwrap();

        Self {
            rect,
            subtype,
            quad_points,
            contents: None,
            title: None,
            color: None,
            creation_date: None,
            modification_date: None,
        }
    }

    /// Set the `/Contents` text — the body of the markup pop-up.
    pub fn with_contents(mut self, contents: impl Into<String>) -> Self {
        self.contents = Some(contents.into());
        self
    }

    /// Set the `/T` text — the title bar of the markup pop-up.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the `/C` colour — used for the highlight / underline /
    /// strike-out / squiggly stroke.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the `/CreationDate` entry — the date the annotation was
    /// created, formatted as a PDF date string per ISO 32000-2 §7.9.4
    /// (e.g. `D:20260515120000Z`). The caller is responsible for
    /// constructing a syntactically valid date string; krilla emits
    /// the value verbatim as a literal string.
    pub fn with_creation_date(mut self, date: impl Into<String>) -> Self {
        self.creation_date = Some(date.into());
        self
    }

    /// Set the `/M` entry — the date the annotation was last modified,
    /// formatted as a PDF date string per ISO 32000-2 §7.9.4. The
    /// caller is responsible for constructing a syntactically valid
    /// date string; krilla emits the value verbatim as a literal
    /// string.
    pub fn with_modification_date(mut self, date: impl Into<String>) -> Self {
        self.modification_date = Some(date.into());
        self
    }

    fn serialize_type(
        &self,
        sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
    ) -> KrillaResult<Option<AppearanceJob>> {
        annotation.subtype(self.subtype.to_pdf());

        let actual_rect = self
            .rect
            .transform(page_root_transform(page_height))
            .unwrap();
        annotation.rect(actual_rect.to_pdf_rect());

        if sc.serialize_settings().pdf_version() >= PdfVersion::Pdf16 {
            annotation.quad_points(self.quad_points.iter().flat_map(|q| q.0).flat_map(|p| {
                let mut p = p.to_tsp();
                page_root_transform(page_height).to_tsp().map_point(&mut p);
                [p.x, p.y]
            }));
        }

        if let Some(title) = &self.title {
            annotation.author(TextStr(title));
        }

        if let Some(contents) = &self.contents {
            annotation.contents(TextStr(contents));
        }

        if let Some(color) = &self.color {
            write_color(annotation, color);
        }

        write_annotation_dates(
            annotation,
            self.creation_date.as_deref(),
            self.modification_date.as_deref(),
        );

        Ok(None)
    }
}

/// Sub-kind for a [`ButtonField`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ButtonKind {
    /// A check box. State stored in `/V` as `/Yes` or `/Off`.
    Checkbox,
    /// A radio button. The radio flag (bit 16) is set on `/Ff`.
    Radio,
    /// A pushbutton — non-stateful, used as a UI affordance. The
    /// pushbutton flag (bit 17) is set on `/Ff`.
    PushButton,
}

/// An AcroForm text field (`/FT /Tx`) — single-line or multi-line text
/// input. The multi-line flag (bit 13) is set via [`TextFieldFlags`].
pub struct TextField {
    /// Current value of the field. Becomes `/V`.
    pub value: String,
    /// Default value used by `/DV`.
    pub default_value: String,
    /// `/MaxLen` (max character length); `None` for no limit.
    pub max_length: Option<u16>,
    /// Per-field flag bits — see [`TextFieldFlags`].
    pub flags: TextFieldFlags,
}

/// An AcroForm button field (`/FT /Btn`) — checkbox, radio button or
/// pushbutton, distinguished by [`ButtonKind`].
pub struct ButtonField {
    /// Whether the button is checked. Ignored for pushbuttons.
    pub checked: bool,
    /// Sub-kind. Sets the appropriate `/Ff` bit (radio / pushbutton).
    pub kind: ButtonKind,
    /// Pushbutton caption (written into `/MK /CA`); empty for the other
    /// kinds.
    pub caption: String,
    /// Per-field flag bits — see [`ButtonFieldFlags`].
    pub flags: ButtonFieldFlags,
}

/// An AcroForm choice field (`/FT /Ch`) — combo box or list box. Set
/// the combo flag via [`ChoiceFieldFlags::with_combo`] to make it a
/// dropdown.
///
/// `values` and `default_values` are vectors of export-value strings.
/// Their cardinality drives `/V` and `/DV` emission per ISO 32000-2
/// §12.7.4.4:
///
/// - empty vector — entry omitted entirely
/// - one entry — `/V (literal)` (single string)
/// - two or more entries — `/V [(a)(b)(c)]` (array of strings); only
///   valid when the multi-select flag (bit 22) is set on
///   [`ChoiceFieldFlags`].
///
/// Single-select callers pass a one-element vector; multi-select
/// (`<select multiple>`) callers pass one entry per selected option.
pub struct ChoiceField {
    /// Currently selected export values (`/V`).
    pub values: Vec<String>,
    /// Default export values (`/DV`).
    pub default_values: Vec<String>,
    /// `(export-value, display-name)` pairs written into `/Opt`.
    pub options: Vec<(String, String)>,
    /// Per-field flag bits — see [`ChoiceFieldFlags`].
    pub flags: ChoiceFieldFlags,
}

/// The field-type-specific payload of a [`WidgetAnnotation`].
pub enum WidgetField {
    /// `/Tx` text field.
    Text(TextField),
    /// `/Btn` button field (checkbox, radio, pushbutton).
    Button(ButtonField),
    /// `/Ch` choice field (combo / list box).
    Choice(ChoiceField),
}

/// Flag bits for an AcroForm text field (`/FT /Tx`).
///
/// The general flags `READ_ONLY` and `REQUIRED` are shared with the
/// other field types; the multi-line / password / file-select / comb
/// bits are text-specific. See ISO 32000-2 §12.7.4.3, Table 230.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct TextFieldFlags {
    /// Set bit 1 (`/Ff` 1): the field is read-only.
    pub read_only: bool,
    /// Set bit 13 (4096): multi-line text input.
    pub multiline: bool,
    /// Set bit 14 (8192): password — characters not echoed.
    pub password: bool,
}

impl TextFieldFlags {
    /// Set the read-only flag (bit 1).
    pub fn with_read_only(mut self, value: bool) -> Self {
        self.read_only = value;
        self
    }

    /// Set the multi-line flag (bit 13).
    pub fn with_multiline(mut self, value: bool) -> Self {
        self.multiline = value;
        self
    }

    /// Set the password flag (bit 14).
    pub fn with_password(mut self, value: bool) -> Self {
        self.password = value;
        self
    }

    fn to_bits(self) -> u32 {
        let mut flags = FieldFlags::empty();
        if self.read_only {
            flags |= FieldFlags::READ_ONLY;
        }
        if self.multiline {
            flags |= FieldFlags::MULTILINE;
        }
        if self.password {
            flags |= FieldFlags::PASSWORD;
        }
        flags.bits()
    }
}

/// Flag bits for an AcroForm button field (`/FT /Btn`).
///
/// The button-specific bits (radio / pushbutton / radios-in-unison)
/// configure the sub-kind. They are mutually exclusive at the spec
/// level: a checkbox sets neither, a radio sets `radio` (bit 16) and
/// optionally `radios_in_unison` (bit 26), a pushbutton sets
/// `pushbutton` (bit 17). See ISO 32000-2 §12.7.4.2, Table 229.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ButtonFieldFlags {
    /// Set bit 1 (`/Ff` 1): the field is read-only.
    pub read_only: bool,
    /// Set bit 16 (32768): the field is a radio button group.
    pub radio: bool,
    /// Set bit 17 (65536): the field is a pushbutton.
    pub pushbutton: bool,
    /// Set bit 26 (33554432): grouped radios with the same `/V` toggle
    /// in unison.
    pub radios_in_unison: bool,
}

impl ButtonFieldFlags {
    /// Set the read-only flag (bit 1).
    pub fn with_read_only(mut self, value: bool) -> Self {
        self.read_only = value;
        self
    }

    /// Set the radio flag (bit 16).
    pub fn with_radio(mut self, value: bool) -> Self {
        self.radio = value;
        self
    }

    /// Set the pushbutton flag (bit 17).
    pub fn with_pushbutton(mut self, value: bool) -> Self {
        self.pushbutton = value;
        self
    }

    /// Set the radios-in-unison flag (bit 26).
    pub fn with_radios_in_unison(mut self, value: bool) -> Self {
        self.radios_in_unison = value;
        self
    }

    fn to_bits(self) -> u32 {
        let mut flags = FieldFlags::empty();
        if self.read_only {
            flags |= FieldFlags::READ_ONLY;
        }
        if self.radio {
            flags |= FieldFlags::RADIO;
        }
        if self.pushbutton {
            flags |= FieldFlags::PUSHBUTTON;
        }
        if self.radios_in_unison {
            flags |= FieldFlags::RADIOS_IN_UNISON;
        }
        flags.bits()
    }
}

/// Flag bits for an AcroForm choice field (`/FT /Ch`).
///
/// The combo flag distinguishes a drop-down (combo) from a list box.
/// `MULTI_SELECT` is permissible but moegoe currently emits only
/// single-select fields; the API is provided for completeness.
/// See ISO 32000-2 §12.7.4.4, Table 232.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ChoiceFieldFlags {
    /// Set bit 1 (`/Ff` 1): the field is read-only.
    pub read_only: bool,
    /// Set bit 18 (131072): combo box (drop-down) instead of list box.
    pub combo: bool,
    /// Set bit 22 (2097152): multi-select.
    pub multi_select: bool,
}

impl ChoiceFieldFlags {
    /// Set the read-only flag (bit 1).
    pub fn with_read_only(mut self, value: bool) -> Self {
        self.read_only = value;
        self
    }

    /// Set the combo flag (bit 18).
    pub fn with_combo(mut self, value: bool) -> Self {
        self.combo = value;
        self
    }

    /// Set the multi-select flag (bit 22).
    pub fn with_multi_select(mut self, value: bool) -> Self {
        self.multi_select = value;
        self
    }

    fn to_bits(self) -> u32 {
        let mut flags = FieldFlags::empty();
        if self.read_only {
            flags |= FieldFlags::READ_ONLY;
        }
        if self.combo {
            flags |= FieldFlags::COMBO;
        }
        if self.multi_select {
            flags |= FieldFlags::MULTI_SELECT;
        }
        flags.bits()
    }
}

/// An AcroForm widget annotation (ISO 32000-2 §12.5.6.19 / §12.7).
///
/// A widget annotation is both an annotation (rectangle on a page) and
/// a form field (entry in the catalogue's `/AcroForm /Fields` array).
/// krilla emits the merged form (annotation dictionary that also
/// carries `/FT`, `/T`, `/V`, etc.) — the merged form is what every
/// modern PDF viewer expects for terminal fields.
///
/// Every widget emits a `/AP /N` appearance stream — a Form XObject
/// (single state for text/choice/pushbutton; `/Yes`+`/Off` sub-states
/// for checkbox/radio) drawn in widget-local coordinates with `/BBox
/// [0 0 w h]`. Text and choice streams reference the document-level
/// Helvetica resource (allocated lazily via
/// [`SerializeContext::standard_helvetica_ref`]); checkbox / radio
/// streams use vector paths only. The catalogue still sets
/// `/NeedAppearances true` so Acrobat regenerates appearances from
/// `/V` + `/DA` on the first save when a non-ASCII value triggers
/// the placeholder substitution.
pub struct WidgetAnnotation {
    pub(crate) rect: Rect,
    pub(crate) partial_name: String,
    pub(crate) field: WidgetField,
    pub(crate) hidden: bool,
}

impl WidgetAnnotation {
    /// Create a new widget annotation.
    ///
    /// `rect` is in user-space (page) coordinates and identifies the
    /// region of the page on which the field appears. `partial_name`
    /// becomes the field's `/T` partial name — the value that PDF form
    /// processors and Acrobat surface in dropdowns, validation messages
    /// and field-name maps. `field` selects the field type and carries
    /// its type-specific payload (text / button / choice).
    pub fn new(rect: Rect, partial_name: impl Into<String>, field: WidgetField) -> Self {
        Self {
            rect,
            partial_name: partial_name.into(),
            field,
            hidden: false,
        }
    }

    /// Mark the widget as hidden — the annotation gets `/F 2`
    /// (`AnnotationFlags::HIDDEN`) so it neither displays nor prints
    /// nor responds to user interaction, but the field still
    /// participates in `/AcroForm /Fields` and form submission. Use
    /// for HTML `<input type="hidden">` and equivalent state-carrying
    /// fields.
    pub fn with_hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    fn serialize_type(
        &self,
        sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
    ) -> KrillaResult<Option<AppearanceJob>> {
        annotation.subtype(pdf_writer::types::AnnotationType::Widget);

        let actual_rect = self
            .rect
            .transform(page_root_transform(page_height))
            .unwrap();
        annotation.rect(actual_rect.to_pdf_rect());

        // /T is required on every terminal field; /FT identifies the
        // field type. The values pulled out of `WidgetField` populate
        // /V, /DV, /MaxLen, /Opt and /Ff.
        annotation.pair(Name(b"T"), TextStr(&self.partial_name));

        // /DA is mandatory on every variable-text field (and harmless
        // elsewhere). A minimal default appearance — Helvetica 10pt
        // black — matches what Acrobat falls back to when /DA is
        // missing but `/NeedAppearances true` is set at the catalogue.
        const DEFAULT_APPEARANCE: &[u8] = b"/Helv 10 Tf 0 g";

        let bbox_w = self.rect.width();
        let bbox_h = self.rect.height();

        // Per-widget content stream(s) for `/AP /N`. Pre-allocated here
        // so we can write the `/AP` reference into the annotation dict;
        // the actual Form XObject indirect objects are emitted by
        // `Annotation::serialize` after `annotation.finish()`.
        let helv_ref = sc.standard_helvetica_ref();
        let job: AppearanceJob;

        // Field-type names (`/Tx`, `/Btn`, `/Ch`) and checkbox/radio
        // state names (`/Yes`, `/Off`) are pinned by ISO 32000-2
        // §12.7.4 and emitted as PDF name objects. `pdf_writer`
        // re-exports the `FieldType` / `CheckBoxState` enums but
        // gates the `to_name()` mapping behind `pub(crate)`, so we
        // write the canonical bytes directly here.
        match &self.field {
            WidgetField::Text(text) => {
                annotation.pair(Name(b"FT"), Name(b"Tx"));
                annotation.pair(Name(b"Ff"), text.flags.to_bits() as i32);
                annotation.pair(Name(b"DA"), Str(DEFAULT_APPEARANCE));
                annotation.pair(Name(b"V"), TextStr(&text.value));
                annotation.pair(Name(b"DV"), TextStr(&text.default_value));
                if let Some(max_len) = text.max_length {
                    annotation.pair(Name(b"MaxLen"), i32::from(max_len));
                }
                let ap_ref = sc.new_ref();
                write_ap_single(annotation, ap_ref);
                job = AppearanceJob {
                    helv_ref,
                    on: AppearanceStream {
                        xobject_ref: ap_ref,
                        bbox_w,
                        bbox_h,
                        content: build_text_appearance_content(&text.value, bbox_h),
                        uses_helvetica: true,
                    },
                    off: None,
                };
            }
            WidgetField::Button(button) => {
                annotation.pair(Name(b"FT"), Name(b"Btn"));
                annotation.pair(Name(b"Ff"), button.flags.to_bits() as i32);
                match button.kind {
                    ButtonKind::Checkbox => {
                        let state: Name = if button.checked {
                            Name(b"Yes")
                        } else {
                            Name(b"Off")
                        };
                        annotation.pair(Name(b"V"), state);
                        annotation.pair(Name(b"DV"), state);
                        annotation.pair(Name(b"AS"), state);
                        let on_ref = sc.new_ref();
                        let off_ref = sc.new_ref();
                        write_ap_on_off(annotation, on_ref, off_ref);
                        job = AppearanceJob {
                            helv_ref,
                            on: AppearanceStream {
                                xobject_ref: on_ref,
                                bbox_w,
                                bbox_h,
                                content: build_checkbox_on_content(bbox_w, bbox_h),
                                uses_helvetica: false,
                            },
                            off: Some(AppearanceStream {
                                xobject_ref: off_ref,
                                bbox_w,
                                bbox_h,
                                content: build_empty_box_content(bbox_w, bbox_h),
                                uses_helvetica: false,
                            }),
                        };
                    }
                    ButtonKind::Radio => {
                        // Each radio widget in a group stores its export
                        // value in /AS; the group's /V selects which one
                        // is on. v1 of the integration emits a single
                        // widget per HTML `<input type="radio">` element
                        // — the convert layer aggregates radios by `name`
                        // into a field with shared `/T`. The export-value
                        // surfacing belongs to the embedder.
                        let state: Name = if button.checked {
                            Name(b"Yes")
                        } else {
                            Name(b"Off")
                        };
                        annotation.pair(Name(b"V"), state);
                        annotation.pair(Name(b"AS"), state);
                        let on_ref = sc.new_ref();
                        let off_ref = sc.new_ref();
                        write_ap_on_off(annotation, on_ref, off_ref);
                        job = AppearanceJob {
                            helv_ref,
                            on: AppearanceStream {
                                xobject_ref: on_ref,
                                bbox_w,
                                bbox_h,
                                content: build_radio_on_content(bbox_w, bbox_h),
                                uses_helvetica: false,
                            },
                            off: Some(AppearanceStream {
                                xobject_ref: off_ref,
                                bbox_w,
                                bbox_h,
                                content: build_radio_off_content(bbox_w, bbox_h),
                                uses_helvetica: false,
                            }),
                        };
                    }
                    ButtonKind::PushButton => {
                        // Pushbuttons have no persistent value. /MK /CA
                        // gives Acrobat a label to draw — written as a
                        // best-effort caption derived from the HTML
                        // element's text content.
                        if !button.caption.is_empty() {
                            let mut mk = annotation.insert(Name(b"MK")).dict();
                            mk.pair(Name(b"CA"), TextStr(&button.caption));
                            mk.finish();
                        }
                        let ap_ref = sc.new_ref();
                        write_ap_single(annotation, ap_ref);
                        job = AppearanceJob {
                            helv_ref,
                            on: AppearanceStream {
                                xobject_ref: ap_ref,
                                bbox_w,
                                bbox_h,
                                content: build_pushbutton_content(&button.caption, bbox_w, bbox_h),
                                uses_helvetica: true,
                            },
                            off: None,
                        };
                    }
                }
            }
            WidgetField::Choice(choice) => {
                annotation.pair(Name(b"FT"), Name(b"Ch"));
                annotation.pair(Name(b"Ff"), choice.flags.to_bits() as i32);
                annotation.pair(Name(b"DA"), Str(DEFAULT_APPEARANCE));
                // /V and /DV emission follows ISO 32000-2 §12.7.4.4:
                // empty → omit; one entry → string literal; two or more
                // → array of strings (MultiSelect, bit 22). Single-
                // select callers are expected to pass a one-element
                // vector; the array form is meaningful only when
                // `ChoiceFieldFlags::multi_select` is set.
                write_choice_value_entry(annotation, Name(b"V"), &choice.values);
                write_choice_value_entry(annotation, Name(b"DV"), &choice.default_values);
                let mut opt = annotation.insert(Name(b"Opt")).array();
                for (export, display) in &choice.options {
                    let mut entry = opt.push().array();
                    entry.item(TextStr(export));
                    entry.item(TextStr(display));
                    entry.finish();
                }
                opt.finish();
                let ap_ref = sc.new_ref();
                write_ap_single(annotation, ap_ref);
                // Find display string for the first /V (export value).
                // The appearance stream renders one selection only —
                // multi-select fields rely on `/NeedAppearances true`
                // for the viewer to render the full selection list.
                let first_export = choice.values.first().map(String::as_str).unwrap_or("");
                let display = choice
                    .options
                    .iter()
                    .find(|(export, _)| export == first_export)
                    .map(|(_, display)| display.as_str())
                    .unwrap_or(first_export);
                job = AppearanceJob {
                    helv_ref,
                    on: AppearanceStream {
                        xobject_ref: ap_ref,
                        bbox_w,
                        bbox_h,
                        content: build_text_appearance_content(display, bbox_h),
                        uses_helvetica: true,
                    },
                    off: None,
                };
            }
        }

        Ok(Some(job))
    }
}

/// Write a `/V` or `/DV` entry on a choice-field widget annotation.
///
/// Cardinality drives the PDF object kind per ISO 32000-2 §12.7.4.4:
/// an empty vector omits the entry, a single value emits a string
/// literal, and two or more values emit an array of strings (only
/// meaningful when the MultiSelect flag, bit 22, is set on the
/// field's `/Ff`).
fn write_choice_value_entry(
    annotation: &mut pdf_writer::writers::Annotation,
    key: Name<'static>,
    values: &[String],
) {
    match values {
        [] => {}
        [single] => {
            annotation.pair(key, TextStr(single));
        }
        many => {
            let mut array = annotation.insert(key).array();
            for value in many {
                array.item(TextStr(value));
            }
            array.finish();
        }
    }
}

/// Write `/AP << /N <ap_ref> >>` into a widget annotation dict.
fn write_ap_single(annotation: &mut pdf_writer::writers::Annotation, ap_ref: Ref) {
    let mut ap = annotation.insert(Name(b"AP")).dict();
    ap.pair(Name(b"N"), ap_ref);
    ap.finish();
}

/// Write `/AP << /N << /Yes <on_ref> /Off <off_ref> >> >>` into a
/// widget annotation dict (checkbox / radio).
fn write_ap_on_off(annotation: &mut pdf_writer::writers::Annotation, on_ref: Ref, off_ref: Ref) {
    let mut ap = annotation.insert(Name(b"AP")).dict();
    let mut n = ap.insert(Name(b"N")).dict();
    n.pair(Name(b"Yes"), on_ref);
    n.pair(Name(b"Off"), off_ref);
    n.finish();
    ap.finish();
}

/// Escape a UTF-8 string for inclusion in a PDF content-stream literal
/// `(...)`. Non-ASCII codepoints become `?` (Acrobat regenerates the
/// proper appearance from `/V` on first save via `/NeedAppearances`).
fn escape_pdf_string_ascii(value: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len() + 2);
    for ch in value.chars() {
        match ch {
            '(' | ')' | '\\' => {
                out.push(b'\\');
                out.push(ch as u8);
            }
            c if (c as u32) >= 0x20 && (c as u32) <= 0x7E => {
                out.push(c as u8);
            }
            _ => out.push(b'?'),
        }
    }
    out
}

/// Build a single-line text appearance: `BT /Helv 10 Tf 0 g 2 y Td (value) Tj ET`.
/// `bbox_h` chooses a baseline ~3pt below the vertical centre.
fn build_text_appearance_content(value: &str, bbox_h: f32) -> Vec<u8> {
    let baseline_y = ((bbox_h - 10.0) / 2.0).max(2.0);
    let mut out = Vec::with_capacity(value.len() + 48);
    out.extend_from_slice(b"BT\n/Helv 10 Tf\n0 g\n2 ");
    out.extend_from_slice(format!("{:.2}", baseline_y).as_bytes());
    out.extend_from_slice(b" Td\n(");
    out.extend_from_slice(&escape_pdf_string_ascii(value));
    out.extend_from_slice(b") Tj\nET\n");
    out
}

/// Build a checkbox "on" stream: 1pt black border + a stroked X.
fn build_checkbox_on_content(bbox_w: f32, bbox_h: f32) -> Vec<u8> {
    let inset = 1.0_f32;
    let w = (bbox_w - 2.0 * inset).max(0.0);
    let h = (bbox_h - 2.0 * inset).max(0.0);
    let pad = 2.0_f32;
    let x1 = inset + pad;
    let y1 = inset + pad;
    let x2 = inset + w - pad;
    let y2 = inset + h - pad;
    format!(
        "q\n0 0 0 RG\n0.5 w\n{inset} {inset} {w} {h} re\nS\n{x1:.2} {y1:.2} m\n{x2:.2} {y2:.2} l\n{x1:.2} {y2:.2} m\n{x2:.2} {y1:.2} l\nS\nQ\n"
    )
    .into_bytes()
}

/// Build a checkbox "off" stream: just the 1pt black border.
fn build_empty_box_content(bbox_w: f32, bbox_h: f32) -> Vec<u8> {
    let inset = 1.0_f32;
    let w = (bbox_w - 2.0 * inset).max(0.0);
    let h = (bbox_h - 2.0 * inset).max(0.0);
    format!("q\n0 0 0 RG\n0.5 w\n{inset} {inset} {w} {h} re\nS\nQ\n").into_bytes()
}

/// Build a radio "on" stream: a stroked circle plus a filled dot.
fn build_radio_on_content(bbox_w: f32, bbox_h: f32) -> Vec<u8> {
    let cx = bbox_w / 2.0;
    let cy = bbox_h / 2.0;
    let r_outer = (bbox_w.min(bbox_h) / 2.0 - 1.0).max(0.5);
    let r_inner = r_outer * 0.55;
    let mut out = String::new();
    out.push_str("q\n0 0 0 RG\n0 0 0 rg\n0.5 w\n");
    append_circle(&mut out, cx, cy, r_outer, "S");
    append_circle(&mut out, cx, cy, r_inner, "f");
    out.push_str("Q\n");
    out.into_bytes()
}

/// Build a radio "off" stream: just the stroked outer circle.
fn build_radio_off_content(bbox_w: f32, bbox_h: f32) -> Vec<u8> {
    let cx = bbox_w / 2.0;
    let cy = bbox_h / 2.0;
    let r_outer = (bbox_w.min(bbox_h) / 2.0 - 1.0).max(0.5);
    let mut out = String::new();
    out.push_str("q\n0 0 0 RG\n0.5 w\n");
    append_circle(&mut out, cx, cy, r_outer, "S");
    out.push_str("Q\n");
    out.into_bytes()
}

/// Build a pushbutton appearance: light grey fill, dark border, centred caption.
fn build_pushbutton_content(caption: &str, bbox_w: f32, bbox_h: f32) -> Vec<u8> {
    let baseline_y = ((bbox_h - 10.0) / 2.0).max(2.0);
    let approx_glyph_w = 5.5_f32;
    let text_x = ((bbox_w - approx_glyph_w * caption.len() as f32) / 2.0).max(2.0);
    let mut out = Vec::with_capacity(caption.len() + 96);
    out.extend_from_slice(b"q\n0.85 0.85 0.85 rg\n0 0 ");
    out.extend_from_slice(format!("{:.2}", bbox_w).as_bytes());
    out.push(b' ');
    out.extend_from_slice(format!("{:.2}", bbox_h).as_bytes());
    out.extend_from_slice(b" re\nf\n0 0 0 RG\n0.5 w\n0 0 ");
    out.extend_from_slice(format!("{:.2}", bbox_w).as_bytes());
    out.push(b' ');
    out.extend_from_slice(format!("{:.2}", bbox_h).as_bytes());
    out.extend_from_slice(b" re\nS\nBT\n/Helv 10 Tf\n0 g\n");
    out.extend_from_slice(format!("{:.2}", text_x).as_bytes());
    out.push(b' ');
    out.extend_from_slice(format!("{:.2}", baseline_y).as_bytes());
    out.extend_from_slice(b" Td\n(");
    out.extend_from_slice(&escape_pdf_string_ascii(caption));
    out.extend_from_slice(b") Tj\nET\nQ\n");
    out
}

/// Approximate a circle with four cubic Bezier segments and emit the
/// trailing operator (`S` for stroke, `f` for fill).
fn append_circle(out: &mut String, cx: f32, cy: f32, r: f32, op: &str) {
    let k = 0.5522847_f32 * r;
    use std::fmt::Write as _;
    writeln!(out, "{:.2} {:.2} m", cx + r, cy).unwrap();
    writeln!(
        out,
        "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
        cx + r,
        cy + k,
        cx + k,
        cy + r,
        cx,
        cy + r
    )
    .unwrap();
    writeln!(
        out,
        "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
        cx - k,
        cy + r,
        cx - r,
        cy + k,
        cx - r,
        cy
    )
    .unwrap();
    writeln!(
        out,
        "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
        cx - r,
        cy - k,
        cx - k,
        cy - r,
        cx,
        cy - r
    )
    .unwrap();
    writeln!(
        out,
        "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
        cx + k,
        cy - r,
        cx + r,
        cy - k,
        cx + r,
        cy
    )
    .unwrap();
    writeln!(out, "{}", op).unwrap();
}

/// Emit `/CreationDate` and `/M` entries on a Text or Markup
/// annotation. Both are PDF date strings (ISO 32000-2 §7.9.4) — the
/// caller supplies a pre-formatted literal (e.g. `D:20260515120000Z`)
/// and krilla writes it verbatim as a PDF string. Centralised so the
/// two markup annotation kinds share the same emission shape.
fn write_annotation_dates(
    annotation: &mut pdf_writer::writers::Annotation,
    creation_date: Option<&str>,
    modification_date: Option<&str>,
) {
    if let Some(date) = creation_date {
        annotation.pair(Name(b"CreationDate"), Str(date.as_bytes()));
    }
    if let Some(date) = modification_date {
        annotation.pair(Name(b"M"), Str(date.as_bytes()));
    }
}

/// Emit a `/C` colour entry on an annotation using the regular-colour
/// projection. Centralised so Link, Text and Markup share the same
/// device-space handling.
fn write_color(annotation: &mut pdf_writer::writers::Annotation, color: &Color) {
    match color.to_regular() {
        crate::color::RegularColor::Rgb(rgb) => {
            let [r, g, b] = rgb.to_pdf_color();
            annotation.color_rgb(r, g, b);
        }
        crate::color::RegularColor::Cmyk(cmyk) => {
            let [c, m, y, k] = cmyk.to_pdf_color();
            annotation.color_cmyk(c, m, y, k);
        }
        crate::color::RegularColor::Luma(gray) => {
            annotation.color_gray(gray.to_pdf_color());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::rgb;
    use crate::document::Document;
    use crate::geom::Point;
    use crate::page::PageSettings;

    fn finish_with(annotation: Annotation) -> Vec<u8> {
        let settings = crate::SerializeSettings {
            pretty: true,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);
        let mut page = document.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
        page.add_annotation(annotation);
        page.finish();
        document.finish().expect("document serialisation should succeed")
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn text_annotation_emits_subtype_and_open_flag() {
        let text = TextAnnotation::new(Rect::from_xywh(10.0, 20.0, 30.0, 40.0).unwrap())
            .with_contents("hello")
            .with_title("reviewer")
            .with_icon(TextAnnotationIcon::Comment)
            .with_open(true)
            .with_color(rgb::Color::new(255, 0, 0).into());

        let pdf = finish_with(Annotation::new_text(text, Some("note".into())));

        assert!(contains(&pdf, b"/Subtype /Text"), "missing /Subtype /Text");
        assert!(contains(&pdf, b"/Name /Comment"), "missing /Name /Comment");
        assert!(contains(&pdf, b"/Open true"), "missing /Open true");
        assert!(contains(&pdf, b"(reviewer)"), "missing /T author");
        assert!(contains(&pdf, b"/F 4"), "missing /F PRINT flag");
    }

    #[test]
    fn text_annotation_default_icon_is_note() {
        let text = TextAnnotation::new(Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap());
        let pdf = finish_with(Annotation::new_text(text, Some("alt".into())));
        assert!(contains(&pdf, b"/Subtype /Text"));
        assert!(contains(&pdf, b"/Name /Note"));
        assert!(contains(&pdf, b"/Open false"));
    }

    #[test]
    fn markup_annotation_highlight_emits_subtype_and_quadpoints() {
        let quad = Quadrilateral([
            Point::from_xy(10.0, 50.0),
            Point::from_xy(60.0, 50.0),
            Point::from_xy(60.0, 40.0),
            Point::from_xy(10.0, 40.0),
        ]);
        let markup = MarkupAnnotation::new(MarkupSubtype::Highlight, vec![quad])
            .with_contents("note")
            .with_title("a")
            .with_color(rgb::Color::new(255, 255, 0).into());

        let pdf = finish_with(Annotation::new_markup(markup, Some("highlight".into())));

        assert!(contains(&pdf, b"/Subtype /Highlight"));
        assert!(contains(&pdf, b"/QuadPoints"));
        assert!(contains(&pdf, b"/F 4"));
    }

    #[test]
    fn markup_annotation_each_subtype_emits_distinct_subtype_name() {
        let make = |subtype| {
            let quad = Quadrilateral([
                Point::from_xy(0.0, 10.0),
                Point::from_xy(20.0, 10.0),
                Point::from_xy(20.0, 0.0),
                Point::from_xy(0.0, 0.0),
            ]);
            let markup = MarkupAnnotation::new(subtype, vec![quad]);
            finish_with(Annotation::new_markup(markup, Some("alt".into())))
        };

        assert!(contains(&make(MarkupSubtype::Highlight), b"/Subtype /Highlight"));
        assert!(contains(&make(MarkupSubtype::Underline), b"/Subtype /Underline"));
        assert!(contains(&make(MarkupSubtype::Strikeout), b"/Subtype /StrikeOut"));
        assert!(contains(&make(MarkupSubtype::Squiggly), b"/Subtype /Squiggly"));
    }

    #[test]
    #[should_panic(expected = "markup annotations require a non-empty quad_points array")]
    fn markup_annotation_empty_quad_points_panics() {
        let _ = MarkupAnnotation::new(MarkupSubtype::Highlight, Vec::new());
    }

    #[test]
    fn text_annotation_emits_creation_and_modification_dates() {
        let text = TextAnnotation::new(Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap())
            .with_creation_date("D:20260515120000Z")
            .with_modification_date("D:20260516153000Z")
            .with_icon(TextAnnotationIcon::Help);

        let pdf = finish_with(Annotation::new_text(text, Some("dated".into())));

        // Per the task spec, /Name is the required Text-subtype entry
        // when an icon is set.
        assert!(contains(&pdf, b"/Subtype /Text"), "missing /Subtype /Text");
        assert!(contains(&pdf, b"/Name /Help"), "missing /Name /Help");
        assert!(
            contains(&pdf, b"/CreationDate (D:20260515120000Z)"),
            "missing /CreationDate"
        );
        assert!(
            contains(&pdf, b"/M (D:20260516153000Z)"),
            "missing /M modification date"
        );
    }

    #[test]
    fn markup_annotation_each_subtype_emits_quadpoints() {
        // ISO 32000-2 §12.5.6.10: every markup subtype
        // (Highlight/Underline/Squiggly/StrikeOut) requires /QuadPoints.
        let make = |subtype| {
            let quad = Quadrilateral([
                Point::from_xy(0.0, 10.0),
                Point::from_xy(20.0, 10.0),
                Point::from_xy(20.0, 0.0),
                Point::from_xy(0.0, 0.0),
            ]);
            let markup = MarkupAnnotation::new(subtype, vec![quad])
                .with_creation_date("D:20260515120000Z");
            finish_with(Annotation::new_markup(markup, Some("alt".into())))
        };

        for subtype in [
            MarkupSubtype::Highlight,
            MarkupSubtype::Underline,
            MarkupSubtype::Strikeout,
            MarkupSubtype::Squiggly,
        ] {
            let pdf = make(subtype);
            assert!(
                contains(&pdf, b"/QuadPoints"),
                "missing /QuadPoints for {subtype:?}"
            );
            assert!(
                contains(&pdf, b"/CreationDate (D:20260515120000Z)"),
                "missing /CreationDate for {subtype:?}"
            );
        }
    }

    #[test]
    fn text_annotation_from_trait_wraps_without_alt() {
        let text = TextAnnotation::new(Rect::from_xywh(0.0, 0.0, 5.0, 5.0).unwrap());
        let annotation: Annotation = text.into();
        assert!(matches!(annotation.annotation_type, AnnotationType::Text(_)));
        assert!(annotation.alt.is_none());
    }

    #[test]
    fn markup_annotation_from_trait_wraps_without_alt() {
        let quad = Quadrilateral([
            Point::from_xy(0.0, 0.0),
            Point::from_xy(1.0, 0.0),
            Point::from_xy(1.0, 1.0),
            Point::from_xy(0.0, 1.0),
        ]);
        let markup = MarkupAnnotation::new(MarkupSubtype::Underline, vec![quad]);
        let annotation: Annotation = markup.into();
        assert!(matches!(annotation.annotation_type, AnnotationType::Markup(_)));
        assert!(annotation.alt.is_none());
    }

    fn widget_rect() -> Rect {
        Rect::from_xywh(20.0, 30.0, 100.0, 18.0).unwrap()
    }

    #[test]
    fn widget_annotation_text_emits_subtype_field_type_and_value() {
        let text = WidgetField::Text(TextField {
            value: "alice".into(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "username", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(contains(&pdf, b"/Subtype /Widget"), "missing /Subtype /Widget");
        assert!(contains(&pdf, b"/FT /Tx"), "missing /FT /Tx");
        assert!(contains(&pdf, b"/T (username)"), "missing partial name /T");
        assert!(contains(&pdf, b"/V (alice)"), "missing field value /V");
        assert!(contains(&pdf, b"/AcroForm"), "missing /AcroForm");
        assert!(
            contains(&pdf, b"/NeedAppearances true"),
            "missing /NeedAppearances true"
        );
    }

    #[test]
    fn widget_annotation_text_multiline_sets_flag_bit_13() {
        let text = WidgetField::Text(TextField {
            value: "hello\nworld".into(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default().with_multiline(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "bio", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Multiline = bit 13 = 4096
        assert!(contains(&pdf, b"/Ff 4096"), "missing multiline /Ff bit");
    }

    #[test]
    fn widget_annotation_text_password_sets_flag_bit_14() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: Some(64),
            flags: TextFieldFlags::default().with_password(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "pw", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Password = bit 14 = 8192
        assert!(contains(&pdf, b"/Ff 8192"), "missing password /Ff bit");
        assert!(contains(&pdf, b"/MaxLen 64"), "missing /MaxLen");
    }

    #[test]
    fn widget_annotation_checkbox_checked_emits_yes_state() {
        let button = WidgetField::Button(ButtonField {
            checked: true,
            kind: ButtonKind::Checkbox,
            caption: String::new(),
            flags: ButtonFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "opt-in", button);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(contains(&pdf, b"/FT /Btn"), "missing /FT /Btn");
        assert!(contains(&pdf, b"/AS /Yes"), "missing /AS /Yes");
        assert!(contains(&pdf, b"/V /Yes"), "missing /V /Yes");
    }

    #[test]
    fn widget_annotation_checkbox_unchecked_emits_off_state() {
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::Checkbox,
            caption: String::new(),
            flags: ButtonFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "opt-in", button);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(contains(&pdf, b"/AS /Off"));
        assert!(contains(&pdf, b"/V /Off"));
    }

    #[test]
    fn widget_annotation_radio_sets_flag_bit_16() {
        let button = WidgetField::Button(ButtonField {
            checked: true,
            kind: ButtonKind::Radio,
            caption: String::new(),
            flags: ButtonFieldFlags::default().with_radio(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "size", button);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Radio = bit 16 = 32768
        assert!(contains(&pdf, b"/Ff 32768"), "missing radio /Ff bit");
    }

    #[test]
    fn widget_annotation_pushbutton_sets_flag_bit_17_and_caption() {
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::PushButton,
            caption: "Submit".into(),
            flags: ButtonFieldFlags::default().with_pushbutton(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "go", button);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Pushbutton = bit 17 = 65536
        assert!(contains(&pdf, b"/Ff 65536"), "missing pushbutton /Ff bit");
        assert!(contains(&pdf, b"(Submit)"), "missing pushbutton caption");
    }

    #[test]
    fn widget_annotation_choice_combo_sets_flag_bit_18() {
        let choice = WidgetField::Choice(ChoiceField {
            values: vec!["US".into()],
            default_values: vec!["US".into()],
            options: vec![
                ("US".into(), "United States".into()),
                ("CA".into(), "Canada".into()),
            ],
            flags: ChoiceFieldFlags::default().with_combo(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "country", choice);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(contains(&pdf, b"/FT /Ch"), "missing /FT /Ch");
        // Combo = bit 18 = 131072
        assert!(contains(&pdf, b"/Ff 131072"), "missing combo /Ff bit");
        assert!(contains(&pdf, b"/Opt"), "missing /Opt array");
        assert!(contains(&pdf, b"(US)"), "missing US export");
        // Single value still emits /V as a string literal, not an array.
        assert!(contains(&pdf, b"/V (US)"), "missing single-value /V");
        assert!(!contains(&pdf, b"/V ["), "single value must not emit /V array");
    }

    #[test]
    fn widget_annotation_choice_multi_select_emits_v_array() {
        // MultiSelect flag (bit 22) plus three selected export values
        // round-trip into a `/V [(red)(green)(blue)]` array per
        // ISO 32000-2 §12.7.4.4.
        let choice = WidgetField::Choice(ChoiceField {
            values: vec!["red".into(), "green".into(), "blue".into()],
            default_values: vec!["red".into()],
            options: vec![
                ("red".into(), "Red".into()),
                ("green".into(), "Green".into()),
                ("blue".into(), "Blue".into()),
                ("yellow".into(), "Yellow".into()),
            ],
            flags: ChoiceFieldFlags::default().with_multi_select(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "colours", choice);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(contains(&pdf, b"/FT /Ch"), "missing /FT /Ch");
        // MultiSelect = bit 22 = 2097152
        assert!(contains(&pdf, b"/Ff 2097152"), "missing multi-select /Ff bit");
        // /V is an array, not a string literal — the byte sequence is
        // `/V [(red)(green)(blue)]` (pdf-writer inserts no separator
        // between adjacent string literals).
        assert!(contains(&pdf, b"/V ["), "missing /V array opener");
        assert!(contains(&pdf, b"(red)"), "missing red value");
        assert!(contains(&pdf, b"(green)"), "missing green value");
        assert!(contains(&pdf, b"(blue)"), "missing blue value");
        // Default values still emit as a single literal.
        assert!(contains(&pdf, b"/DV (red)"), "missing /DV single literal");
    }

    #[test]
    fn widget_annotation_choice_empty_values_omits_v() {
        // An empty `values` vector omits `/V` entirely — viewers fall
        // back to `/DV` or the first option.
        let choice = WidgetField::Choice(ChoiceField {
            values: Vec::new(),
            default_values: Vec::new(),
            options: vec![("A".into(), "Apple".into())],
            flags: ChoiceFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "fruit", choice);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(contains(&pdf, b"/FT /Ch"), "missing /FT /Ch");
        // Neither /V nor /DV should appear at all.
        assert!(!contains(&pdf, b"/V ("), "unexpected /V literal");
        assert!(!contains(&pdf, b"/V ["), "unexpected /V array");
        assert!(!contains(&pdf, b"/DV ("), "unexpected /DV literal");
        assert!(!contains(&pdf, b"/DV ["), "unexpected /DV array");
    }

    #[test]
    fn widget_annotation_read_only_sets_flag_bit_1() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default().with_read_only(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "ro", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Read-only = bit 1 = 1
        assert!(contains(&pdf, b"/Ff 1"), "missing read-only /Ff bit");
    }

    #[test]
    fn widget_annotation_from_trait_wraps_without_alt() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "f", text);
        let annotation: Annotation = widget.into();
        assert!(matches!(annotation.annotation_type, AnnotationType::Widget(_)));
        assert!(annotation.alt.is_none());
    }

    #[test]
    fn document_with_no_widgets_does_not_emit_acroform() {
        let mut document = Document::new();
        let page = document.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
        page.finish();
        let pdf = document.finish().expect("document serialisation should succeed");
        assert!(!contains(&pdf, b"/AcroForm"));
    }
}
