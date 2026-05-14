//! PDF annotations, allowing you to add extra "content" to specific pages.
//!
//! PDF has the concept of annotations, which allow you to associate certain regions of
//! a page with an "annotation". krilla currently supports three families of annotations:
//!
//! - [`LinkAnnotation`]: hyperlinks targeting destinations or actions.
//! - [`TextAnnotation`]: sticky-note style comments (ISO 32000-2 §12.5.6.4).
//! - [`MarkupAnnotation`]: highlight / underline / strike-out / squiggly markup
//!   over a region of page content (ISO 32000-2 §12.5.6.10).
//!
//! Additional annotation subtypes can be added on demand.

use core::f32;

use pdf_writer::types::AnnotationFlags;
use pdf_writer::{Chunk, Finish, Name, Ref, TextStr};

use crate::color::Color;
use crate::configure::{PdfVersion, ValidationError};
use crate::error::KrillaResult;
use crate::geom::{Quadrilateral, Rect};
use crate::interactive::action::Action;
use crate::interactive::destination::Destination;
use crate::page::page_root_transform;
use crate::serialize::SerializeContext;
use crate::surface::Location;

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

impl Annotation {
    pub(crate) fn serialize(
        &self,
        sc: &mut SerializeContext,
        root_ref: Ref,
        page_height: f32,
    ) -> KrillaResult<Chunk> {
        // PDF/X-1a: only TrapNet and PrinterMark annotations are allowed.
        // krilla does not emit either of those, so every supported annotation
        // type triggers the same forbids_annotations validator error here.
        if sc.serialize_settings().validator().forbids_annotations() {
            sc.register_validation_error(ValidationError::ContainsAnnotation(self.location));
        }

        let mut chunk = Chunk::new();
        let mut annotation = chunk
            .indirect(root_ref)
            .start::<pdf_writer::writers::Annotation>();

        self.annotation_type
            .serialize_type(sc, &mut annotation, page_height)?;

        // Link annotations only set the /F PRINT flag when they have a visible
        // border (so borderless links don't print). Text and Markup
        // annotations are visible page artefacts and should always print.
        if let AnnotationType::Link(l) = &self.annotation_type {
            // TODO: No need to write the print flag even if it is `None`,
            // only for PDF/A.
            if l.border.is_none()
                || sc
                    .serialize_settings()
                    .configuration
                    .validator()
                    .requires_annotation_flags()
            {
                annotation.flags(AnnotationFlags::PRINT);
            }
        } else {
            annotation.flags(AnnotationFlags::PRINT);
        }

        if let Some(struct_parent) = self.struct_parent {
            annotation.struct_parent(struct_parent);
        }

        if let Some(alt_text) = &self.alt {
            annotation.contents(TextStr(alt_text));
        }

        if self.alt.as_ref().is_none_or(String::is_empty) {
            sc.register_validation_error(ValidationError::MissingAnnotationAltText(self.location));
        }

        annotation.finish();

        Ok(chunk)
    }
}

/// A type of annotation.
pub enum AnnotationType {
    /// A link annotation.
    Link(LinkAnnotation),
    /// A text (sticky-note) annotation.
    Text(TextAnnotation),
    /// A markup annotation (highlight, underline, strike-out, squiggly).
    Markup(MarkupAnnotation),
}

impl AnnotationType {
    fn serialize_type(
        &self,
        sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
    ) -> KrillaResult<()> {
        match self {
            AnnotationType::Link(l) => l.serialize_type(sc, annotation, page_height),
            AnnotationType::Text(t) => t.serialize_type(sc, annotation, page_height),
            AnnotationType::Markup(m) => m.serialize_type(sc, annotation, page_height),
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
    ) -> KrillaResult<()> {
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

        Ok(())
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

    fn serialize_type(
        &self,
        _sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
    ) -> KrillaResult<()> {
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

        Ok(())
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

    fn serialize_type(
        &self,
        sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
    ) -> KrillaResult<()> {
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

        Ok(())
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
        let mut document = Document::new();
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
}
