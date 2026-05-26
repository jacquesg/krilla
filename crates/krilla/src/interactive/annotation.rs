//! PDF annotations, allowing you to add extra "content" to specific pages.
//!
//! PDF has the concept of annotations, which allow you to associate certain regions of
//! a page with an "annotation". krilla currently supports four families of annotations:
//!
//! - [`LinkAnnotation`]: hyperlinks targeting destinations or actions.
//! - [`TextAnnotation`]: sticky-note style comments (ISO 32000-2 §12.5.6.4).
//! - [`MarkupAnnotation`]: highlight / underline / strike-out / squiggly markup
//!   over a region of page content (ISO 32000-2 §12.5.6.10).
//! - [`FileAttachmentAnnotation`]: per-page file-attachment annotations
//!   (ISO 32000-2 §12.5.6.15) that pin an [`EmbeddedFile`] to a page
//!   rectangle and display one of the predefined `/Name` icons.
//! - [`WidgetAnnotation`]: AcroForm widget annotations for interactive form
//!   fields — text inputs, buttons (checkbox / radio / pushbutton) and choice
//!   fields (combo / list) per ISO 32000-2 §12.7.
//!
//! Additional annotation subtypes can be added on demand.

use core::f32;

use pdf_writer::types::{AnnotationFlags, FieldFlags};
use pdf_writer::{Chunk, Finish, Name, Ref, Str, TextStr};

use crate::chunk_container::ChunkContainer;
use crate::color::{Color, RegularColor};
use crate::configure::{PdfVersion, ValidationError};
use crate::error::KrillaResult;
use crate::geom::{Quadrilateral, Rect};
use crate::interactive::action::{Action, JavaScriptAction};
use crate::interactive::destination::Destination;
use crate::interchange::embed::EmbeddedFile;
use crate::page::page_root_transform;
use crate::serialize::SerializeContext;
use crate::surface::Location;

/// `/MK /R` rotation entry for an AcroForm widget annotation per ISO
/// 32000-2 §12.5.6.19 Table 167.
///
/// Encodes the integer the spec admits (a multiple of 90 in
/// `0..360`). `None` rotation is the default and the absent state on
/// the wire — when the embedder selects `None`, krilla omits `/R`
/// from the `/MK` dictionary so the viewer applies its default
/// orientation. The three rotated states map to the explicit integer
/// the spec carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rotation {
    /// 90° counter-clockwise.
    Quarter,
    /// 180° (upside-down).
    Half,
    /// 270° counter-clockwise (equivalently 90° clockwise).
    ThreeQuarter,
}

impl Rotation {
    /// Return the integer encoding the PDF `/MK /R` entry uses
    /// (90 / 180 / 270 — multiples of 90 per ISO 32000-2 §12.5.6.19).
    #[inline]
    pub fn degrees(self) -> i32 {
        match self {
            Rotation::Quarter => 90,
            Rotation::Half => 180,
            Rotation::ThreeQuarter => 270,
        }
    }
}

/// `/MK /IF /SW` — when the icon scales relative to the widget rect
/// (ISO 32000-2 §12.5.6.19 Table 189). Together with [`ScaleType`]
/// the keyword determines the conditional logic the viewer uses
/// before applying the scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScaleWhen {
    /// `/SW /A` — always scale the icon (default).
    #[default]
    Always,
    /// `/SW /B` — scale only when the icon is larger than the
    /// annotation rectangle.
    ContentBiggerThanRect,
    /// `/SW /S` — scale only when the icon is smaller than the
    /// annotation rectangle.
    ContentSmallerThanRect,
    /// `/SW /N` — never scale the icon.
    Never,
}

impl ScaleWhen {
    /// The single-byte name the PDF `/IF /SW` entry uses (ISO
    /// 32000-2 §12.5.6.19 Table 189).
    #[inline]
    pub fn to_pdf_name(self) -> &'static [u8] {
        match self {
            ScaleWhen::Always => b"A",
            ScaleWhen::ContentBiggerThanRect => b"B",
            ScaleWhen::ContentSmallerThanRect => b"S",
            ScaleWhen::Never => b"N",
        }
    }
}

/// `/MK /IF /S` — how the icon scales when [`ScaleWhen`] admits
/// scaling (ISO 32000-2 §12.5.6.19 Table 188).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScaleType {
    /// `/S /A` — anamorphic scaling: fill the annotation rectangle
    /// without preserving the icon's aspect ratio.
    #[default]
    Anamorphic,
    /// `/S /P` — proportional scaling: preserve the icon's aspect
    /// ratio and centre it inside the annotation rectangle (the
    /// `/A` alignment array refines the placement).
    Proportional,
}

impl ScaleType {
    /// The single-byte name the PDF `/IF /S` entry uses (ISO
    /// 32000-2 §12.5.6.19 Table 188).
    #[inline]
    pub fn to_pdf_name(self) -> &'static [u8] {
        match self {
            ScaleType::Anamorphic => b"A",
            ScaleType::Proportional => b"P",
        }
    }
}

/// `/MK /IF` — icon fit dictionary per ISO 32000-2 §12.5.6.19
/// Table 187. Controls how a pushbutton widget's icon image
/// (`/I`, `/RI`, `/IX`) is scaled and positioned inside the
/// annotation rectangle.
///
/// `scale_when` selects the predicate the viewer evaluates;
/// `scale_type` selects how the icon is scaled when the predicate
/// admits scaling; `align_x` / `align_y` are the alignment
/// percentages (`[0.0, 1.0]`) that position the icon inside the
/// rectangle when proportional scaling leaves slack on one axis;
/// `fit_bounds` requests that proportional scaling first shrink the
/// icon to fit inside the annotation rectangle's drawing area
/// (`/FB true`) — only meaningful for proportional scaling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconFit {
    /// `/SW` — when to scale.
    pub scale_when: ScaleWhen,
    /// `/S` — how to scale.
    pub scale_type: ScaleType,
    /// `/A[0]` — horizontal alignment in `[0.0, 1.0]`. `0.5` centres
    /// the icon; `0.0` left-aligns; `1.0` right-aligns.
    pub align_x: f32,
    /// `/A[1]` — vertical alignment in `[0.0, 1.0]`. `0.5` centres
    /// the icon; `0.0` bottom-aligns; `1.0` top-aligns.
    pub align_y: f32,
    /// `/FB` — if `true`, viewer shrinks the icon (after
    /// proportional scaling) so the entire scaled icon fits inside
    /// the annotation's drawing area.
    pub fit_bounds: bool,
}

impl Default for IconFit {
    fn default() -> Self {
        // Spec-default `/A` is `[0.5 0.5]` per ISO 32000-2 §12.5.6.19
        // Table 187. The default `/SW`/`/S` combination (`/A`/`/A`)
        // matches the Acrobat default — always scale anamorphically.
        Self {
            scale_when: ScaleWhen::Always,
            scale_type: ScaleType::Anamorphic,
            align_x: 0.5,
            align_y: 0.5,
            fit_bounds: false,
        }
    }
}

/// `/MK /TP` — text position relative to the icon for a pushbutton
/// widget (ISO 32000-2 §12.5.6.19 Table 192). The default
/// [`TextPosition::CaptionOnly`] matches the spec default when `/TP`
/// is absent — the caption fills the button and the icon (if any)
/// is suppressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextPosition {
    /// `/TP 0` — caption only; icon (if any) is suppressed.
    #[default]
    CaptionOnly,
    /// `/TP 1` — icon only; caption (if any) is suppressed.
    IconOnly,
    /// `/TP 2` — caption below the icon.
    CaptionBelowIcon,
    /// `/TP 3` — caption above the icon.
    CaptionAboveIcon,
    /// `/TP 4` — caption to the right of the icon.
    CaptionRightOfIcon,
    /// `/TP 5` — caption to the left of the icon.
    CaptionLeftOfIcon,
    /// `/TP 6` — caption overlaid (centred on) the icon.
    CaptionOverlaidOnIcon,
}

impl TextPosition {
    /// The integer the PDF `/TP` entry carries (ISO 32000-2
    /// §12.5.6.19 Table 192).
    #[inline]
    pub fn to_pdf_integer(self) -> i32 {
        match self {
            TextPosition::CaptionOnly => 0,
            TextPosition::IconOnly => 1,
            TextPosition::CaptionBelowIcon => 2,
            TextPosition::CaptionAboveIcon => 3,
            TextPosition::CaptionRightOfIcon => 4,
            TextPosition::CaptionLeftOfIcon => 5,
            TextPosition::CaptionOverlaidOnIcon => 6,
        }
    }
}

/// `/MK` (Appearance Characteristics) dictionary entries for an
/// AcroForm widget annotation per ISO 32000-2 §12.5.6.19 Table 167
/// and §12.7.4.3.
///
/// The `/MK` dictionary supplements the field's appearance with the
/// optional widget-level decorations the spec defines — border /
/// background colours, rotation, the pushbutton caption family
/// (down / rollover), the rollover / alternate icon entries
/// (`/RI`, `/IX`), the icon fit dictionary (`/IF`), and the text
/// position keyword (`/TP`) — in addition to the icon (`/I`) and
/// caption (`/CA`) entries the existing krilla surface already
/// populates.
///
/// `border_colour` and `background_colour` accept any
/// [`Color::Regular`] value the embedder can construct; the
/// serialiser projects the colour onto the matching `/BC` / `/BG`
/// array layout (length 1 for DeviceGray, 3 for DeviceRGB, 4 for
/// DeviceCMYK) per Table 167. Wide-gamut ICC paints written into
/// `/MK` collapse to a three-component DeviceRGB array because
/// `/MK` colours are device-space only per the same table; the
/// embedder is expected to author device-space colours when this
/// matters for round-trip preservation.
///
/// `rollover_caption`, `down_caption`, `rollover_icon`,
/// `alternate_icon`, `icon_fit`, and `text_position` are only
/// meaningful for pushbutton widgets (ISO 32000-2 §12.5.6.19 Table
/// 167: `/RC`, `/AC`, `/RI`, `/IX`, `/IF`, `/TP` apply only to
/// widget annotations with field `/FT /Btn` and the pushbutton flag
/// set). krilla writes them on any widget the
/// `AppearanceCharacteristics` is attached to; viewers that read
/// `/MK` on a non-button widget silently ignore the surplus
/// entries.
#[derive(Debug, Clone, Default)]
pub struct AppearanceCharacteristics {
    /// `/BC` — border colour. `None` omits the entry; the viewer
    /// then applies its default (typically transparent / no
    /// border).
    pub border_colour: Option<Color>,
    /// `/BG` — background colour. `None` omits the entry; the
    /// viewer applies no background fill.
    pub background_colour: Option<Color>,
    /// `/R` — widget rotation per [`Rotation`]. `None` omits the
    /// entry and the viewer applies the default orientation.
    pub rotation: Option<Rotation>,
    /// `/RC` — rollover caption (pushbutton only); displayed when
    /// the pointer hovers over the widget. `None` omits the entry.
    pub rollover_caption: Option<String>,
    /// `/AC` — alternate (down) caption (pushbutton only);
    /// displayed while the user is clicking the widget. `None`
    /// omits the entry.
    pub down_caption: Option<String>,
    /// `/RI` — rollover icon (pushbutton only); the image the
    /// viewer displays when the pointer is over the widget but the
    /// button is not depressed. `None` omits the entry; only
    /// available when the `raster-images` feature is enabled.
    #[cfg(feature = "raster-images")]
    pub rollover_icon: Option<crate::graphics::image::Image>,
    /// `/IX` — alternate (down) icon (pushbutton only); the image
    /// the viewer displays while the button is depressed. `None`
    /// omits the entry; only available when the `raster-images`
    /// feature is enabled.
    #[cfg(feature = "raster-images")]
    pub alternate_icon: Option<crate::graphics::image::Image>,
    /// `/IF` — icon fit dictionary (pushbutton only); controls how
    /// the icon images (`/I`, `/RI`, `/IX`) are scaled and
    /// positioned inside the annotation rectangle. `None` omits the
    /// entry; the viewer then falls back to its anamorphic-scaling
    /// default.
    pub icon_fit: Option<IconFit>,
    /// `/TP` — text position keyword (pushbutton only); the spec
    /// admits the absent case to mean [`TextPosition::CaptionOnly`].
    /// `None` omits the entry; the viewer falls back to its
    /// caption-only default.
    pub text_position: Option<TextPosition>,
}

impl AppearanceCharacteristics {
    /// Whether every slot is `None` — used internally to skip the
    /// `/MK` extension entirely when the embedder constructed but
    /// never populated the structure.
    fn is_empty(&self) -> bool {
        #[cfg(feature = "raster-images")]
        let icons_empty = self.rollover_icon.is_none() && self.alternate_icon.is_none();
        #[cfg(not(feature = "raster-images"))]
        let icons_empty = true;
        self.border_colour.is_none()
            && self.background_colour.is_none()
            && self.rotation.is_none()
            && self.rollover_caption.is_none()
            && self.down_caption.is_none()
            && icons_empty
            && self.icon_fit.is_none()
            && self.text_position.is_none()
    }
}

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

/// A pushbutton icon-appearance Form XObject — ISO 32000-2 §12.5.6.19
/// Table 167 `/MK /I`. Wraps a single registered [`crate::graphics::
/// image::Image`] (via its already-resolved indirect ref) inside a
/// widget-local Form XObject whose content stream stretches the image
/// to fill `[0 0 bbox_w bbox_h]`. The XObject's indirect reference
/// becomes the `/MK /I` entry on the widget annotation; viewers draw
/// the icon inside the widget's rectangle.
pub(crate) struct IconAppearanceXObject {
    pub(crate) xobject_ref: Ref,
    pub(crate) image_ref: Ref,
    pub(crate) bbox_w: f32,
    pub(crate) bbox_h: f32,
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
    /// Pushbutton icon-appearance Form XObjects. The `normal` slot
    /// (`/MK /I`) is populated when the widget was decorated via
    /// [`WidgetAnnotation::with_icon_appearance`]; the `rollover`
    /// (`/MK /RI`) and `alternate` (`/MK /IX`) slots are populated
    /// when the embedder authored
    /// [`AppearanceCharacteristics::rollover_icon`] or
    /// [`AppearanceCharacteristics::alternate_icon`].
    pub(crate) icons: WidgetIconXObjects,
}

/// Pre-resolved indirect references to the embedded `Image` objects
/// backing a widget annotation's `/MK /I`, `/MK /RI`, and `/MK /IX`
/// entries. Resolved in [`Annotation::serialize`] before the annotation
/// dict starts writing because `register_image` needs mutable access
/// to the [`ChunkContainer`].
#[derive(Default, Clone, Copy)]
pub(crate) struct WidgetIconRefs {
    /// `/MK /I` — normal-state icon image ref.
    pub(crate) normal: Option<Ref>,
    /// `/MK /RI` — rollover-state icon image ref.
    pub(crate) rollover: Option<Ref>,
    /// `/MK /IX` — alternate (down-state) icon image ref.
    pub(crate) alternate: Option<Ref>,
}

impl WidgetIconRefs {
    /// Whether every icon slot is `None` — used to short-circuit the
    /// pushbutton arm's per-state Form XObject allocation when the
    /// widget carries no icon.
    fn is_empty(&self) -> bool {
        self.normal.is_none() && self.rollover.is_none() && self.alternate.is_none()
    }
}

/// The set of icon Form XObjects emitted by the pushbutton path —
/// one per populated entry in [`WidgetIconRefs`]. Each Form XObject
/// wraps the registered image and is referenced from the widget
/// annotation's `/MK` dictionary at the corresponding key.
#[derive(Default)]
pub(crate) struct WidgetIconXObjects {
    /// `/MK /I` icon Form XObject.
    pub(crate) normal: Option<IconAppearanceXObject>,
    /// `/MK /RI` icon Form XObject.
    pub(crate) rollover: Option<IconAppearanceXObject>,
    /// `/MK /IX` icon Form XObject.
    pub(crate) alternate: Option<IconAppearanceXObject>,
}

/// An annotation.
pub struct Annotation {
    pub(crate) annotation_type: AnnotationType,
    pub(crate) alt: Option<String>,
    /// `/StructParent` key (PDF 1.5+, ISO 32000-2 §14.7.4.4 / ISO
    /// 14289-1 §7.18.1). Populated automatically by
    /// [`crate::page::Page::add_tagged_annotation`] when the annotation
    /// participates in the structure tree; callers using the untagged
    /// [`crate::page::Page::add_annotation`] entry point may set this
    /// directly via [`Annotation::with_struct_parent`] when they have
    /// allocated the struct-parent slot themselves. The field is also
    /// exposed for `pub(crate)` mutation from the structure-tree
    /// builder, which back-fills the value when the tag tree resolves
    /// the annotation's `/StructElem` parent. Keeping the slot public
    /// for read access lets embedders implementing their own structure
    /// trees verify the wired value.
    pub struct_parent: Option<i32>,
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

    /// Create a new file-attachment annotation per ISO 32000-2
    /// §12.5.6.15.
    ///
    /// The annotation pins an [`EmbeddedFile`] to a page rectangle and
    /// displays one of the predefined `/Name` icons
    /// ([`FileAttachmentIcon`]). When the user activates the icon a
    /// conforming reader presents the embedded file for opening or
    /// saving. The underlying file specification dictionary is
    /// registered as an indirect object (and deduplicated by content
    /// hash) so multiple annotations on the same payload share one
    /// FileSpec.
    ///
    /// The alt text may be required by certain export profiles (e.g.
    /// PDF/UA). See [`FileAttachmentAnnotation`] for the available
    /// fields.
    pub fn new_file_attachment(
        annotation: FileAttachmentAnnotation,
        alt_text: Option<String>,
    ) -> Self {
        Self {
            annotation_type: AnnotationType::FileAttachment(annotation),
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

    /// Set the `/StructParent` entry — the integer key into the
    /// document's `/StructTreeRoot /ParentTree` that resolves to the
    /// structure element this annotation belongs to (ISO 32000-2
    /// §14.7.4.4 / ISO 14289-1 §7.18.1). PDF/UA-1 requires every
    /// annotation appearing inside a `/StructElem` to carry the
    /// corresponding `/StructParent`; conforming readers thread the
    /// reverse pointer to surface the annotation as an `OBJR` leaf
    /// under the tag tree.
    ///
    /// Most callers obtain this value implicitly by calling
    /// [`crate::page::Page::add_tagged_annotation`]; embedders that
    /// allocate the slot themselves (e.g. when feeding a custom
    /// structure-tree builder) may set it directly. Passing the value
    /// twice is harmless — the later call wins.
    pub fn with_struct_parent(mut self, struct_parent: i32) -> Self {
        self.struct_parent = Some(struct_parent);
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

impl From<FileAttachmentAnnotation> for Annotation {
    fn from(value: FileAttachmentAnnotation) -> Self {
        Self {
            annotation_type: AnnotationType::FileAttachment(value),
            alt: None,
            struct_parent: None,
            location: None,
        }
    }
}

impl Annotation {
    /// If this annotation is a [`WidgetField::RadioGroupChild`],
    /// return the indirect reference of its parent group dict.
    /// `InternalPage::serialize` uses this hook to populate the
    /// parent's `/Kids` array in annotation-insertion order.
    pub(crate) fn radio_group_parent_ref(&self) -> Option<Ref> {
        match &self.annotation_type {
            AnnotationType::Widget(w) => match &w.field {
                WidgetField::RadioGroupChild(child) => Some(child.parent_ref),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn serialize(
        &self,
        sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
        page_height: f32,
    ) -> KrillaResult<()> {
        // PDF/X-1a (ISO 15930-4) forbids every annotation type krilla
        // supports. Surface the violation so the error path catches it
        // before the annotation dict reaches the file.
        if sc
            .serialize_settings()
            .validators()
            .forbids_annotations()
        {
            sc.register_validation_error(ValidationError::ContainsAnnotation(self.location));
        }

        // Pre-resolve any pushbutton icon-appearance image refs *before*
        // the annotation dict starts writing — `register_image` needs
        // mutable access to `chunk_container`, which the annotation
        // dict's `&mut Chunk` would otherwise hold exclusively. The
        // resulting `Ref`s are threaded into `serialize_type` so the
        // widget's `/MK /I`, `/MK /RI`, and `/MK /IX` entries can name
        // the freshly-registered images. krilla deduplicates by image
        // hash, so multiple widgets sharing one `Image` clone resolve
        // to the same indirect reference.
        let widget_icon_refs: WidgetIconRefs = {
            #[cfg(feature = "raster-images")]
            {
                if let AnnotationType::Widget(w) = &self.annotation_type {
                    let normal = w
                        .icon_image
                        .as_ref()
                        .map(|image| sc.register_image(chunk_container, image.clone()));
                    let (rollover, alternate) = w
                        .appearance_characteristics
                        .as_ref()
                        .map(|mk| {
                            (
                                mk.rollover_icon
                                    .as_ref()
                                    .map(|image| sc.register_image(chunk_container, image.clone())),
                                mk.alternate_icon
                                    .as_ref()
                                    .map(|image| sc.register_image(chunk_container, image.clone())),
                            )
                        })
                        .unwrap_or((None, None));
                    WidgetIconRefs {
                        normal,
                        rollover,
                        alternate,
                    }
                } else {
                    WidgetIconRefs::default()
                }
            }
            #[cfg(not(feature = "raster-images"))]
            {
                WidgetIconRefs::default()
            }
        };

        // FileAttachment annotations need to register their
        // [`EmbeddedFile`] *before* the annotation dict opens so the
        // resulting `Ref` can be written into `/FS`. The embedded-file
        // FileSpec is itself an indirect object that wants mutable
        // access to `chunk_container`; the annotation dict otherwise
        // borrows `chunk.non_stream.annotations` for the entire
        // `serialize_type` call. Pre-resolve here so the borrow chain
        // stays acyclic.
        let file_spec_ref: Option<Ref> = match &self.annotation_type {
            AnnotationType::FileAttachment(f) => {
                Some(sc.register_cacheable(chunk_container, f.file.clone()))
            }
            _ => None,
        };

        let chunk = &mut chunk_container.non_stream.annotations;
        let mut annotation = chunk
            .indirect(root_ref)
            .start::<pdf_writer::writers::Annotation>();

        // Wire the pre-resolved FileSpec ref onto the annotation dict
        // before delegating to `serialize_type`. The FileAttachment
        // branch consumes the ref via `file_spec_ref` in its own
        // closure; other annotation types ignore it.
        if let Some(fs_ref) = file_spec_ref {
            annotation.pair(Name(b"FS"), fs_ref);
        }

        let appearance_job = self
            .annotation_type
            .serialize_type(sc, &mut annotation, page_height, widget_icon_refs)?;

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

        // AcroForm catalogue wiring (ISO 32000-2 §12.7.3): every
        // terminal widget annotation's indirect reference participates
        // in the catalogue's `/AcroForm /Fields` array. We register
        // the ref *after* the annotation chunk has been emitted;
        // `ChunkContainer::finish` remaps the ref through the same
        // remapper used for every other indirect object and writes
        // the array entry.
        //
        // Radio-group children are *not* terminal fields — they are
        // referenced from their parent's `/Kids` and reached via tree
        // traversal. The parent dict goes into `/AcroForm /Fields`
        // instead (emitted by `InternalPage::serialize` when consuming
        // queued `RadioGroupPayload`s).
        if let AnnotationType::Widget(w) = &self.annotation_type {
            if !matches!(w.field, WidgetField::RadioGroupChild(_)) {
                sc.register_widget_field(root_ref);
            }
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
/// references Helvetica. When the widget carries a pushbutton
/// icon-appearance (`/MK /I`), an additional Form XObject is emitted
/// that wraps the registered image — its `/Resources /XObject /Im0`
/// names the image ref and the content stream stretches it across the
/// widget's bbox.
fn emit_appearance_xobjects(chunk: &mut Chunk, job: AppearanceJob) {
    write_form_xobject(chunk, &job.on, job.helv_ref);
    if let Some(off) = job.off {
        write_form_xobject(chunk, &off, job.helv_ref);
    }
    if let Some(icon) = job.icons.normal {
        write_icon_form_xobject(chunk, &icon);
    }
    if let Some(icon) = job.icons.rollover {
        write_icon_form_xobject(chunk, &icon);
    }
    if let Some(icon) = job.icons.alternate {
        write_icon_form_xobject(chunk, &icon);
    }
}

/// Build an [`IconAppearanceXObject`] when both the source image ref
/// and an allocated Form XObject ref are present. Returns `None` when
/// either is absent (i.e. the embedder did not author this icon
/// slot).
fn icon_appearance_xobject(
    image_ref: Option<Ref>,
    xobject_ref: Option<Ref>,
    bbox_w: f32,
    bbox_h: f32,
) -> Option<IconAppearanceXObject> {
    match (image_ref, xobject_ref) {
        (Some(image_ref), Some(xobject_ref)) => Some(IconAppearanceXObject {
            xobject_ref,
            image_ref,
            bbox_w,
            bbox_h,
        }),
        _ => None,
    }
}

/// Emit a pushbutton icon-appearance Form XObject. The content stream
/// is `q <bbox_w> 0 0 <bbox_h> 0 0 cm /Im0 Do Q` — a single image draw
/// stretched to fill the widget's bbox. `/Resources /XObject /Im0`
/// names the registered image ref; the `/MK /I` entry on the widget
/// dict references this Form XObject directly.
fn write_icon_form_xobject(chunk: &mut Chunk, icon: &IconAppearanceXObject) {
    use std::fmt::Write as _;
    let mut content = String::with_capacity(48);
    // The image's intrinsic coordinate system is `1 x 1`; the matrix
    // scales it to fill `[0 0 bbox_w bbox_h]` so the icon stretches
    // across the widget rectangle. Authors needing fit / preserve-
    // aspect-ratio behaviour drive `/IF` via the embedder; krilla's
    // current surface emits a bare image draw.
    writeln!(
        &mut content,
        "q\n{:.4} 0 0 {:.4} 0 0 cm\n/Im0 Do\nQ",
        icon.bbox_w, icon.bbox_h,
    )
    .unwrap();

    let mut xobj = chunk.form_xobject(icon.xobject_ref, content.as_bytes());
    xobj.bbox(pdf_writer::Rect::new(0.0, 0.0, icon.bbox_w, icon.bbox_h));
    {
        let mut resources = xobj.resources();
        let mut xobjects = resources.x_objects();
        xobjects.pair(Name(b"Im0"), icon.image_ref);
        xobjects.finish();
        resources.finish();
    }
    xobj.finish();
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
    /// A file-attachment annotation (ISO 32000-2 §12.5.6.15).
    FileAttachment(FileAttachmentAnnotation),
}

impl AnnotationType {
    fn serialize_type(
        &self,
        sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
        widget_icon_refs: WidgetIconRefs,
    ) -> KrillaResult<Option<AppearanceJob>> {
        match self {
            AnnotationType::Link(l) => l.serialize_type(sc, annotation, page_height),
            AnnotationType::Text(t) => t.serialize_type(sc, annotation, page_height),
            AnnotationType::Markup(m) => m.serialize_type(sc, annotation, page_height),
            AnnotationType::Widget(w) => {
                w.serialize_type(sc, annotation, page_height, widget_icon_refs)
            }
            AnnotationType::FileAttachment(f) => f.serialize_type(sc, annotation, page_height),
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

/// `/BS << /S … >>` style code for a link-annotation border
/// (ISO 32000-2 §12.5.4 Table 165).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum LinkBorderStyle {
    /// `/S /S` — solid line.
    Solid,
    /// `/S /D` — dashed line; krilla emits the default `[3 3]` dash
    /// pattern alongside.
    Dashed,
    /// `/S /U` — single underline line on the bottom edge.
    Underline,
    /// `/S /I` — inset (depressed) line.
    Inset,
    /// `/S /B` — bevelled (raised) line.
    Beveled,
}

impl LinkBorderStyle {
    /// Project onto the pdf-writer enum used by the `/BS /S` writer.
    pub(crate) fn to_pdf(self) -> pdf_writer::types::BorderType {
        use pdf_writer::types::BorderType;
        match self {
            Self::Solid => BorderType::Solid,
            Self::Dashed => BorderType::Dashed,
            Self::Underline => BorderType::Underline,
            Self::Inset => BorderType::Inset,
            Self::Beveled => BorderType::Beveled,
        }
    }
}

/// Border of a link annotation.
pub struct LinkBorder {
    pub(crate) width: f32,
    pub(crate) color: Color,
    pub(crate) style: Option<LinkBorderStyle>,
}

impl LinkBorder {
    /// Create a new link annotation border.
    ///
    /// `width`: The width of the border in pt.
    /// `color`: The color of the border.
    pub fn new(width: f32, color: Color) -> Self {
        Self { width, color, style: None }
    }

    /// Set the `/BS << /S … >>` style code. When set, the resulting
    /// `/Link` annotation carries a full `/BS` sub-dictionary
    /// (`/Type /Border /W <width> /S <style> [/D <dashes>]`) per
    /// ISO 32000-2 §12.5.4. When unset, only the legacy `/Border`
    /// array is emitted.
    pub fn with_style(mut self, style: LinkBorderStyle) -> Self {
        self.style = Some(style);
        self
    }
}

/// `/H` highlight mode on a [`LinkAnnotation`] (ISO 32000-2 §12.5.6.5
/// Table 165, mirrored on widget annotations by ISO 32000-2 §12.5.6.19
/// Table 167). Determines the visual effect the conforming reader
/// applies while the user holds the pointer button over the annotation.
///
/// PDF/UA-1 §7.18.2.1 demands an explicit `/H` value on every link
/// annotation so assistive technology knows whether the activation
/// gesture conveys feedback to the user. Most viewers default to
/// `/H /I` (Invert) when the entry is missing; krilla writes the
/// explicit value when one is set, otherwise omits the entry and
/// relies on the viewer default.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub enum LinkHighlight {
    /// `/H /N` — no highlighting effect.
    None,
    /// `/H /I` — invert the contents of the annotation rectangle.
    /// Most PDF viewers treat this as the implicit default when the
    /// `/H` entry is absent.
    #[default]
    Invert,
    /// `/H /O` — invert the annotation's border (outline only).
    Outline,
    /// `/H /P` — display the annotation rectangle as if it had been
    /// pushed below the surface of the page.
    Push,
}

impl LinkHighlight {
    /// Project onto the pdf-writer enum used by the `/H` writer.
    pub(crate) fn to_pdf(self) -> pdf_writer::types::HighlightEffect {
        use pdf_writer::types::HighlightEffect;
        match self {
            Self::None => HighlightEffect::None,
            Self::Invert => HighlightEffect::Invert,
            Self::Outline => HighlightEffect::Outline,
            Self::Push => HighlightEffect::Push,
        }
    }
}

/// A link annotation.
pub struct LinkAnnotation {
    pub(crate) rect: Rect,
    pub(crate) quad_points: Option<Vec<Quadrilateral>>,
    pub(crate) target: Target,
    pub(crate) border: Option<LinkBorder>,
    pub(crate) highlight: Option<LinkHighlight>,
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
            highlight: None,
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
            highlight: None,
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

    /// Set the `/H` highlight mode (ISO 32000-2 §12.5.6.5 Table 165).
    /// PDF/UA-1 §7.18.2.1 requires every link annotation to carry an
    /// explicit highlight mode so assistive technology can convey the
    /// activation effect to users. When unset, krilla omits the `/H`
    /// entry and viewers fall back to their implicit default (`/I`
    /// Invert across most viewers).
    pub fn with_highlight(mut self, highlight: LinkHighlight) -> Self {
        self.highlight = Some(highlight);
        self
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

        // ISO 32000-2 §12.5.6.5 `/H` — explicit highlight mode.
        // PDF/UA-1 §7.18.2.1 mandates the entry; krilla writes the
        // explicit value when the embedder has called
        // `with_highlight(...)`. Without an explicit value the entry
        // is omitted and conforming readers default to `/H /I`
        // (Invert).
        if let Some(highlight) = self.highlight {
            annotation.highlight(highlight.to_pdf());
        }

        if let Some(border) = &self.border {
            write_color(annotation, &border.color);
            // ISO 32000-2 §12.5.4 — `/BS << /Type /Border /W /S [/D]
            // >>`. PDF 1.6+ viewers prefer the `/BS` sub-dictionary
            // over the legacy `/Border` array; krilla emits both so
            // older readers continue to see the border width.
            if let Some(style) = border.style {
                let mut bs = annotation.border_style();
                bs.width(border.width).style(style.to_pdf());
                // Dashed borders need a `/D` pattern array (default
                // `[3 3]`); other styles ignore the entry.
                if matches!(style, LinkBorderStyle::Dashed) {
                    bs.dashes([3.0_f32, 3.0_f32]);
                }
                bs.finish();
            }
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

/// Icon glyph for a [`FileAttachmentAnnotation`] (`/Name` entry,
/// ISO 32000-2 §12.5.6.15 Table 178). Viewers display the
/// corresponding pre-defined glyph at the annotation rectangle;
/// activating it opens or saves the embedded file.
///
/// The default ([`Self::PushPin`]) matches the ISO 32000-2
/// "shall be one of" defaulting behaviour — when `/Name` is absent
/// most viewers fall back to PushPin.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum FileAttachmentIcon {
    /// `/Graph` — a small bar-chart icon.
    Graph,
    /// `/Paperclip` — a paperclip icon.
    Paperclip,
    /// `/PushPin` — a push-pin icon. Default.
    #[default]
    PushPin,
    /// `/Tag` — a luggage-tag icon.
    Tag,
}

impl FileAttachmentIcon {
    fn to_pdf(self) -> pdf_writer::types::AnnotationIcon<'static> {
        use pdf_writer::types::AnnotationIcon;
        match self {
            FileAttachmentIcon::Graph => AnnotationIcon::Graph,
            FileAttachmentIcon::Paperclip => AnnotationIcon::Paperclip,
            FileAttachmentIcon::PushPin => AnnotationIcon::PushPin,
            FileAttachmentIcon::Tag => AnnotationIcon::Tag,
        }
    }
}

/// A file-attachment annotation per ISO 32000-2 §12.5.6.15.
///
/// File-attachment annotations pin an [`EmbeddedFile`] to a page
/// rectangle and display one of the predefined `/Name` icons
/// ([`FileAttachmentIcon`]). When the user activates the icon a
/// conforming reader presents the embedded file for opening or
/// saving.
///
/// The annotation's `/FS` entry is an indirect reference to a file
/// specification dictionary; krilla registers the [`EmbeddedFile`]
/// via [`crate::serialize::SerializeContext::register_cacheable`],
/// so multiple annotations sharing one payload (same path / mime /
/// data hash) dedupe onto a single FileSpec object. The annotation
/// does NOT automatically participate in the document catalogue's
/// `/Names /EmbeddedFiles` name tree — that channel is reserved for
/// document-level attachments registered via
/// [`crate::document::Document::embed_file`]. An author that wants
/// both a document-level entry and a page-level annotation pointing
/// at the same payload must call both APIs; the deduplication cache
/// guarantees a single FileSpec dict.
///
/// PDF/A-1 (ISO 19005-1 §6.5.2) forbids the `FileAttachment`
/// subtype outright; PDF/A-2 and later permit it. krilla emits the
/// annotation under every non-PDF/A-1 validator; the rejection is
/// documented for PDF/A-1 in `configure/PDF_A1.md` (no code-side
/// block — the validator's `forbids_annotations` path is PDF/X-1a
/// only, and PDF/A-1's rejection of FileAttachment is left to the
/// embedder to enforce).
///
/// Build with [`FileAttachmentAnnotation::new`] and the chainable
/// setter methods; wrap into an [`Annotation`] via
/// [`Annotation::new_file_attachment`] or
/// [`From<FileAttachmentAnnotation>`].
pub struct FileAttachmentAnnotation {
    pub(crate) rect: Rect,
    pub(crate) file: EmbeddedFile,
    pub(crate) icon: FileAttachmentIcon,
    pub(crate) contents: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) creation_date: Option<String>,
    pub(crate) modification_date: Option<String>,
}

impl FileAttachmentAnnotation {
    /// Create a new file-attachment annotation with the given
    /// bounding rectangle, embedded file and `/Name` icon.
    ///
    /// `rect` is in user-space (page) coordinates; krilla applies the
    /// same page-root transform as the other annotation kinds. The
    /// `file` is registered as a deduplicated indirect FileSpec
    /// object at serialisation time.
    pub fn new(rect: Rect, file: EmbeddedFile, icon: FileAttachmentIcon) -> Self {
        Self {
            rect,
            file,
            icon,
            contents: None,
            title: None,
            creation_date: None,
            modification_date: None,
        }
    }

    /// Set the `/Contents` text — the body of the pop-up shown when
    /// the user hovers over or activates the annotation. If the
    /// embedder also sets an `alt_text` on [`Annotation::new_file_attachment`]
    /// the outer alt-text wins (it is written after `/Contents` here).
    pub fn with_contents(mut self, contents: impl Into<String>) -> Self {
        self.contents = Some(contents.into());
        self
    }

    /// Set the `/T` text — the title bar of the pop-up. Typically
    /// the author's name.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the `/CreationDate` entry — the date the annotation was
    /// created, formatted as a PDF date string per ISO 32000-2
    /// §7.9.4 (e.g. `D:20260515120000Z`). The caller is responsible
    /// for constructing a syntactically valid date string; krilla
    /// emits the value verbatim as a literal string.
    pub fn with_creation_date(mut self, date: impl Into<String>) -> Self {
        self.creation_date = Some(date.into());
        self
    }

    /// Set the `/M` entry — the date the annotation was last
    /// modified, formatted as a PDF date string per ISO 32000-2
    /// §7.9.4. The caller is responsible for constructing a
    /// syntactically valid date string; krilla emits the value
    /// verbatim as a literal string.
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
        // ISO 32000-2 §12.5.6.15 — `/Subtype /FileAttachment`. The
        // `/FS` entry has already been written by the outer
        // [`Annotation::serialize`] using the indirect ref returned
        // by `register_cacheable`.
        annotation.subtype(pdf_writer::types::AnnotationType::FileAttachment);

        let actual_rect = self
            .rect
            .transform(page_root_transform(page_height))
            .unwrap();
        annotation.rect(actual_rect.to_pdf_rect());
        annotation.icon(self.icon.to_pdf());

        if let Some(title) = &self.title {
            annotation.author(TextStr(title));
        }

        if let Some(contents) = &self.contents {
            annotation.contents(TextStr(contents));
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

/// An AcroForm signature field (`/FT /Sig`) per ISO 32000-2 §12.7.4.5.
///
/// krilla emits the signature widget structure but does not sign the
/// document. The resulting field is *unsigned* — `/V` is omitted, ready
/// for a downstream signing pipeline (PAdES / PKCS#7) to populate the
/// signature dictionary. The optional [`SignatureLock`] becomes a
/// `/Lock` sub-dictionary on the widget that records which other fields
/// the signing tool must lock alongside this one (ISO 32000-2
/// §12.7.4.5, Table 232 — `SigFieldLock`).
///
/// Appearance: krilla emits an empty Form XObject so PDF viewers
/// render the field as a blank rectangle until it has been signed.
/// Signing tools typically replace the appearance stream when they
/// populate `/V`.
pub struct SignatureField {
    /// `/Lock` sub-dictionary contents. `SignatureLock::None` omits
    /// the `/Lock` entry entirely.
    pub lock: SignatureLock,
}

/// `/Lock` sub-dictionary of a signature field (ISO 32000-2
/// §12.7.4.5, Table 232 — `SigFieldLock`).
///
/// Determines which other fields a downstream signing tool must lock
/// after applying the signature. `None` omits the dictionary; the
/// signing tool is then free to lock or leave fields alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureLock {
    /// No `/Lock` dictionary emitted.
    None,
    /// `/Lock << /Type /SigFieldLock /Action /All >>` — every field
    /// in the document is locked when the signature is applied.
    All,
    /// `/Lock << /Type /SigFieldLock /Action /Include /Fields [...] >>`
    /// — only the listed field names are locked.
    Include {
        /// Fully qualified field names written into `/Fields`.
        fields: Vec<String>,
    },
    /// `/Lock << /Type /SigFieldLock /Action /Exclude /Fields [...] >>`
    /// — every field *except* the listed names is locked.
    Exclude {
        /// Fully qualified field names written into `/Fields`.
        fields: Vec<String>,
    },
}

/// A single child widget of a [`RadioGroupField`] — one HTML
/// `<input type="radio">` element. Each child contributes a widget
/// annotation to its page; the field-tree wiring (parent dict, `/T`,
/// `/V`, `/Ff`) lives on the parent and is emitted alongside the
/// catalogue's `/AcroForm /Fields` array.
#[derive(Clone, Debug)]
pub struct RadioChild {
    /// Rectangle of the child widget annotation in user-space (page)
    /// coordinates.
    pub rect: Rect,
    /// Export value — the PDF name written into the child's `/AS`
    /// when this child is the selected member of the group. The
    /// parent's `/V` carries the same name for the currently-selected
    /// child.
    pub export_value: String,
}

/// A non-terminal AcroForm radio-button field (ISO 32000-2 §12.7.5.2.3).
///
/// HTML `<input type="radio">` elements that share a `name` are
/// mutually exclusive: at most one is checked. The PDF encoding is a
/// single non-terminal `/Btn` field with the Radio flag (`/Ff` bit 16)
/// set, whose `/Kids` are the per-radio widget annotations on the
/// page. The parent dict carries `/T` (field name), `/V` (selected
/// export value or `/Off`) and `/DV` (default selection); each child
/// carries `/AS` (its export value when selected or `/Off`) and
/// `/Parent`. Only the parent dict's indirect reference participates
/// in `/AcroForm /Fields`; children are reached via tree traversal.
///
/// Use [`crate::page::Page::add_radio_group`] to attach a group to a
/// page — that entry point allocates the parent ref, builds the
/// per-child widget annotations and queues the parent dict for
/// catalogue emission.
#[derive(Clone, Debug)]
pub struct RadioGroupField {
    /// Field name — written into the parent's `/T`. Matches the HTML
    /// `name` attribute shared by every radio in the group.
    pub name: String,
    /// Per-radio child widgets. Order is preserved; the resulting
    /// `/Kids` array follows the same order.
    pub children: Vec<RadioChild>,
    /// Export value of the currently-selected child, or `None` when
    /// no radio is checked (the parent's `/V` falls back to `/Off`).
    pub selected_export: Option<String>,
    /// Default selection used by form reset and written into `/DV`.
    /// `None` emits `/DV /Off`.
    pub default_selected_export: Option<String>,
    /// Per-field flag bits — see [`ButtonFieldFlags`]. The Radio flag
    /// (bit 16) is forced on by the catalogue emitter; the embedder
    /// may set [`ButtonFieldFlags::radios_in_unison`] or
    /// [`ButtonFieldFlags::read_only`] as appropriate.
    pub flags: ButtonFieldFlags,
}

impl RadioGroupField {
    /// Build a radio group with the given name and children. By
    /// default no child is selected and the Radio flag is set; mutate
    /// [`Self::selected_export`], [`Self::default_selected_export`]
    /// and [`Self::flags`] directly to refine the field.
    pub fn new(name: impl Into<String>, children: Vec<RadioChild>) -> Self {
        Self {
            name: name.into(),
            children,
            selected_export: None,
            default_selected_export: None,
            flags: ButtonFieldFlags::default().with_radio(true),
        }
    }

    /// Set the currently-selected child's export value (becomes `/V`).
    pub fn with_selected(mut self, selected: Option<String>) -> Self {
        self.selected_export = selected;
        self
    }

    /// Set the default-selected child's export value (becomes `/DV`).
    pub fn with_default_selected(mut self, default_selected: Option<String>) -> Self {
        self.default_selected_export = default_selected;
        self
    }

    /// Set the RadiosInUnison flag (bit 26 — radios sharing an export
    /// value toggle together). Off by default per ISO 32000-2 Table
    /// 226 default behaviour.
    pub fn with_in_unison(mut self, value: bool) -> Self {
        self.flags = self.flags.with_radios_in_unison(value);
        self
    }
}

/// The field-type-specific payload of a [`WidgetAnnotation`].
pub enum WidgetField {
    /// `/Tx` text field.
    Text(TextField),
    /// `/Btn` button field (checkbox, radio, pushbutton). Single
    /// ungrouped radios continue to use this variant with
    /// [`ButtonKind::Radio`]; grouped radios go through
    /// [`RadioGroupField`] and emit as
    /// [`WidgetField::RadioGroupChild`] internally.
    Button(ButtonField),
    /// `/Ch` choice field (combo / list box).
    Choice(ChoiceField),
    /// One member of a [`RadioGroupField`] — a child widget annotation
    /// that points at its parent group dict via `/Parent`. Constructed
    /// by [`crate::page::Page::add_radio_group`]; callers do not build
    /// this variant directly.
    RadioGroupChild(RadioGroupChild),
    /// `/Sig` signature field — an unsigned signature widget that a
    /// downstream signing pipeline (e.g. PAdES, PKCS#7) fills in with
    /// the signature dictionary. krilla itself does not sign documents;
    /// it emits the widget structure (`/FT /Sig`, `/T`, optional
    /// `/Lock`) so the document is ready to be signed. See
    /// [`SignatureField`].
    Signature(SignatureField),
}

/// One radio-group child widget — the field-tree dispatch payload for
/// [`WidgetField::RadioGroupChild`]. Constructed by
/// [`crate::page::Page::add_radio_group`].
pub struct RadioGroupChild {
    /// Indirect reference of the parent group dict, allocated before
    /// the children are serialised so each child can carry `/Parent`.
    pub(crate) parent_ref: Ref,
    /// Export value emitted as `/AS /<export_value>` when selected,
    /// otherwise `/AS /Off`.
    pub(crate) export_value: String,
    /// Whether this child is the currently-selected member of the
    /// group — drives `/AS` between the export-value name and `/Off`.
    pub(crate) is_selected: bool,
}

impl RadioGroupChild {
    pub(crate) fn new(parent_ref: Ref, export_value: String, is_selected: bool) -> Self {
        Self {
            parent_ref,
            export_value,
            is_selected,
        }
    }
}

/// Flag bits for an AcroForm text field (`/FT /Tx`).
///
/// The general flags `READ_ONLY`, `REQUIRED` and `NO_EXPORT` are shared
/// with the other field types; the multi-line / password / file-select
/// / no-scroll / comb / rich-text / do-not-spell-check bits are
/// text-specific. See ISO 32000-2 §12.7.4.3, Table 230.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct TextFieldFlags {
    /// Set bit 1 (`/Ff` 1): the field is read-only.
    pub read_only: bool,
    /// Set bit 2 (`/Ff` 2): the field is required at submission.
    pub required: bool,
    /// Set bit 3 (`/Ff` 4): the field is excluded from submission.
    pub no_export: bool,
    /// Set bit 13 (4096): multi-line text input.
    pub multiline: bool,
    /// Set bit 14 (8192): password — characters not echoed.
    pub password: bool,
    /// Set bit 21 (1 << 20): the field stores a filename to be
    /// submitted as a file (PDF 1.4+).
    pub file_select: bool,
    /// Set bit 23 (1 << 22): the field is excluded from spell-checking.
    pub do_not_spell_check: bool,
    /// Set bit 24 (1 << 23): the field is not scrollable. PDF 1.4+.
    pub do_not_scroll: bool,
    /// Set bit 25 (1 << 24): the field is automatically divided into
    /// equally-spaced character positions (comb). Requires `MaxLen` and
    /// none of `Multiline`, `Password`, `FileSelect`. PDF 1.5+.
    pub comb: bool,
    /// Set bit 26 (1 << 25): the field value is treated as rich text.
    /// PDF 1.5+.
    pub rich_text: bool,
}

impl TextFieldFlags {
    /// Set the read-only flag (bit 1).
    pub fn with_read_only(mut self, value: bool) -> Self {
        self.read_only = value;
        self
    }

    /// Set the required flag (bit 2).
    pub fn with_required(mut self, value: bool) -> Self {
        self.required = value;
        self
    }

    /// Set the no-export flag (bit 3).
    pub fn with_no_export(mut self, value: bool) -> Self {
        self.no_export = value;
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

    /// Set the file-select flag (bit 21).
    pub fn with_file_select(mut self, value: bool) -> Self {
        self.file_select = value;
        self
    }

    /// Set the do-not-spell-check flag (bit 23).
    pub fn with_do_not_spell_check(mut self, value: bool) -> Self {
        self.do_not_spell_check = value;
        self
    }

    /// Set the do-not-scroll flag (bit 24).
    pub fn with_do_not_scroll(mut self, value: bool) -> Self {
        self.do_not_scroll = value;
        self
    }

    /// Set the comb flag (bit 25). Requires `MaxLen` and none of
    /// `Multiline`, `Password`, `FileSelect`. PDF 1.5+.
    pub fn with_comb(mut self, value: bool) -> Self {
        self.comb = value;
        self
    }

    /// Set the rich-text flag (bit 26). PDF 1.5+.
    pub fn with_rich_text(mut self, value: bool) -> Self {
        self.rich_text = value;
        self
    }

    fn to_bits(self) -> u32 {
        let mut flags = FieldFlags::empty();
        if self.read_only {
            flags |= FieldFlags::READ_ONLY;
        }
        if self.required {
            flags |= FieldFlags::REQUIRED;
        }
        if self.no_export {
            flags |= FieldFlags::NO_EXPORT;
        }
        if self.multiline {
            flags |= FieldFlags::MULTILINE;
        }
        if self.password {
            flags |= FieldFlags::PASSWORD;
        }
        if self.file_select {
            flags |= FieldFlags::FILE_SELECT;
        }
        if self.do_not_spell_check {
            flags |= FieldFlags::DO_NOT_SPELL_CHECK;
        }
        if self.do_not_scroll {
            flags |= FieldFlags::DO_NOT_SCROLL;
        }
        if self.comb {
            flags |= FieldFlags::COMB;
        }
        if self.rich_text {
            flags |= FieldFlags::RICH_TEXT;
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
/// `pushbutton` (bit 17). The general `required` / `no_export` bits
/// (bits 2 / 3) are shared with the other field types.
/// See ISO 32000-2 §12.7.4.2, Table 229.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ButtonFieldFlags {
    /// Set bit 1 (`/Ff` 1): the field is read-only.
    pub read_only: bool,
    /// Set bit 2 (`/Ff` 2): the field is required at submission.
    pub required: bool,
    /// Set bit 3 (`/Ff` 4): the field is excluded from submission.
    pub no_export: bool,
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

    /// Set the required flag (bit 2).
    pub fn with_required(mut self, value: bool) -> Self {
        self.required = value;
        self
    }

    /// Set the no-export flag (bit 3).
    pub fn with_no_export(mut self, value: bool) -> Self {
        self.no_export = value;
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

    pub(crate) fn to_bits(self) -> u32 {
        let mut flags = FieldFlags::empty();
        if self.read_only {
            flags |= FieldFlags::READ_ONLY;
        }
        if self.required {
            flags |= FieldFlags::REQUIRED;
        }
        if self.no_export {
            flags |= FieldFlags::NO_EXPORT;
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
/// single-select fields; the API is provided for completeness. The
/// general `required` / `no_export` / `do_not_spell_check` bits
/// (bits 2 / 3 / 23) are shared with the other field types.
/// See ISO 32000-2 §12.7.4.4, Table 232.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ChoiceFieldFlags {
    /// Set bit 1 (`/Ff` 1): the field is read-only.
    pub read_only: bool,
    /// Set bit 2 (`/Ff` 2): the field is required at submission.
    pub required: bool,
    /// Set bit 3 (`/Ff` 4): the field is excluded from submission.
    pub no_export: bool,
    /// Set bit 18 (131072): combo box (drop-down) instead of list box.
    pub combo: bool,
    /// Set bit 22 (2097152): multi-select.
    pub multi_select: bool,
    /// Set bit 23 (1 << 22): the field is excluded from spell-checking.
    /// Only meaningful for combo boxes with an edit control.
    pub do_not_spell_check: bool,
}

impl ChoiceFieldFlags {
    /// Set the read-only flag (bit 1).
    pub fn with_read_only(mut self, value: bool) -> Self {
        self.read_only = value;
        self
    }

    /// Set the required flag (bit 2).
    pub fn with_required(mut self, value: bool) -> Self {
        self.required = value;
        self
    }

    /// Set the no-export flag (bit 3).
    pub fn with_no_export(mut self, value: bool) -> Self {
        self.no_export = value;
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

    /// Set the do-not-spell-check flag (bit 23). Only meaningful for
    /// combo boxes with an edit control.
    pub fn with_do_not_spell_check(mut self, value: bool) -> Self {
        self.do_not_spell_check = value;
        self
    }

    fn to_bits(self) -> u32 {
        let mut flags = FieldFlags::empty();
        if self.read_only {
            flags |= FieldFlags::READ_ONLY;
        }
        if self.required {
            flags |= FieldFlags::REQUIRED;
        }
        if self.no_export {
            flags |= FieldFlags::NO_EXPORT;
        }
        if self.combo {
            flags |= FieldFlags::COMBO;
        }
        if self.multi_select {
            flags |= FieldFlags::MULTI_SELECT;
        }
        if self.do_not_spell_check {
            flags |= FieldFlags::DO_NOT_SPELL_CHECK;
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
    pub(crate) tooltip: Option<String>,
    /// `/AA /K` — keystroke action. Fires before a value change is
    /// committed; typically wired to `AFDate_KeystrokeEx(fmt)` /
    /// `AFTime_Keystroke(fmt)` / `AFNumber_Keystroke(...)` so the
    /// viewer rejects out-of-range characters as the user types.
    pub(crate) keystroke_action: Option<Action>,
    /// `/AA /F` — format action. Fires before the value is displayed;
    /// typically `AFDate_FormatEx(fmt)` / `AFTime_Format(fmt)` /
    /// `AFNumber_Format(...)` so the field re-renders the canonical
    /// formatted value.
    pub(crate) format_action: Option<Action>,
    /// `/AA /V` — validate action. Fires after a value change has
    /// been committed; typically `AFRange_Validate(true, min, true, max)`
    /// for numeric range bounds. The action may reject the change by
    /// setting `event.rc = false` so the viewer reverts the field.
    pub(crate) validate_action: Option<Action>,
    /// `/MK /I` — pushbutton icon appearance (ISO 32000-2 §12.5.6.19
    /// Table 167). When set, krilla emits a Form XObject wrapping
    /// the image and threads its indirect reference into the
    /// widget's `/MK` dictionary. The image draws into the widget's
    /// `/BBox [0 0 w h]` so the viewer fills the button area with
    /// the icon. Only meaningful for pushbutton widgets
    /// (`<input type="image">`); krilla silently ignores the value
    /// on other field types.
    #[cfg(feature = "raster-images")]
    pub(crate) icon_image: Option<crate::graphics::image::Image>,
    /// `/DA` — default appearance string (ISO 32000-2 §12.7.4.3 Table
    /// 230). Overrides krilla's built-in `/Helv 10 Tf 0 g` fallback
    /// so the widget can reference a specific font and colour. The
    /// string is written verbatim as a PDF byte string; the embedder
    /// is responsible for escaping. Only meaningful for variable-text
    /// fields (`/Tx`, `/Ch`); krilla writes the value on Text and
    /// Choice widgets and ignores it elsewhere.
    pub(crate) default_appearance: Option<String>,
    /// `/MK` (Appearance Characteristics) supplementary entries —
    /// ISO 32000-2 §12.5.6.19 Table 167. Carries `/BC`, `/BG`, `/R`,
    /// `/RC`, `/AC`. The existing `/MK /CA` (caption) and `/MK /I`
    /// (icon) entries the pushbutton path emits compose with these —
    /// when both this field and the pushbutton path contribute, the
    /// serialiser writes a single `/MK` dict carrying every populated
    /// entry. `None` (the constructor default) suppresses the
    /// extension entirely; pushbutton widgets still emit their
    /// existing `/CA` + `/I` entries when applicable.
    pub(crate) appearance_characteristics: Option<AppearanceCharacteristics>,
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
            tooltip: None,
            keystroke_action: None,
            format_action: None,
            validate_action: None,
            #[cfg(feature = "raster-images")]
            icon_image: None,
            default_appearance: None,
            appearance_characteristics: None,
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

    /// Set the widget's `/TU` alternate field name — the human-readable
    /// label that PDF readers surface as a tooltip when the pointer
    /// hovers over the field, and that assistive technology
    /// (screen readers, voice-control tools) speaks to identify the
    /// field. Defined by ISO 32000-2 §12.7.4.1 Table 226 (`TU`,
    /// "Alternate field name").
    ///
    /// `/TU` is inheritable across the field hierarchy; krilla
    /// therefore omits it on radio-group children, which inherit
    /// the parent's value alongside `/T` and `/FT`.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Set the widget's `/AA /K` (keystroke) additional action —
    /// per ISO 32000-2 §12.7.4 Table 230. Fires before a value change
    /// is committed; the action may reject the change by setting
    /// `event.rc = false`.
    ///
    /// Typical use: `Action::JavaScript(JavaScriptAction::new(
    /// "AFDate_KeystrokeEx(\"yyyy-mm-dd\");"))` to validate keystrokes
    /// against an ISO date format. Passing `None` to a future setter
    /// would clear the slot; the present builder is additive and is
    /// expected to be called at most once per slot.
    pub fn with_keystroke_action(mut self, action: Action) -> Self {
        self.keystroke_action = Some(action);
        self
    }

    /// Set the widget's `/AA /F` (format) additional action —
    /// per ISO 32000-2 §12.7.4 Table 230. Fires before the field's
    /// value is rendered so the action may rewrite it (e.g. reformat
    /// a date as `dd/mm/yyyy`). Typical use: the corresponding
    /// `AFDate_FormatEx` / `AFTime_Format` / `AFNumber_Format` helper.
    pub fn with_format_action(mut self, action: Action) -> Self {
        self.format_action = Some(action);
        self
    }

    /// Set the widget's `/AA /V` (validate) additional action —
    /// per ISO 32000-2 §12.7.4 Table 230. Fires after the value has
    /// been committed; the action may set `event.rc = false` to
    /// revert. Typical use: `AFRange_Validate(true, min, true, max)`
    /// on a numeric or range field.
    pub fn with_validate_action(mut self, action: Action) -> Self {
        self.validate_action = Some(action);
        self
    }

    /// Attach an icon appearance image to a pushbutton widget
    /// (`<input type="image">`). The image surfaces as `/MK /I` on the
    /// widget annotation per ISO 32000-2 §12.5.6.19 Table 167; PDF
    /// viewers draw the icon inside the widget's `/BBox`.
    ///
    /// krilla registers the image once and wraps it in a Form XObject
    /// at serialisation time; multiple widgets pointing at the same
    /// `Image` clone share the underlying image resource. The icon
    /// entry is only meaningful for pushbutton fields (`WidgetField::
    /// Button(ButtonField { kind: ButtonKind::PushButton, .. })`);
    /// other widget kinds silently ignore the value.
    #[cfg(feature = "raster-images")]
    pub fn with_icon_appearance(mut self, image: crate::graphics::image::Image) -> Self {
        self.icon_image = Some(image);
        self
    }

    /// Override the widget's `/DA` (default appearance) string. The
    /// supplied bytes are written verbatim as the `/DA` entry per ISO
    /// 32000-2 §12.7.4.3 Table 230. Use for variable-text fields
    /// (`/Tx`, `/Ch`) when the widget should reference a specific font
    /// or colour (e.g. `/Courier 10 Tf 0 g` for a monospaced hex
    /// literal). When unset, krilla falls back to its built-in
    /// `/Helv 10 Tf 0 g` default. The embedder is responsible for
    /// supplying a syntactically valid `/DA` operator sequence.
    pub fn with_default_appearance(mut self, da: impl Into<String>) -> Self {
        self.default_appearance = Some(da.into());
        self
    }

    /// Attach the widget's `/MK` (Appearance Characteristics)
    /// supplementary entries — ISO 32000-2 §12.5.6.19 Table 167. The
    /// supplied structure carries `/BC` (border colour), `/BG`
    /// (background colour), `/R` (rotation), `/RC` (rollover caption),
    /// `/AC` (down caption), `/RI` (rollover icon), `/IX` (alternate
    /// (down) icon), `/IF` (icon fit dictionary), and `/TP` (text
    /// position) entries; krilla composes the supplied entries with
    /// the pushbutton path's existing `/CA` (caption) and `/I`
    /// (normal-state icon) entries so a single `/MK` dictionary
    /// surfaces every populated field. `/RC`, `/AC`, `/RI`, `/IX`,
    /// `/IF`, and `/TP` are meaningful only for pushbutton widgets
    /// per the spec; the serialiser writes them verbatim when set and
    /// viewers ignore the surplus on other field types. Passing an
    /// empty [`AppearanceCharacteristics`] (every slot `None`) is a
    /// no-op: krilla still recognises the call but omits the `/MK`
    /// extension at serialisation time.
    pub fn with_appearance_characteristics(mut self, mk: AppearanceCharacteristics) -> Self {
        self.appearance_characteristics = Some(mk);
        self
    }

    fn serialize_type(
        &self,
        sc: &mut SerializeContext,
        annotation: &mut pdf_writer::writers::Annotation,
        page_height: f32,
        widget_icon_refs: WidgetIconRefs,
    ) -> KrillaResult<Option<AppearanceJob>> {
        annotation.subtype(pdf_writer::types::AnnotationType::Widget);

        let actual_rect = self
            .rect
            .transform(page_root_transform(page_height))
            .unwrap();
        annotation.rect(actual_rect.to_pdf_rect());

        // /T is required on every terminal field; /FT identifies the
        // field type. The values pulled out of `WidgetField` populate
        // /V, /DV, /MaxLen, /Opt and /Ff. Radio-group children inherit
        // all of these from the parent (ISO 32000-2 §12.7.5.2.3) and
        // therefore omit /T, /FT, /Ff and /V entirely — the per-arm
        // serialisation below decides whether to emit them.
        let is_radio_group_child = matches!(&self.field, WidgetField::RadioGroupChild(_));
        if !is_radio_group_child {
            annotation.pair(Name(b"T"), TextStr(&self.partial_name));
            if let Some(tooltip) = &self.tooltip {
                annotation.pair(Name(b"TU"), TextStr(tooltip));
            }
        }

        // /DA is mandatory on every variable-text field (and harmless
        // elsewhere). A minimal default appearance — Helvetica 10pt
        // black — matches what Acrobat falls back to when /DA is
        // missing but `/NeedAppearances true` is set at the catalogue.
        // Callers may override via `with_default_appearance(...)` to
        // request a specific font / colour (e.g. monospaced `/Courier`
        // for a hex literal). The override is consulted on Text and
        // Choice arms; other field types ignore the slot.
        const DEFAULT_APPEARANCE: &[u8] = b"/Helv 10 Tf 0 g";
        let da_bytes: &[u8] = self
            .default_appearance
            .as_deref()
            .map(str::as_bytes)
            .unwrap_or(DEFAULT_APPEARANCE);

        let bbox_w = self.rect.width();
        let bbox_h = self.rect.height();

        // Per-widget content stream(s) for `/AP /N`. Pre-allocated here
        // so we can write the `/AP` reference into the annotation dict;
        // the actual Form XObject indirect objects are emitted by
        // `Annotation::serialize` after `annotation.finish()`.
        let helv_ref = sc.standard_helvetica_ref();
        let job: AppearanceJob;

        // `/MK` (Appearance Characteristics) dictionary contributions
        // accumulated across the field-type arms. The PushButton arm
        // sets `pushbutton_caption` to a non-empty string when the
        // button carries a label, and populates the
        // `pushbutton_icon_xobject_refs` triple with the per-icon-state
        // Form XObject indirect refs allocated for `/I`, `/RI`, `/IX`.
        // These supplement `self.appearance_characteristics`; after the
        // match we emit a single `/MK` dictionary carrying every
        // populated entry, or nothing when every contribution is empty
        // (ISO 32000-2 §12.5.6.19 Table 167).
        let mut pushbutton_caption: Option<&str> = None;
        let mut pushbutton_icon_xobject_refs = WidgetIconRefs::default();

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
                annotation.pair(Name(b"DA"), Str(da_bytes));
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
                    icons: WidgetIconXObjects::default(),
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
                            icons: WidgetIconXObjects::default(),
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
                            icons: WidgetIconXObjects::default(),
                        };
                    }
                    ButtonKind::PushButton => {
                        // Pushbuttons have no persistent value. /MK
                        // carries the optional caption (/CA, ISO
                        // 32000-2 §12.5.6.19 Table 167) and the icon
                        // family (/I, /RI, /IX — same table). HTML
                        // `<input type="image">` routes the normal-state
                        // image through
                        // `WidgetAnnotation::with_icon_appearance`; the
                        // rollover (`/RI`) and alternate (`/IX`) images
                        // ride along on
                        // `AppearanceCharacteristics::{rollover_icon,
                        // alternate_icon}`. The caption is the alt-text
                        // fallback for viewers that cannot decode the
                        // icon (or while the image is loading). The /MK
                        // dict itself is written by a single
                        // `write_mk_dictionary` pass after the match so
                        // the supplementary entries threaded through
                        // `appearance_characteristics` (/BC, /BG, /R,
                        // /RC, /AC, /IF, /TP) compose with the caption
                        // / icon family into one merged `/MK`
                        // dictionary.
                        let icon_xobject_refs = WidgetIconRefs {
                            normal: widget_icon_refs.normal.map(|_| sc.new_ref()),
                            rollover: widget_icon_refs.rollover.map(|_| sc.new_ref()),
                            alternate: widget_icon_refs.alternate.map(|_| sc.new_ref()),
                        };
                        if !button.caption.is_empty() {
                            pushbutton_caption = Some(button.caption.as_str());
                        }
                        pushbutton_icon_xobject_refs = icon_xobject_refs;
                        let ap_ref = sc.new_ref();
                        write_ap_single(annotation, ap_ref);
                        let icons = WidgetIconXObjects {
                            normal: icon_appearance_xobject(
                                widget_icon_refs.normal,
                                icon_xobject_refs.normal,
                                bbox_w,
                                bbox_h,
                            ),
                            rollover: icon_appearance_xobject(
                                widget_icon_refs.rollover,
                                icon_xobject_refs.rollover,
                                bbox_w,
                                bbox_h,
                            ),
                            alternate: icon_appearance_xobject(
                                widget_icon_refs.alternate,
                                icon_xobject_refs.alternate,
                                bbox_w,
                                bbox_h,
                            ),
                        };
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
                            icons,
                        };
                    }
                }
            }
            WidgetField::RadioGroupChild(child) => {
                // Child widgets of a non-terminal radio-group field carry
                // /Parent + /AS only — they inherit /FT, /Ff, /T, /V from
                // the parent (ISO 32000-2 §12.7.5.2.3). No /T, no /FT,
                // no /Ff, no /V on a child: that is what makes the group
                // a single mutually-exclusive field.
                annotation.pair(Name(b"Parent"), child.parent_ref);
                let state_bytes: Vec<u8> = if child.is_selected {
                    child.export_value.as_bytes().to_vec()
                } else {
                    b"Off".to_vec()
                };
                annotation.pair(Name(b"AS"), Name(&state_bytes));
                let on_ref = sc.new_ref();
                let off_ref = sc.new_ref();
                write_ap_on_off_with_state(
                    annotation,
                    &child.export_value,
                    on_ref,
                    off_ref,
                );
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
                    icons: WidgetIconXObjects::default(),
                };
            }
            WidgetField::Signature(sig) => {
                // ISO 32000-2 §12.7.4.5: a signature field carries
                // `/FT /Sig` plus optional `/Lock` and `/SV` (seed value).
                // krilla emits the field unsigned — `/V` is *omitted*
                // entirely so a downstream signing pipeline can fill it
                // in without rewriting the widget structure.
                annotation.pair(Name(b"FT"), Name(b"Sig"));
                match &sig.lock {
                    SignatureLock::None => {}
                    SignatureLock::All => {
                        let mut lock = annotation.insert(Name(b"Lock")).dict();
                        lock.pair(Name(b"Type"), Name(b"SigFieldLock"));
                        lock.pair(Name(b"Action"), Name(b"All"));
                        lock.finish();
                    }
                    SignatureLock::Include { fields } => {
                        let mut lock = annotation.insert(Name(b"Lock")).dict();
                        lock.pair(Name(b"Type"), Name(b"SigFieldLock"));
                        lock.pair(Name(b"Action"), Name(b"Include"));
                        let mut arr = lock.insert(Name(b"Fields")).array();
                        for f in fields {
                            arr.item(TextStr(f));
                        }
                        arr.finish();
                        lock.finish();
                    }
                    SignatureLock::Exclude { fields } => {
                        let mut lock = annotation.insert(Name(b"Lock")).dict();
                        lock.pair(Name(b"Type"), Name(b"SigFieldLock"));
                        lock.pair(Name(b"Action"), Name(b"Exclude"));
                        let mut arr = lock.insert(Name(b"Fields")).array();
                        for f in fields {
                            arr.item(TextStr(f));
                        }
                        arr.finish();
                        lock.finish();
                    }
                }
                // An empty appearance stream — the field is unsigned so
                // there is nothing to display. Signing tools replace this
                // when they populate `/V`. We still emit a Form XObject
                // reference so viewers do not synthesise a fallback
                // appearance from `/V` (which is absent).
                let ap_ref = sc.new_ref();
                write_ap_single(annotation, ap_ref);
                job = AppearanceJob {
                    helv_ref,
                    on: AppearanceStream {
                        xobject_ref: ap_ref,
                        bbox_w,
                        bbox_h,
                        content: build_empty_signature_content(),
                        uses_helvetica: false,
                    },
                    off: None,
                    icons: WidgetIconXObjects::default(),
                };
            }
            WidgetField::Choice(choice) => {
                annotation.pair(Name(b"FT"), Name(b"Ch"));
                annotation.pair(Name(b"Ff"), choice.flags.to_bits() as i32);
                annotation.pair(Name(b"DA"), Str(da_bytes));
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
                    icons: WidgetIconXObjects::default(),
                };
            }
        }

        // `/MK` (Appearance Characteristics) — ISO 32000-2 §12.5.6.19
        // Table 167. Merges the per-field-type contributions
        // (`/CA`, `/I`, `/RI`, `/IX` populated by the PushButton arm)
        // with the supplementary entries the embedder authored via
        // `with_appearance_characteristics` (`/BC`, `/BG`, `/R`, `/RC`,
        // `/AC`, `/IF`, `/TP`). Suppressed entirely when no entry is
        // populated so the `/MK` slot stays absent from the annotation
        // dictionary, matching the spec's "optional" behaviour.
        write_mk_dictionary(
            annotation,
            pushbutton_caption,
            pushbutton_icon_xobject_refs,
            self.appearance_characteristics.as_ref(),
        );

        // `/AA` additional-actions dictionary (ISO 32000-2 §12.7.4
        // Table 230). Emitted only when at least one of the
        // form-field action slots — keystroke (`/K`), format (`/F`),
        // validate (`/V`) — is populated; the dict is otherwise
        // omitted because an empty `/AA` is meaningless to viewers.
        // Each populated slot gets its own action sub-dictionary
        // routed through `Action::serialize`, which selects the
        // correct `/S` action type (`/JavaScript`, `/GoTo`, `/URI`,
        // …) and writes the type-specific payload.
        //
        // Radio-group child widgets inherit `/AA` from their parent
        // group (the AcroForm field-tree rule for terminal-field
        // entries per §12.7.4.1) so suppress the slot when this
        // widget is a radio-group child — emitting `/AA` here would
        // override the inherited value with one tied to a child
        // annotation that the viewer addresses by its `/AS` state.
        if !is_radio_group_child
            && (self.keystroke_action.is_some()
                || self.format_action.is_some()
                || self.validate_action.is_some())
        {
            let mut aa = annotation.insert(Name(b"AA")).dict();
            if let Some(action) = &self.keystroke_action {
                let action_writer = aa.insert(Name(b"K")).start();
                action.serialize(sc, action_writer)?;
            }
            if let Some(action) = &self.format_action {
                let action_writer = aa.insert(Name(b"F")).start();
                action.serialize(sc, action_writer)?;
            }
            if let Some(action) = &self.validate_action {
                let action_writer = aa.insert(Name(b"V")).start();
                action.serialize(sc, action_writer)?;
            }
            aa.finish();
        }

        Ok(Some(job))
    }
}

/// Write the `/MK` (Appearance Characteristics) dictionary for an
/// AcroForm widget annotation per ISO 32000-2 §12.5.6.19 Table 167.
///
/// Combines the pushbutton-arm-emitted entries (`/CA` caption, `/I`
/// normal-state icon Form XObject reference, `/RI` rollover icon ref,
/// `/IX` alternate (down) icon ref) with the supplementary entries the
/// embedder authored via
/// [`WidgetAnnotation::with_appearance_characteristics`] — `/BC` border
/// colour, `/BG` background colour, `/R` rotation, `/RC` rollover
/// caption, `/AC` alternate (down) caption, `/IF` icon fit
/// dictionary, `/TP` text position keyword. When every contribution
/// is empty the dictionary is omitted entirely so the `/MK` slot
/// stays absent from the annotation dict (matching the spec's
/// optional treatment).
fn write_mk_dictionary(
    annotation: &mut pdf_writer::writers::Annotation,
    pushbutton_caption: Option<&str>,
    pushbutton_icon_refs: WidgetIconRefs,
    appearance: Option<&AppearanceCharacteristics>,
) {
    let any_appearance_entry = appearance.is_some_and(|mk| !mk.is_empty());
    if pushbutton_caption.is_none() && pushbutton_icon_refs.is_empty() && !any_appearance_entry {
        return;
    }

    let mut mk = annotation.insert(Name(b"MK")).dict();
    if let Some(caption) = pushbutton_caption {
        mk.pair(Name(b"CA"), TextStr(caption));
    }
    if let Some(icon_ref) = pushbutton_icon_refs.normal {
        mk.pair(Name(b"I"), icon_ref);
    }
    if let Some(icon_ref) = pushbutton_icon_refs.rollover {
        mk.pair(Name(b"RI"), icon_ref);
    }
    if let Some(icon_ref) = pushbutton_icon_refs.alternate {
        mk.pair(Name(b"IX"), icon_ref);
    }
    if let Some(mk_extras) = appearance {
        if let Some(colour) = &mk_extras.border_colour {
            write_mk_colour_entry(&mut mk, Name(b"BC"), colour);
        }
        if let Some(colour) = &mk_extras.background_colour {
            write_mk_colour_entry(&mut mk, Name(b"BG"), colour);
        }
        if let Some(rotation) = mk_extras.rotation {
            mk.pair(Name(b"R"), rotation.degrees());
        }
        if let Some(rc) = &mk_extras.rollover_caption {
            mk.pair(Name(b"RC"), TextStr(rc));
        }
        if let Some(ac) = &mk_extras.down_caption {
            mk.pair(Name(b"AC"), TextStr(ac));
        }
        if let Some(icon_fit) = mk_extras.icon_fit {
            write_mk_icon_fit(&mut mk, &icon_fit);
        }
        if let Some(tp) = mk_extras.text_position {
            mk.pair(Name(b"TP"), tp.to_pdf_integer());
        }
    }
    mk.finish();
}

/// Write the `/IF` (Icon Fit) sub-dictionary inside the `/MK` dict
/// per ISO 32000-2 §12.5.6.19 Table 187. Every entry has a spec
/// default; krilla emits each entry verbatim so round-trip embedders
/// can observe the chosen values without inferring them from
/// omission.
fn write_mk_icon_fit(mk: &mut pdf_writer::Dict, icon_fit: &IconFit) {
    let mut sub = mk.insert(Name(b"IF")).dict();
    sub.pair(Name(b"SW"), Name(icon_fit.scale_when.to_pdf_name()));
    sub.pair(Name(b"S"), Name(icon_fit.scale_type.to_pdf_name()));
    // `/A` is a two-element array of percentages clamped to [0.0, 1.0].
    {
        let mut align = sub.insert(Name(b"A")).array();
        align.item(icon_fit.align_x.clamp(0.0, 1.0));
        align.item(icon_fit.align_y.clamp(0.0, 1.0));
        align.finish();
    }
    sub.pair(Name(b"FB"), icon_fit.fit_bounds);
    sub.finish();
}

/// Write one `/BC` or `/BG` colour entry inside the `/MK` dictionary
/// per ISO 32000-2 §12.5.6.19 Table 167.
///
/// The PDF spec scopes `/MK` colour entries to device-space arrays —
/// 0, 1, 3, or 4 numeric components selecting "no colour" /
/// DeviceGray / DeviceRGB / DeviceCMYK respectively. krilla projects
/// the supplied [`Color`] onto the matching array length:
/// [`RegularColor::Luma`] -> one-component DeviceGray;
/// [`RegularColor::Rgb`] -> three-component DeviceRGB;
/// [`RegularColor::Cmyk`] -> four-component DeviceCMYK;
/// [`RegularColor::IccBased`] -> three-component DeviceRGB (the
/// authored components verbatim — `/MK` is device-space only and
/// ICC profiles cannot ride along the entry). Special colours
/// (Separation, DeviceN) fall back to a single 0.0 entry (a
/// well-formed "no colour" array) because they have no device-space
/// representation suitable for an unannotated number array; the
/// embedder is expected to author a device-space border / background
/// when this matters.
fn write_mk_colour_entry(
    mk: &mut pdf_writer::Dict,
    key: Name<'static>,
    colour: &Color,
) {
    let mut array = mk.insert(key).array();
    match colour {
        Color::Regular(regular) => match regular {
            RegularColor::Luma(luma) => {
                array.item(luma.to_pdf_color());
            }
            RegularColor::Rgb(rgb) => {
                for component in rgb.to_pdf_color() {
                    array.item(component);
                }
            }
            RegularColor::Cmyk(cmyk) => {
                for component in cmyk.to_pdf_color() {
                    array.item(component);
                }
            }
            RegularColor::IccBased { components, .. } => {
                for &component in components {
                    array.item(component);
                }
            }
        },
        Color::Special(_) => {
            // Separation / DeviceN have no device-space scalar form
            // that survives an unannotated number array. Emit an
            // empty-style "no colour" placeholder so the entry stays
            // well-formed; embedders that require Separation /
            // DeviceN borders should switch to a Form XObject `/N`
            // appearance instead.
            // Use a single-component zero so the array has a
            // well-defined PDF interpretation (DeviceGray 0 = black).
            array.item(0.0_f32);
        }
    }
    array.finish();
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

/// Write `/AP << /N << /<export> <on_ref> /Off <off_ref> >> >>` into a
/// radio-group child widget annotation. The "on" sub-state name must
/// match the child's `/AS` when selected so viewers can flip
/// appearance based on the parent's `/V`.
fn write_ap_on_off_with_state(
    annotation: &mut pdf_writer::writers::Annotation,
    on_state: &str,
    on_ref: Ref,
    off_ref: Ref,
) {
    let mut ap = annotation.insert(Name(b"AP")).dict();
    let mut n = ap.insert(Name(b"N")).dict();
    n.pair(Name(on_state.as_bytes()), on_ref);
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

/// Build an empty signature appearance — no visible glyphs, just an
/// empty marked-content stream. PDF viewers fall back to viewer-default
/// rendering for an unsigned signature field; this keeps the AP form
/// XObject well-formed so the viewer does not synthesise a placeholder.
fn build_empty_signature_content() -> Vec<u8> {
    Vec::new()
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
        // PDF 32000-2 §12.5.6.5 `/C` entries on annotations are
        // device-space only (1, 3 or 4 components — DeviceGray /
        // DeviceRGB / DeviceCMYK). An ICC-based wide-gamut paint
        // has no ICC engine inside krilla, so the source components
        // are written verbatim as a DeviceRGB triple. Authoring a
        // wide-gamut paint on an annotation surface is a niche edge
        // case (annotation appearance streams handle gamut more
        // precisely than the `/C` colour entry).
        crate::color::RegularColor::IccBased { components, .. } => {
            annotation.color_rgb(components[0], components[1], components[2]);
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
    fn widget_annotation_emits_tu_tooltip_when_set() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "email", text)
            .with_tooltip("E-mail address");
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(
            contains(&pdf, b"/TU (E-mail address)"),
            "missing /TU tooltip"
        );
    }

    #[test]
    fn widget_annotation_omits_tu_when_unset() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "email", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(!contains(&pdf, b"/TU"), "/TU emitted when tooltip unset");
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
    fn widget_annotation_text_required_sets_flag_bit_2() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default().with_required(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "req", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Required = bit 2 = 2
        assert!(contains(&pdf, b"/Ff 2"), "missing required /Ff bit");
    }

    #[test]
    fn widget_annotation_text_no_export_sets_flag_bit_3() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default().with_no_export(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "nx", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // NoExport = bit 3 = 4
        assert!(contains(&pdf, b"/Ff 4"), "missing no-export /Ff bit");
    }

    #[test]
    fn widget_annotation_text_file_select_sets_flag_bit_21() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default().with_file_select(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "upload", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // FileSelect = bit 21 = 1048576
        assert!(
            contains(&pdf, b"/Ff 1048576"),
            "missing file-select /Ff bit"
        );
    }

    #[test]
    fn widget_annotation_text_do_not_spell_check_sets_flag_bit_23() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default().with_do_not_spell_check(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "nsc", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // DoNotSpellCheck = bit 23 = 4194304
        assert!(
            contains(&pdf, b"/Ff 4194304"),
            "missing do-not-spell-check /Ff bit"
        );
    }

    #[test]
    fn widget_annotation_text_do_not_scroll_sets_flag_bit_24() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default().with_do_not_scroll(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "noscroll", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // DoNotScroll = bit 24 = 8388608
        assert!(
            contains(&pdf, b"/Ff 8388608"),
            "missing do-not-scroll /Ff bit"
        );
    }

    #[test]
    fn widget_annotation_text_comb_sets_flag_bit_25() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: Some(8),
            flags: TextFieldFlags::default().with_comb(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "comb", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Comb = bit 25 = 16777216
        assert!(contains(&pdf, b"/Ff 16777216"), "missing comb /Ff bit");
        assert!(contains(&pdf, b"/MaxLen 8"), "missing /MaxLen for comb");
    }

    #[test]
    fn widget_annotation_text_rich_text_sets_flag_bit_26() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default().with_rich_text(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "rt", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // RichText = bit 26 = 33554432
        assert!(
            contains(&pdf, b"/Ff 33554432"),
            "missing rich-text /Ff bit"
        );
    }

    #[test]
    fn widget_annotation_button_required_sets_flag_bit_2() {
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::Checkbox,
            caption: String::new(),
            flags: ButtonFieldFlags::default().with_required(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "agree", button);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(contains(&pdf, b"/Ff 2"), "missing required /Ff bit");
    }

    #[test]
    fn widget_annotation_button_no_export_sets_flag_bit_3() {
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::Checkbox,
            caption: String::new(),
            flags: ButtonFieldFlags::default().with_no_export(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "agree", button);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        assert!(contains(&pdf, b"/Ff 4"), "missing no-export /Ff bit");
    }

    #[test]
    fn widget_annotation_choice_required_sets_flag_bit_2() {
        let choice = WidgetField::Choice(ChoiceField {
            values: Vec::new(),
            default_values: Vec::new(),
            options: vec![("US".into(), "United States".into())],
            flags: ChoiceFieldFlags::default()
                .with_combo(true)
                .with_required(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "country", choice);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Combo (bit 18 = 131072) + Required (bit 2 = 2) = 131074
        assert!(contains(&pdf, b"/Ff 131074"), "missing required+combo /Ff");
    }

    #[test]
    fn widget_annotation_choice_no_export_sets_flag_bit_3() {
        let choice = WidgetField::Choice(ChoiceField {
            values: Vec::new(),
            default_values: Vec::new(),
            options: vec![("US".into(), "United States".into())],
            flags: ChoiceFieldFlags::default()
                .with_combo(true)
                .with_no_export(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "country", choice);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // Combo (131072) + NoExport (4) = 131076
        assert!(contains(&pdf, b"/Ff 131076"), "missing no-export+combo /Ff");
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

    fn empty_text_widget(partial: &str) -> WidgetAnnotation {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        WidgetAnnotation::new(widget_rect(), partial, text)
    }

    #[test]
    fn widget_annotation_no_actions_omits_aa_dict() {
        let widget = empty_text_widget("plain");
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(!contains(&pdf, b"/AA"), "/AA emitted on widget with no actions");
    }

    #[test]
    fn widget_annotation_keystroke_action_emits_aa_k_javascript() {
        let widget = empty_text_widget("dob").with_keystroke_action(Action::JavaScript(
            JavaScriptAction::new("AFDate_KeystrokeEx(\"yyyy-mm-dd\");"),
        ));
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(contains(&pdf, b"/AA"), "missing /AA dict");
        assert!(contains(&pdf, b"/K <<"), "missing /AA /K key");
        assert!(contains(&pdf, b"/S /JavaScript"), "missing /S /JavaScript");
        assert!(
            contains(&pdf, b"AFDate_KeystrokeEx"),
            "missing JS body"
        );
    }

    #[test]
    fn widget_annotation_format_action_emits_aa_f_javascript() {
        let widget = empty_text_widget("dob").with_format_action(Action::JavaScript(
            JavaScriptAction::new("AFDate_FormatEx(\"yyyy-mm-dd\");"),
        ));
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(contains(&pdf, b"/AA"), "missing /AA dict");
        assert!(contains(&pdf, b"/F <<"), "missing /AA /F key");
        assert!(contains(&pdf, b"AFDate_FormatEx"), "missing JS body");
    }

    #[test]
    fn widget_annotation_validate_action_emits_aa_v_javascript() {
        let widget = empty_text_widget("score").with_validate_action(Action::JavaScript(
            JavaScriptAction::new("AFRange_Validate(true, 0, true, 100);"),
        ));
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(contains(&pdf, b"/AA"), "missing /AA dict");
        assert!(contains(&pdf, b"/V <<"), "missing /AA /V key");
        assert!(contains(&pdf, b"AFRange_Validate"), "missing JS body");
    }

    #[test]
    fn widget_annotation_all_three_actions_emit_three_keys() {
        let widget = empty_text_widget("dob")
            .with_keystroke_action(Action::JavaScript(JavaScriptAction::new(
                "AFDate_KeystrokeEx(\"yyyy-mm-dd\");",
            )))
            .with_format_action(Action::JavaScript(JavaScriptAction::new(
                "AFDate_FormatEx(\"yyyy-mm-dd\");",
            )))
            .with_validate_action(Action::JavaScript(JavaScriptAction::new(
                "/* validate stub */",
            )));
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(contains(&pdf, b"/AA"), "missing /AA dict");
        assert!(contains(&pdf, b"/K <<"), "missing /K");
        assert!(contains(&pdf, b"/F <<"), "missing /F");
        assert!(contains(&pdf, b"/V <<"), "missing /V");
    }

    #[test]
    fn widget_annotation_radio_group_child_suppresses_aa() {
        // Radio-group children inherit /AA from the parent group per
        // ISO 32000-2 §12.7.4.1. The /AA emitter therefore skips the
        // slot on RadioGroupChild widgets even if a (mis-)configured
        // setter populated it.
        let group = RadioGroupField::new("preference", three_radio_children());
        let pdf = finish_with_radio_group(group);
        // No /AA on radio children — they have no setter wired
        // through `add_radio_group`, but the suppression branch should
        // still leave the document free of /AA dicts.
        assert!(!contains(&pdf, b"/AA"), "/AA leaked onto radio-group children");
    }

    #[test]
    fn document_with_no_widgets_does_not_emit_acroform() {
        let mut document = Document::new();
        let page = document.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
        page.finish();
        let pdf = document.finish().expect("document serialisation should succeed");
        assert!(!contains(&pdf, b"/AcroForm"));
    }

    fn three_radio_children() -> Vec<RadioChild> {
        vec![
            RadioChild {
                rect: Rect::from_xywh(10.0, 10.0, 12.0, 12.0).unwrap(),
                export_value: "yes".into(),
            },
            RadioChild {
                rect: Rect::from_xywh(40.0, 10.0, 12.0, 12.0).unwrap(),
                export_value: "no".into(),
            },
            RadioChild {
                rect: Rect::from_xywh(70.0, 10.0, 12.0, 12.0).unwrap(),
                export_value: "maybe".into(),
            },
        ]
    }

    fn finish_with_radio_group(group: RadioGroupField) -> Vec<u8> {
        let settings = crate::SerializeSettings {
            pretty: true,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);
        let mut page = document.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
        page.add_radio_group(group);
        page.finish();
        document.finish().expect("document serialisation should succeed")
    }

    #[test]
    fn widget_annotation_radio_group_emits_parent_with_kids() {
        // Three radios sharing a name, "yes" selected. The parent
        // dict carries /T (group_name), /V /yes, /Kids of length 3;
        // every child carries /Parent and /AS.
        let group = RadioGroupField::new("group_name", three_radio_children())
            .with_selected(Some("yes".into()));
        let pdf = finish_with_radio_group(group);

        assert!(contains(&pdf, b"/T (group_name)"), "missing parent /T");
        assert!(contains(&pdf, b"/V /yes"), "missing parent /V /yes");
        assert!(contains(&pdf, b"/Kids ["), "missing /Kids array opener");
        // Three child refs in the radio-group's `/Kids` array. The
        // PDF also contains a Pages-tree dictionary with its own
        // `/Kids` (listing the document's pages), so we cannot rely
        // on a naive "first `/Kids [`" match. We anchor on the group
        // parent dict via its `/T (group_name)` entry and scan for
        // the `/Kids` that follows.
        let parent_anchor = pdf
            .windows(b"/T (group_name)".len())
            .position(|w| w == b"/T (group_name)")
            .expect("missing /T (group_name) anchor");
        let after_parent = &pdf[parent_anchor..];
        let kids_offset = after_parent
            .windows(b"/Kids [".len())
            .position(|w| w == b"/Kids [")
            .expect("missing /Kids [ after parent /T");
        let after_kids = &after_parent[kids_offset..];
        let close = after_kids
            .iter()
            .position(|&b| b == b']')
            .expect("missing /Kids array closer");
        let kids_body = &after_kids[..close];
        let kid_ref_count = kids_body
            .windows(b" 0 R".len())
            .filter(|w| w == b" 0 R")
            .count();
        assert_eq!(
            kid_ref_count, 3,
            "expected 3 /Kids entries, got {kid_ref_count}; body={:?}",
            std::str::from_utf8(kids_body).unwrap_or("<non-utf8>"),
        );

        // Every child widget annotation carries /Parent and /AS. The
        // PDF contains three `/Subtype /Widget` annotations and a
        // page-tree `/Parent` entry on the page dictionary itself —
        // counting `/Subtype /Widget` is the cleanest way to assert
        // child cardinality without colliding with the page's own
        // parent pointer.
        let widget_count = pdf
            .windows(b"/Subtype /Widget".len())
            .filter(|w| w == b"/Subtype /Widget")
            .count();
        assert_eq!(
            widget_count, 3,
            "expected 3 child widget annotations, got {widget_count}",
        );
        assert!(contains(&pdf, b"/AS /yes"), "missing /AS /yes on selected child");
        // Two unselected children fall back to /Off.
        let off_as_count = pdf
            .windows(b"/AS /Off".len())
            .filter(|w| w == b"/AS /Off")
            .count();
        assert_eq!(off_as_count, 2, "expected 2 /AS /Off entries, got {off_as_count}");
    }

    #[test]
    fn widget_annotation_radio_group_no_selection_uses_off() {
        // No child checked. Parent's /V is /Off and every child's
        // /AS is /Off.
        let group = RadioGroupField::new("preference", three_radio_children());
        let pdf = finish_with_radio_group(group);

        assert!(contains(&pdf, b"/V /Off"), "missing parent /V /Off");
        let off_as_count = pdf
            .windows(b"/AS /Off".len())
            .filter(|w| w == b"/AS /Off")
            .count();
        assert_eq!(
            off_as_count, 3,
            "expected every child to fall back to /AS /Off; got {off_as_count}",
        );
        // None of the children should emit /AS /<export>.
        assert!(!contains(&pdf, b"/AS /yes"));
        assert!(!contains(&pdf, b"/AS /no"));
        assert!(!contains(&pdf, b"/AS /maybe"));
    }

    #[test]
    fn widget_annotation_radio_group_only_parent_in_fields() {
        // The /AcroForm /Fields array must contain the parent ref
        // exactly once and NO child refs. The parent ref is allocated
        // before the children, so it has the lowest numbered ref in
        // the group; the four refs that follow are the three children
        // and (lazily) the Helvetica font.
        let group = RadioGroupField::new("group", three_radio_children())
            .with_selected(Some("no".into()));
        let pdf = finish_with_radio_group(group);

        // Locate the /Fields array.
        let fields_pos = pdf
            .windows(b"/Fields [".len())
            .position(|w| w == b"/Fields [")
            .expect("missing /Fields [");
        let after = &pdf[fields_pos..];
        let close = after
            .iter()
            .position(|&b| b == b']')
            .expect("missing /Fields array closer");
        let fields_body = &after[..close];
        let fields_ref_count = fields_body
            .windows(b" R".len())
            .filter(|w| w == b" R")
            .count();
        assert_eq!(
            fields_ref_count, 1,
            "/AcroForm /Fields must contain exactly one ref (the radio-group parent), got {fields_ref_count}",
        );
    }

    #[test]
    fn widget_annotation_radio_group_ff_radio_bit_set() {
        // The parent /Ff integer must have bit 15 (0x8000 = 32768)
        // set and bit 16 (0x10000 = 65536, Pushbutton) clear.
        let group = RadioGroupField::new("g", three_radio_children())
            .with_selected(Some("yes".into()));
        let pdf = finish_with_radio_group(group);

        // The parent dict is the only one carrying /T (group_name)
        // and /Ff together; the children have neither. So /Ff 32768
        // is unambiguous.
        assert!(
            contains(&pdf, b"/Ff 32768"),
            "expected /Ff 32768 (Radio flag only), got missing entry",
        );
        // Pushbutton (bit 17 = 65536) and combinations of both must
        // not appear on the parent.
        assert!(
            !contains(&pdf, b"/Ff 65536"),
            "pushbutton flag must not be set on a radio group",
        );
        assert!(
            !contains(&pdf, b"/Ff 98304"),
            "pushbutton+radio combination must not be set on a radio group",
        );
    }

    // -------------------------------------------------------------------
    // moegoe T1-D4 — `/MK` Appearance Characteristics
    // (`/BC`, `/BG`, `/R`, `/RC`, `/AC`) per ISO 32000-2 §12.5.6.19
    // Table 167 and §12.7.4.3.
    // -------------------------------------------------------------------

    fn text_widget_with_appearance(mk: AppearanceCharacteristics) -> Vec<u8> {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "field", text)
            .with_appearance_characteristics(mk);
        finish_with(Annotation::new_widget(widget, None))
    }

    #[test]
    fn widget_mk_border_colour_devicergb_emits_three_component_array() {
        let mk = AppearanceCharacteristics {
            border_colour: Some(Color::from(rgb::Color::new(255, 0, 0))),
            ..Default::default()
        };
        let pdf = text_widget_with_appearance(mk);
        assert!(contains(&pdf, b"/MK <<"), "missing /MK dictionary opener");
        // /BC carries [r g b] in [0.0, 1.0] — `255` -> `1` (printed
        // without a trailing decimal by pdf-writer).
        assert!(
            contains(&pdf, b"/BC [1 0 0]"),
            "expected /BC [1 0 0] DeviceRGB array, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
    }

    #[test]
    fn widget_mk_background_colour_devicegray_emits_single_component_array() {
        use crate::color::luma;
        let mk = AppearanceCharacteristics {
            background_colour: Some(Color::from(luma::Color::new(0))),
            ..Default::default()
        };
        let pdf = text_widget_with_appearance(mk);
        assert!(contains(&pdf, b"/MK <<"), "missing /MK dictionary opener");
        // /BG carries [g] in [0.0, 1.0] for DeviceGray.
        assert!(
            contains(&pdf, b"/BG [0]"),
            "expected /BG [0] DeviceGray array, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
    }

    #[test]
    fn widget_mk_background_colour_devicecmyk_emits_four_component_array() {
        use crate::color::cmyk;
        let mk = AppearanceCharacteristics {
            background_colour: Some(Color::from(cmyk::Color::new(255, 0, 0, 0))),
            ..Default::default()
        };
        let pdf = text_widget_with_appearance(mk);
        assert!(contains(&pdf, b"/MK <<"), "missing /MK dictionary opener");
        // /BG carries [c m y k] in [0.0, 1.0] for DeviceCMYK.
        assert!(
            contains(&pdf, b"/BG [1 0 0 0]"),
            "expected /BG [1 0 0 0] DeviceCMYK array, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
    }

    #[test]
    fn widget_mk_rotation_quarter_emits_r_90() {
        let mk = AppearanceCharacteristics {
            rotation: Some(Rotation::Quarter),
            ..Default::default()
        };
        let pdf = text_widget_with_appearance(mk);
        assert!(contains(&pdf, b"/MK <<"), "missing /MK dictionary opener");
        assert!(
            contains(&pdf, b"/R 90"),
            "expected /R 90 rotation entry, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
    }

    #[test]
    fn widget_mk_rotation_half_emits_r_180() {
        let mk = AppearanceCharacteristics {
            rotation: Some(Rotation::Half),
            ..Default::default()
        };
        let pdf = text_widget_with_appearance(mk);
        assert!(
            contains(&pdf, b"/R 180"),
            "expected /R 180 rotation entry, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
    }

    #[test]
    fn widget_mk_rotation_three_quarter_emits_r_270() {
        let mk = AppearanceCharacteristics {
            rotation: Some(Rotation::ThreeQuarter),
            ..Default::default()
        };
        let pdf = text_widget_with_appearance(mk);
        assert!(
            contains(&pdf, b"/R 270"),
            "expected /R 270 rotation entry, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
    }

    #[test]
    fn widget_mk_rollover_caption_emits_rc_string() {
        // /RC is meaningful for pushbutton widgets per Table 167.
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::PushButton,
            caption: "Go".into(),
            flags: ButtonFieldFlags::default().with_pushbutton(true),
        });
        let mk = AppearanceCharacteristics {
            rollover_caption: Some("Hover".into()),
            ..Default::default()
        };
        let widget = WidgetAnnotation::new(widget_rect(), "submit", button)
            .with_appearance_characteristics(mk);
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(contains(&pdf, b"/MK <<"), "missing /MK dictionary opener");
        assert!(
            contains(&pdf, b"/RC (Hover)"),
            "expected /RC (Hover) rollover caption, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
        // The existing /CA (caption) and the new /RC must coexist in
        // the same /MK dict so the pushbutton path's contribution
        // composes with the appearance-characteristics path.
        assert!(
            contains(&pdf, b"/CA (Go)"),
            "/CA must still emit alongside /RC"
        );
    }

    #[test]
    fn widget_mk_down_caption_emits_ac_string() {
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::PushButton,
            caption: "Go".into(),
            flags: ButtonFieldFlags::default().with_pushbutton(true),
        });
        let mk = AppearanceCharacteristics {
            down_caption: Some("Pressed".into()),
            ..Default::default()
        };
        let widget = WidgetAnnotation::new(widget_rect(), "submit", button)
            .with_appearance_characteristics(mk);
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(
            contains(&pdf, b"/AC (Pressed)"),
            "expected /AC (Pressed) down caption, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
    }

    #[test]
    fn widget_without_appearance_characteristics_omits_mk_on_non_pushbutton() {
        // The /MK extension is opt-in; a text widget with no
        // appearance characteristics must not emit /MK (PushButton
        // is the only field type that emits /MK via the caption /
        // icon path).
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "no-mk", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(
            !contains(&pdf, b"/MK <<"),
            "/MK must be absent when no appearance characteristics are set"
        );
    }

    #[test]
    fn widget_mk_icon_fit_proportional_emits_if_dict() {
        let mk = AppearanceCharacteristics {
            icon_fit: Some(IconFit {
                scale_when: ScaleWhen::ContentBiggerThanRect,
                scale_type: ScaleType::Proportional,
                align_x: 0.25,
                align_y: 0.75,
                fit_bounds: true,
            }),
            ..Default::default()
        };
        let pdf = text_widget_with_appearance(mk);
        assert!(contains(&pdf, b"/MK <<"), "missing /MK dictionary opener");
        assert!(
            contains(&pdf, b"/IF <<"),
            "expected /IF sub-dictionary, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
        // /SW selects when to scale.
        assert!(contains(&pdf, b"/SW /B"), "expected /SW /B inside /IF");
        // /S selects how to scale.
        assert!(contains(&pdf, b"/S /P"), "expected /S /P inside /IF");
        // /A array carries [align_x align_y]. pdf-writer pretty-prints
        // floats without trailing zeros; assert the substring.
        assert!(
            contains(&pdf, b"/A [0.25 0.75]"),
            "expected /A [0.25 0.75] inside /IF, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
        assert!(contains(&pdf, b"/FB true"), "expected /FB true inside /IF");
    }

    #[test]
    fn widget_mk_text_position_caption_below_icon_emits_tp_2() {
        let mk = AppearanceCharacteristics {
            text_position: Some(TextPosition::CaptionBelowIcon),
            ..Default::default()
        };
        let pdf = text_widget_with_appearance(mk);
        assert!(
            contains(&pdf, b"/TP 2"),
            "expected /TP 2 inside /MK, body was {:?}",
            mk_dictionary_slice(&pdf)
        );
    }

    #[test]
    fn widget_mk_text_position_full_range_round_trips() {
        // Walk through every TextPosition keyword and assert the
        // matching integer reaches the /MK dict. The match arm in
        // `TextPosition::to_pdf_integer` is the only mapping under
        // test; this guards against future renumbering breaking the
        // public surface.
        let cases: &[(TextPosition, &[u8])] = &[
            (TextPosition::CaptionOnly, b"/TP 0"),
            (TextPosition::IconOnly, b"/TP 1"),
            (TextPosition::CaptionBelowIcon, b"/TP 2"),
            (TextPosition::CaptionAboveIcon, b"/TP 3"),
            (TextPosition::CaptionRightOfIcon, b"/TP 4"),
            (TextPosition::CaptionLeftOfIcon, b"/TP 5"),
            (TextPosition::CaptionOverlaidOnIcon, b"/TP 6"),
        ];
        for (tp, needle) in cases {
            let mk = AppearanceCharacteristics {
                text_position: Some(*tp),
                ..Default::default()
            };
            let pdf = text_widget_with_appearance(mk);
            assert!(
                contains(&pdf, needle),
                "expected {:?} for {:?}",
                std::str::from_utf8(needle).unwrap_or("<non-utf8>"),
                tp
            );
        }
    }

    #[cfg(feature = "raster-images")]
    #[test]
    fn widget_mk_rollover_icon_emits_ri_ref_and_form_xobject() {
        use crate::graphics::image::Image;

        let image = Image::from_rgba8(vec![255, 0, 0, 255], 1, 1);
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::PushButton,
            caption: String::new(),
            flags: ButtonFieldFlags::default().with_pushbutton(true),
        });
        let mk = AppearanceCharacteristics {
            rollover_icon: Some(image),
            ..Default::default()
        };
        let widget = WidgetAnnotation::new(widget_rect(), "btn", button)
            .with_appearance_characteristics(mk);
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(contains(&pdf, b"/MK <<"), "missing /MK dictionary opener");
        // /RI <n> 0 R — the indirect reference token.
        let mk_pos = pdf
            .windows(b"/MK <<".len())
            .position(|w| w == b"/MK <<")
            .expect("/MK dict not found");
        let mk_tail = &pdf[mk_pos..];
        let ri_in_mk = mk_tail
            .windows(b"/RI ".len())
            .position(|w| w == b"/RI ")
            .expect("missing /RI entry inside /MK dict");
        let after_ri = &mk_tail[ri_in_mk + b"/RI ".len()..];
        assert!(
            after_ri.iter().take_while(|b| b.is_ascii_digit()).count() > 0,
            "/RI must be followed by a numeric ref id"
        );
        // The rollover-icon Form XObject draws the image as /Im0.
        assert!(
            contains(&pdf, b"/Im0 Do"),
            "missing image draw in rollover-icon Form XObject content stream"
        );
    }

    #[cfg(feature = "raster-images")]
    #[test]
    fn widget_mk_alternate_icon_emits_ix_ref_and_form_xobject() {
        use crate::graphics::image::Image;

        let image = Image::from_rgba8(vec![0, 255, 0, 255], 1, 1);
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::PushButton,
            caption: String::new(),
            flags: ButtonFieldFlags::default().with_pushbutton(true),
        });
        let mk = AppearanceCharacteristics {
            alternate_icon: Some(image),
            ..Default::default()
        };
        let widget = WidgetAnnotation::new(widget_rect(), "btn", button)
            .with_appearance_characteristics(mk);
        let pdf = finish_with(Annotation::new_widget(widget, None));
        let mk_pos = pdf
            .windows(b"/MK <<".len())
            .position(|w| w == b"/MK <<")
            .expect("/MK dict not found");
        let mk_tail = &pdf[mk_pos..];
        let ix_in_mk = mk_tail
            .windows(b"/IX ".len())
            .position(|w| w == b"/IX ")
            .expect("missing /IX entry inside /MK dict");
        let after_ix = &mk_tail[ix_in_mk + b"/IX ".len()..];
        assert!(
            after_ix.iter().take_while(|b| b.is_ascii_digit()).count() > 0,
            "/IX must be followed by a numeric ref id"
        );
        assert!(
            contains(&pdf, b"/Im0 Do"),
            "missing image draw in alternate-icon Form XObject content stream"
        );
    }

    #[cfg(feature = "raster-images")]
    #[test]
    fn widget_mk_three_icon_states_emit_distinct_refs() {
        use crate::graphics::image::Image;

        let normal_image = Image::from_rgba8(vec![255, 0, 0, 255], 1, 1);
        let rollover_image = Image::from_rgba8(vec![0, 255, 0, 255], 1, 1);
        let alternate_image = Image::from_rgba8(vec![0, 0, 255, 255], 1, 1);
        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::PushButton,
            caption: String::new(),
            flags: ButtonFieldFlags::default().with_pushbutton(true),
        });
        let mk = AppearanceCharacteristics {
            rollover_icon: Some(rollover_image),
            alternate_icon: Some(alternate_image),
            ..Default::default()
        };
        let widget = WidgetAnnotation::new(widget_rect(), "btn", button)
            .with_icon_appearance(normal_image)
            .with_appearance_characteristics(mk);
        let pdf = finish_with(Annotation::new_widget(widget, None));
        // All three icon entries must appear in the /MK dict.
        let mk_pos = pdf
            .windows(b"/MK <<".len())
            .position(|w| w == b"/MK <<")
            .expect("/MK dict not found");
        let mk_tail = &pdf[mk_pos..];
        assert!(
            mk_tail.windows(b"/I ".len()).any(|w| w == b"/I "),
            "missing /I entry"
        );
        assert!(
            mk_tail.windows(b"/RI ".len()).any(|w| w == b"/RI "),
            "missing /RI entry"
        );
        assert!(
            mk_tail.windows(b"/IX ".len()).any(|w| w == b"/IX "),
            "missing /IX entry"
        );
    }

    /// Slice the bytes from the `/MK <<` opener to the matching `>>`
    /// so test failure messages show the dictionary body. The
    /// pretty-printed PDF emitted under `pretty: true` keeps each
    /// entry on its own line; the slice is bounded to the next
    /// `>>` token to avoid pulling in the rest of the annotation
    /// dictionary.
    fn mk_dictionary_slice(pdf: &[u8]) -> String {
        let Some(start) = pdf
            .windows(b"/MK <<".len())
            .position(|w| w == b"/MK <<")
        else {
            return "<no /MK dict>".to_string();
        };
        let tail = &pdf[start..];
        let end = tail
            .windows(2)
            .position(|w| w == b">>")
            .map(|p| p + 2)
            .unwrap_or(tail.len().min(256));
        String::from_utf8_lossy(&tail[..end]).into_owned()
    }

    // -------------------------------------------------------------------
    // moegoe G26 — fork-extension setters: `/H`, `/StructParent`,
    // `/DA` override, `/MK /I`.
    // -------------------------------------------------------------------

    #[test]
    fn link_annotation_with_highlight_invert_emits_h_i() {
        let link = LinkAnnotation::new(
            Rect::from_xywh(10.0, 20.0, 80.0, 16.0).unwrap(),
            Target::Destination(crate::interactive::destination::Destination::Xyz(
                crate::interactive::destination::XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
            )),
        )
        .with_highlight(LinkHighlight::Invert);
        let pdf = finish_with(Annotation::new_link(link, Some("link".into())));
        assert!(contains(&pdf, b"/H /I"), "missing /H /I");
    }

    #[test]
    fn link_annotation_with_highlight_push_emits_h_p() {
        let link = LinkAnnotation::new(
            Rect::from_xywh(10.0, 20.0, 80.0, 16.0).unwrap(),
            Target::Destination(crate::interactive::destination::Destination::Xyz(
                crate::interactive::destination::XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
            )),
        )
        .with_highlight(LinkHighlight::Push);
        let pdf = finish_with(Annotation::new_link(link, Some("link".into())));
        assert!(contains(&pdf, b"/H /P"), "missing /H /P");
    }

    #[test]
    fn link_annotation_without_highlight_omits_h() {
        let link = LinkAnnotation::new(
            Rect::from_xywh(10.0, 20.0, 80.0, 16.0).unwrap(),
            Target::Destination(crate::interactive::destination::Destination::Xyz(
                crate::interactive::destination::XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
            )),
        );
        let pdf = finish_with(Annotation::new_link(link, Some("link".into())));
        assert!(!contains(&pdf, b"/H /"), "unexpected /H entry on link without highlight");
    }

    #[test]
    fn annotation_with_struct_parent_emits_structparent_entry() {
        let text = TextAnnotation::new(Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap());
        let annotation = Annotation::new_text(text, Some("alt".into())).with_struct_parent(7);
        let pdf = finish_with(annotation);
        assert!(
            contains(&pdf, b"/StructParent 7"),
            "missing /StructParent 7"
        );
    }

    #[test]
    fn widget_text_with_default_appearance_overrides_helvetica() {
        let text = WidgetField::Text(TextField {
            value: "deadbeef".into(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "hex", text)
            .with_default_appearance("/Courier 10 Tf 0 g");
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(
            contains(&pdf, b"/DA (/Courier 10 Tf 0 g)"),
            "missing overridden /DA"
        );
        assert!(
            !contains(&pdf, b"/DA (/Helv 10 Tf 0 g)"),
            "default Helvetica /DA must not appear when override is set"
        );
    }

    #[test]
    fn widget_text_without_default_appearance_falls_back_to_helvetica() {
        let text = WidgetField::Text(TextField {
            value: String::new(),
            default_value: String::new(),
            max_length: None,
            flags: TextFieldFlags::default(),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "untitled", text);
        let pdf = finish_with(Annotation::new_widget(widget, None));
        assert!(
            contains(&pdf, b"/DA (/Helv 10 Tf 0 g)"),
            "missing default Helvetica /DA"
        );
    }

    #[cfg(feature = "raster-images")]
    #[test]
    fn widget_pushbutton_with_icon_appearance_emits_mk_i() {
        use crate::graphics::image::Image;

        // Construct a 2x2 RGBA image directly from raw pixels — avoids
        // taking a hard dependency on a PNG fixture file. Four 8-bit
        // RGBA samples (red, green, blue, transparent) give a
        // deterministic test image.
        let image = Image::from_rgba8(
            vec![
                255, 0, 0, 255, // red
                0, 255, 0, 255, // green
                0, 0, 255, 255, // blue
                0, 0, 0, 0, // transparent
            ],
            2,
            2,
        );

        let button = WidgetField::Button(ButtonField {
            checked: false,
            kind: ButtonKind::PushButton,
            caption: "Go".into(),
            flags: ButtonFieldFlags::default().with_pushbutton(true),
        });
        let widget = WidgetAnnotation::new(widget_rect(), "submit", button)
            .with_icon_appearance(image);
        let pdf = finish_with(Annotation::new_widget(widget, None));

        // /MK dict carries both /CA caption and /I icon ref.
        assert!(contains(&pdf, b"/MK <<"), "missing /MK dictionary opener");
        assert!(contains(&pdf, b"/CA (Go)"), "missing /CA caption inside /MK");
        // /I <n> 0 R — the indirect reference token. We assert the
        // `/I ` substring followed by digits + ` 0 R`.
        let mk_pos = pdf
            .windows(b"/MK <<".len())
            .position(|w| w == b"/MK <<")
            .expect("/MK dict not found");
        let mk_tail = &pdf[mk_pos..];
        let i_in_mk = mk_tail
            .windows(b"/I ".len())
            .position(|w| w == b"/I ")
            .expect("missing /I entry inside /MK dict");
        let after_i = &mk_tail[i_in_mk + b"/I ".len()..];
        assert!(
            after_i.iter().take_while(|b| b.is_ascii_digit()).count() > 0,
            "/I must be followed by a numeric ref id"
        );
        // The icon Form XObject's content stream draws the image as
        // /Im0; assert the content-stream marker is present somewhere
        // in the document.
        assert!(
            contains(&pdf, b"/Im0 Do"),
            "missing image draw in icon Form XObject content stream"
        );
    }

    // -------------------------------------------------------------------
    // moegoe K11 — `FileAttachment` annotation per ISO 32000-2 §12.5.6.15.
    // -------------------------------------------------------------------

    fn sample_embedded_file(path: &str) -> EmbeddedFile {
        use crate::interchange::embed::{AssociationKind, MimeType};
        use crate::metadata::DateTime;
        EmbeddedFile {
            path: path.into(),
            mime_type: MimeType::new("application/octet-stream"),
            description: Some("payload".into()),
            association_kind: AssociationKind::Supplement,
            data: crate::Data::from(vec![0x42_u8, 0x4f, 0x4d, 0x42]),
            modification_date: Some(DateTime::new(2026)),
            compress: Some(false),
            location: None,
            embed_location: crate::interchange::embed::EmbedLocation::Before,
        }
    }

    fn file_attachment_pdf(icon: FileAttachmentIcon, path: &str) -> Vec<u8> {
        let annotation = FileAttachmentAnnotation::new(
            Rect::from_xywh(10.0, 20.0, 30.0, 30.0).unwrap(),
            sample_embedded_file(path),
            icon,
        )
        .with_contents("payload description");
        finish_with(Annotation::new_file_attachment(
            annotation,
            Some("attachment alt".into()),
        ))
    }

    #[test]
    fn file_attachment_pushpin_emits_subtype_name_and_fs() {
        let pdf = file_attachment_pdf(FileAttachmentIcon::PushPin, "payload.bin");
        assert!(
            contains(&pdf, b"/Subtype /FileAttachment"),
            "missing /Subtype /FileAttachment"
        );
        assert!(contains(&pdf, b"/Name /PushPin"), "missing /Name /PushPin");
        // The annotation's /FS entry resolves to the FileSpec dict's
        // indirect ref. The FileSpec carries the file's `/F` path.
        assert!(contains(&pdf, b"/FS "), "missing /FS indirect reference");
        assert!(
            contains(&pdf, b"(payload.bin)"),
            "missing FileSpec /F path entry"
        );
    }

    #[test]
    fn file_attachment_paperclip_emits_paperclip_name() {
        let pdf = file_attachment_pdf(FileAttachmentIcon::Paperclip, "clip.bin");
        assert!(contains(&pdf, b"/Subtype /FileAttachment"));
        assert!(
            contains(&pdf, b"/Name /Paperclip"),
            "missing /Name /Paperclip"
        );
    }

    #[test]
    fn file_attachment_graph_emits_graph_name() {
        let pdf = file_attachment_pdf(FileAttachmentIcon::Graph, "chart.bin");
        assert!(contains(&pdf, b"/Subtype /FileAttachment"));
        assert!(contains(&pdf, b"/Name /Graph"), "missing /Name /Graph");
    }

    #[test]
    fn file_attachment_tag_emits_tag_name() {
        let pdf = file_attachment_pdf(FileAttachmentIcon::Tag, "tag.bin");
        assert!(contains(&pdf, b"/Subtype /FileAttachment"));
        assert!(contains(&pdf, b"/Name /Tag"), "missing /Name /Tag");
    }

    #[test]
    fn file_attachment_default_icon_is_pushpin() {
        // Default constructor on the enum is PushPin per ISO 32000-2
        // §12.5.6.15 default behaviour.
        let icon = FileAttachmentIcon::default();
        assert!(matches!(icon, FileAttachmentIcon::PushPin));
    }

    #[test]
    fn file_attachment_from_trait_wraps_without_alt() {
        let annotation = FileAttachmentAnnotation::new(
            Rect::from_xywh(0.0, 0.0, 5.0, 5.0).unwrap(),
            sample_embedded_file("bare.bin"),
            FileAttachmentIcon::PushPin,
        );
        let wrapped: Annotation = annotation.into();
        assert!(matches!(
            wrapped.annotation_type,
            AnnotationType::FileAttachment(_)
        ));
        assert!(wrapped.alt.is_none());
    }

    #[test]
    fn file_attachment_two_annotations_same_file_share_filespec() {
        // Two FileAttachment annotations pointing at byte-identical
        // EmbeddedFiles must dedupe onto a single FileSpec indirect
        // object. The annotations still get distinct indirect refs;
        // their /FS entries name the same FileSpec.
        let file = sample_embedded_file("shared.bin");
        let a1 = FileAttachmentAnnotation::new(
            Rect::from_xywh(10.0, 10.0, 20.0, 20.0).unwrap(),
            file.clone(),
            FileAttachmentIcon::PushPin,
        );
        let a2 = FileAttachmentAnnotation::new(
            Rect::from_xywh(50.0, 50.0, 20.0, 20.0).unwrap(),
            file,
            FileAttachmentIcon::Paperclip,
        );

        let settings = crate::SerializeSettings {
            pretty: true,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);
        let mut page =
            document.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
        page.add_annotation(Annotation::new_file_attachment(a1, Some("alt".into())));
        page.add_annotation(Annotation::new_file_attachment(a2, Some("alt".into())));
        page.finish();
        let pdf = document
            .finish()
            .expect("document serialisation should succeed");

        // Print the PDF (for debug) and inspect.
        if std::env::var_os("KRILLA_DUMP_PDF").is_some() {
            eprintln!(
                "PDF bytes: {}",
                String::from_utf8_lossy(&pdf)
            );
        }

        // The FileSpec dict is registered through `register_cacheable`,
        // which dedupes by content hash. We assert that:
        //
        // 1. The PDF contains exactly one `/Type /Filespec` dictionary.
        // 2. Both annotations exist (PushPin + Paperclip icons).
        //
        // We do NOT assert on the path literal occurrence count: the
        // FileSpec emits the path twice (`/F (path)` plus `/UF
        // <textstr>` under PDF 1.7+ which encodes to (path) too); the
        // metric that matters is the FileSpec object count.
        let filespec_count = pdf
            .windows(b"/Type /Filespec".len())
            .filter(|w| w == b"/Type /Filespec")
            .count();
        assert_eq!(
            filespec_count, 1,
            "FileSpec should be deduplicated to a single indirect object; \
             got {filespec_count} /Type /Filespec dictionaries"
        );

        // Both annotations are present (Paperclip + PushPin /Name
        // entries).
        assert!(contains(&pdf, b"/Name /PushPin"));
        assert!(contains(&pdf, b"/Name /Paperclip"));
    }

    #[test]
    fn file_attachment_contents_field_emitted_when_no_alt() {
        // When the embedder does not provide an outer alt-text, the
        // inner `with_contents(...)` value survives as the annotation's
        // /Contents entry. (When alt is set, the outer write wins —
        // verified by the other tests which set both and observe alt
        // in the /Contents slot.)
        let annotation = FileAttachmentAnnotation::new(
            Rect::from_xywh(0.0, 0.0, 5.0, 5.0).unwrap(),
            sample_embedded_file("notes.bin"),
            FileAttachmentIcon::Tag,
        )
        .with_contents("inner description");
        let pdf = finish_with(Annotation::new_file_attachment(annotation, None));
        assert!(
            contains(&pdf, b"(inner description)"),
            "inner /Contents must survive when no outer alt-text is set"
        );
    }
}
