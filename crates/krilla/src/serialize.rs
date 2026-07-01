use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroU16;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;
use pdf_writer::types::{OutputIntentSubtype, StructRole, StructRole2};
use pdf_writer::writers::{FileSpec, OutputIntent, StructTreeRoot};
use pdf_writer::{Chunk, Content, Finish, Limits, Name, Pdf, Ref, Settings, Str, TextStr};

use crate::chunk_container::ChunkContainer;
use crate::color::{CieBasedColorSpace, DeviceColorSpace, SpecialColorSpace};
use crate::configure::validate::ValidationStore;
use crate::configure::{Configuration, PdfVersion, ValidationError, Validators};
use crate::error::{KrillaError, KrillaResult, LimitError};
use crate::geom::Size;
use crate::graphics::color::{rgb, ColorSpace, ColourConversion};
use crate::graphics::devicen::DeviceNColorSpace;
use crate::graphics::icc::{GenericICCProfile, ICCBasedColorSpace, ICCColorSpace, ICCProfile};
#[cfg(feature = "raster-images")]
use crate::graphics::image::Image;
use crate::graphics::separation::SeparationColorSpace;
use crate::interactive::destination::{NamedDestination, XyzDestination};
use crate::interchange::embed::EmbeddedFile;
use crate::interchange::outline::Outline;
use crate::interchange::tagging::{AnnotationIdentifier, PageTagIdentifier, TagTree};
use crate::page::{InternalPage, PageLabel, PageLabelContainer};
#[cfg(feature = "pdf")]
use crate::pdf::{PdfDocument, PdfSerializerContext};
use crate::resource;
use crate::resource::{Resource, Resourceable};
use crate::surface::{Location, Surface};
use crate::text::GlyphId;
use crate::text::{Font, FontContainer, FontIdentifier};
use crate::util::SipHashable;

const STR_LEN: usize = 32767;
const NAME_LEN: usize = 127;

// These only apply to PDF 1.4 and PDF/A-1.
const MAX_FLOAT: f32 = 32767.0;
const DICT_LEN: usize = 4095;
const ARRAY_LEN: usize = 8191;

/// Settings that should be applied when creating a PDF document.
#[derive(Clone, Debug)]
pub struct SerializeSettings {
    /// Whether to write PDFs in a way that is easier to inspect manually. This
    /// will result in larger file sizes.
    pub pretty: bool,
    /// Whether content streams should be compressed. Leads to significantly smaller file sizes,
    /// but also longer running times. It is highly recommended that you set this to `true`.
    pub compress_content_streams: bool,
    /// Whether device-independent colors should be used instead of
    /// device-dependent ones.
    ///
    /// Note that this value might be overridden depending on which validator
    /// you use. For example, when exporting to PDF/A, this value will be set to
    /// true, regardless of what value will be passed.
    pub no_device_cs: bool,
    /// Whether the PDF should be ASCII-compatible, i.e. only consist of
    /// characters in the ASCII range.
    ///
    /// Note that this only on a best-effort basis. For example, XMP metadata always
    /// contains a binary marker. In addition to that, some validators,
    /// like PDF/A, require that the file header be a binary marker, meaning
    /// that the header itself will not be ASCII-compatible. Finally, embedded PDFs will
    /// be embedded as is and not re-encoded with ASCII-compatible encoding.
    pub ascii_compatible: bool,
    /// Whether the PDF should include XMP metadata.
    ///
    /// Note that this value might be overridden depending on which validator
    /// you use. For example, when exporting to PDF/A, this value will be set to
    /// true, regardless of what value will be passed.
    pub xmp_metadata: bool,
    /// The ICC profile that should be used for CMYK colors
    /// when `no_device_cs` is enabled.
    ///
    /// This is usually not required, but it is for example required when exporting
    /// to PDF/A and using a CMYK color, since they have to be device-independent.
    ///
    /// For PDF/X variants that embed their output intent (PDF/X-1a, PDF/X-3,
    /// PDF/X-4, PDF/X-6), this profile is also used as the embedded
    /// printer/output profile for the PDF/X output intent.
    pub cmyk_profile: Option<ICCProfile<4>>,
    /// A validator and PDF version used for export.
    ///
    /// In case validation fails, export will fail, and a list of validation errors that
    /// occurred will be returned instead of the PDF.
    ///
    /// **Important**: Make sure to carefully read the documentation of the [`validate`] module
    /// before using this feature! Just setting a validator might not be enough to ensure that
    /// your output conforms to the given standard, as some requirements are semantic in nature
    /// and cannot possibly be verified by krilla!
    ///
    /// However, as long as you carefully read and follow the documentation,
    /// you can be certain that the resulting document will conform to the standard (unless there
    /// is a bug).
    ///
    /// [`validate`]: crate::configure::validate
    pub configuration: Configuration,
    /// Whether to enable the creation of tagged documents. See the module documentation
    /// of [`tagging`] for more information about tagged PDF documents.
    ///
    /// Note that enabling this does not automatically make your documents tagged, as tagging implies
    /// enriching the document with semantic information, which krilla cannot do
    /// for you, since it's content-agnostic. All this setting does is to allow you
    /// to dynamically disable tagging if you wish to do so. This allows you to write
    /// your code primarily with tagging in mind, but still allows you to
    /// disable it dynamically, without having to make any changes to your code.
    ///
    /// Note that this value might be overridden depending on which validator
    /// you use. For example, when exporting with PDF/UA, this value will always
    /// be set to `true`.
    ///
    /// [`tagging`]: crate::interchange::tagging
    pub enable_tagging: bool,
    /// A function that should be used to render SVG glyphs. If you don't need this, yu can
    /// just use the default function which doesn't render them at all. If you do want this, it
    /// is recommended that you use the function provided by the `krilla-svg` crate.
    pub render_svg_glyph_fn: RenderSvgGlyphFn,
    /// An external ICC profile reference used by PDF/X-4p and PDF/X-6p.
    ///
    /// This setting is required when exporting with [`Prepress::X4P`] or
    /// [`Prepress::X6P`]. In those modes, the PDF/X output intent references
    /// the ICC profile externally instead of embedding it in the PDF.
    ///
    /// Supplying this setting when no PDF/X-4p or PDF/X-6p validator is active
    /// is rejected during validation.
    ///
    /// [`Prepress::X4P`]: crate::configure::Prepress::X4P
    /// [`Prepress::X6P`]: crate::configure::Prepress::X6P
    pub external_output_profile: Option<ExternalOutputProfile>,

    /// Caller-supplied output intents, emitted in the catalogue's
    /// `/OutputIntents` array in addition to any intents generated by the
    /// active validator.
    ///
    /// Authors use this to declare the destination colour space of the
    /// document for colour-managed workflows when no PDF/A validator is
    /// active, or to add an additional intent alongside one. Per
    /// ISO 32000-2 §14.11.5, multiple output intents are allowed.
    ///
    /// krilla does not enforce cross-intent agreement — that is the
    /// caller's responsibility.
    pub output_intents: Vec<CustomOutputIntent>,
    /// Fallback CMYK destination profile, emitted as a default
    /// `/OutputIntents` entry when the document declares no other intent.
    ///
    /// This is the colour-managed fallback for documents that contain CMYK
    /// content but neither activate a validator (PDF/A, PDF/X) that
    /// generates its own output intent, nor supply caller-driven entries
    /// via [`output_intents`]. When set under those conditions, krilla
    /// emits a single `/Type /OutputIntent` dictionary with
    /// `/S /GTS_PDFX` referencing the supplied ICC profile, so colour-
    /// managed consumers can transform device CMYK content to the
    /// destination space.
    ///
    /// When any of the following is true, this setting is ignored:
    ///
    /// - The active validator (via [`configuration`]) already produces
    ///   an output intent (PDF/A or PDF/X variants).
    /// - [`output_intents`] is non-empty.
    ///
    /// The default is `None`, preserving existing behaviour.
    ///
    /// This is distinct from [`cmyk_profile`], which is consulted only by
    /// `no_device_cs` mode and the PDF/X embedded-output-intent variants.
    /// Setting [`cmyk_profile`] does not emit an output intent on its own.
    ///
    /// [`output_intents`]: SerializeSettings::output_intents
    /// [`cmyk_profile`]: SerializeSettings::cmyk_profile
    /// [`configuration`]: SerializeSettings::configuration
    pub fallback_cmyk_profile: Option<ICCProfile<4>>,
    /// How text drawn through [`Surface::draw_glyphs`] and
    /// [`Surface::draw_text`] should be emitted into the content stream.
    ///
    /// [`TextRendering::Glyphs`] (the default) preserves searchable,
    /// selectable, copy-pasteable text by writing `Tj`-family text-showing
    /// operators. [`TextRendering::Vector`] emits the same glyphs as
    /// filled vector outlines (`m`/`l`/`c`/`h`/`f`), which is required by
    /// some print workflows (e.g. PDF/X embedders that cannot rely on the
    /// consumer to rasterise the embedded fonts) at the cost of text
    /// extraction.
    ///
    /// Per-call `outlined: true` arguments to [`Surface::draw_glyphs`]
    /// still force outline emission even when this setting is
    /// [`TextRendering::Glyphs`]; the setting is therefore a one-way
    /// global override that promotes all text to vector mode but never
    /// downgrades a caller's per-call request.
    ///
    /// [`Surface::draw_glyphs`]: crate::surface::Surface::draw_glyphs
    /// [`Surface::draw_text`]: crate::surface::Surface::draw_text
    pub text_rendering: TextRendering,
    /// How embedded font programmes are written into the PDF.
    ///
    /// [`FontEmbedding::Subset`] (the default) keeps krilla's existing
    /// behaviour: each CID font is reduced to the set of glyphs actually
    /// referenced by the document and that subset is written as the
    /// `/FontFile2` (or `/FontFile3`) stream on the font descriptor.
    ///
    /// [`FontEmbedding::Full`] skips the subsetter and embeds the
    /// unmodified font programme. This is useful in workflows where the
    /// PDF will be re-edited downstream (a subset would make later
    /// glyph access fail) or for licensed fonts that explicitly permit
    /// full embedding.
    ///
    /// [`FontEmbedding::None`] omits the font programme entirely. The
    /// font descriptor is written without `/FontFile2` or `/FontFile3`,
    /// so consumers must resolve the glyph data from a host-installed
    /// font matching the descriptor name. This is permitted by
    /// ISO 32000-2 §9.9 but produces a fragile PDF — PDF/A and PDF/UA
    /// forbid it. It is the caller's responsibility to ensure the
    /// active validator (if any) tolerates the choice.
    ///
    /// Note that this setting only affects CID font emission. Type3
    /// bitmap fonts (used for colour-emoji glyphs) never carry an
    /// embedded `/FontFile*` programme to begin with, so this setting
    /// is a no-op for them.
    pub font_embedding: FontEmbedding,
    /// How glyph positions are emitted into the PDF content stream.
    ///
    /// [`GlyphLayout::Optical`] (the default) preserves krilla's
    /// existing behaviour: every glyph run is written via a `TJ`-style
    /// positioned-show array (`encode_glyphs_with_individual_positioning`),
    /// which encodes per-glyph `x_offset` adjustments and reconciles
    /// caller-supplied advances against the font's intrinsic advances.
    /// This is the quality mode -- it preserves kerning and any
    /// per-character placement the shaper produced.
    ///
    /// [`GlyphLayout::Metric`] short-circuits the positioned-show path
    /// and emits glyph runs as a single `Tj` string per consecutive
    /// run, relying on the font's intrinsic advance widths for
    /// inter-glyph spacing. The content stream is smaller (no `[ ... ]
    /// TJ` array with per-glyph numeric adjustments) but kerning and
    /// any `x_offset` the shaper supplied are discarded. This is the
    /// speed/size mode -- mirrors PDFreactor's `glyph-layout: metric`.
    ///
    /// The setting only governs whether krilla writes a `TJ` array or
    /// a plain `Tj` string for runs of two or more glyphs. Single-glyph
    /// runs without an `x_offset` always use `Tj` regardless (this
    /// predates the setting and is unrelated to it).
    pub glyph_layout: GlyphLayout,
    /// How regular colours should be projected before being written
    /// to the content stream.
    ///
    /// [`ColourConversion::Auto`] (the default) preserves the
    /// existing krilla behaviour: every colour is emitted in its
    /// source space (RGB, CMYK, Luma, or Separation). The
    /// `Force*` variants project regular colours into the requested
    /// target space using ISO 32000-2 §8.6.4 (RGB <-> CMYK) and
    /// Rec. 709 (RGB -> Y) at every fill, stroke, and glyph paint
    /// dispatch in `crate::content`.
    ///
    /// `ContentOnly` and `ForceSpot` are reserved for Phase 3 of
    /// the moegoe `colour_conversion` work and are currently
    /// pass-throughs at this layer; see [`ColourConversion`] for
    /// the variant-by-variant contract.
    ///
    /// Single-stop gradients that route through the solid-fill path
    /// at `content.rs` are also projected; multi-stop gradients are
    /// **not** projected because doing so would alter interpolation.
    pub colour_conversion: ColourConversion,
    /// How aggressively path geometry should be simplified before
    /// being written to the PDF content stream.
    ///
    /// [`ShapeOptimisation::Auto`] is the default and preserves
    /// krilla's existing behaviour: every path segment supplied to
    /// the surface is written verbatim into the content stream.
    /// [`ShapeOptimisation::None`] disables every form of path
    /// simplification (it is currently equivalent to `Auto` because
    /// krilla does not simplify paths, but the contract is that no
    /// simplification will ever be applied under this mode).
    /// [`ShapeOptimisation::Full`] permits krilla to apply the most
    /// aggressive path simplification it can without changing the
    /// rendered appearance of the page.
    ///
    /// Krilla does not currently perform any path simplification, so
    /// this setting is a no-op at the content-stream level. It exists
    /// so consumers (notably moegoe's `-bd-pdf-shape-optimisation`
    /// cascade) can carry an authored value through to the serialiser
    /// without losing it; a real simplification pass is future work.
    pub shape_optimisation: ShapeOptimisation,
    /// Promote RGB greys to `/DeviceGray` at paint dispatch.
    ///
    /// When `true`, every solid RGB colour whose channels are equal
    /// (`r == g == b`) is reclassified as a Luma colour before colour-
    /// space selection, so the content stream emits a `g` (DeviceGray)
    /// operator instead of `rg`. Implements ISO 32000-2 §8.6.4 by
    /// choosing the narrowest device space that represents the source
    /// value exactly.
    ///
    /// Maps to moegoe's `-bd-pdf-colour-options: use-true-black`
    /// (Prince warrant): print workflows that route greyscale content
    /// through `/DeviceGray` avoid an unnecessary three-channel
    /// representation and the slight ink-laydown asymmetry that comes
    /// with it.
    ///
    /// The default is `false`, preserving the source colour space
    /// exactly (existing behaviour).
    ///
    /// This setting composes with [`colour_conversion`]: projection
    /// runs first, then `r == g == b` promotion is applied to the
    /// projected value. A `ForceRgb` policy with this flag therefore
    /// still produces `/DeviceGray` for greyscale inputs.
    ///
    /// [`colour_conversion`]: SerializeSettings::colour_conversion
    pub rgb_grey_to_devicegray: bool,
    /// Bypass the ICC reclassification path for pure black at paint
    /// dispatch.
    ///
    /// When `true`, solid paints sourced from `rgb(0, 0, 0)` or
    /// `device-cmyk(0, 0, 0, 1)` are emitted in their device space
    /// (`DeviceRGB` / `DeviceCMYK`) regardless of [`no_device_cs`].
    /// This short-circuits the per-paint sRGB / cmyk-profile routing
    /// that would otherwise replace pure black with the ICC-transformed
    /// equivalent — which, through a fallback CMYK profile, can become
    /// a near-black mixed value rather than the intended single-channel
    /// black.
    ///
    /// Maps to moegoe's `-bd-pdf-colour-options: preserve-black`
    /// (a moegoe extension): authors who set this flag are asserting
    /// that pure black must remain device-black in print, even when
    /// the rest of the document is colour-managed.
    ///
    /// The default is `false`, preserving existing behaviour.
    ///
    /// **Validator interaction.** PDF/A and PDF/X variants force
    /// [`no_device_cs`] to `true` and forbid device colour spaces in
    /// many content positions; combining `preserve_black` with such a
    /// validator may cause emission of a device-space colour that the
    /// validator subsequently rejects. The flag is intended for
    /// non-validated, print-oriented workflows.
    ///
    /// [`no_device_cs`]: SerializeSettings::no_device_cs
    pub preserve_black: bool,
    /// Encrypt the document with AES-256 (Standard Security Handler
    /// V=5, R=6 — ISO 32000-2 §7.6.4 "AESV3"). When `Some`, krilla
    /// applies the configuration to the underlying `Pdf` before any
    /// indirect object is written, so every string and stream the
    /// document subsequently emits is encrypted under the document's
    /// file key. The `/Encrypt` dict, the trailer `/ID` strings, and
    /// (when [`crate::encryption::Encryption::with_encrypt_metadata`] is `false`) the
    /// metadata stream remain plaintext per the spec.
    ///
    /// Compatibility note: the AESV3 cipher suite was introduced by
    /// PDF 2.0; most modern readers (Acrobat 9+, MuPDF, pdf.js) accept
    /// it on PDF 1.7 documents as well, but readers limited to PDF
    /// 1.6 or older will refuse to open the file.
    ///
    /// Default is `None` (no encryption).
    ///
    /// [`Encryption`]: crate::encryption::Encryption
    pub encryption: Option<crate::encryption::Encryption>,
    /// Write the file's cross-reference information as a
    /// `/Type /XRef` stream (ISO 32000-1 §7.5.8 / 32000-2 §7.5.8)
    /// instead of the traditional plain `xref` table.
    ///
    /// Cross-reference streams allow the xref to be compressed
    /// alongside the rest of the file body and are a prerequisite
    /// for any document that uses object streams (also ISO
    /// 32000-1 §7.5.7).
    ///
    /// **Version constraint.** Cross-reference streams require
    /// PDF 1.5 or later. krilla does not downgrade silently — if
    /// this flag is set while the active PDF version is below 1.5
    /// the resulting file will not be readable by PDF 1.4
    /// consumers. PDF/A-1 (PDF 1.4) callers must keep this
    /// `false`.
    ///
    /// The default is `false`, preserving the traditional
    /// `xref` + `trailer` layout.
    pub xref_streams: bool,
}

/// How embedded font programmes are written into the PDF.
///
/// See [`SerializeSettings::font_embedding`] for the full contract.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum FontEmbedding {
    /// Embed only the glyphs the document references.
    ///
    /// Each CID font is run through the subsetter and only the
    /// referenced glyphs are written as the `/FontFile2` (or
    /// `/FontFile3`) stream. This is the default and what every PDF/A
    /// and PDF/UA validator expects.
    #[default]
    Subset,
    /// Embed the full font programme without subsetting.
    ///
    /// The original font data (as supplied to [`Font::new`]) is written
    /// verbatim as the `/FontFile2` (or `/FontFile3`) stream. Useful
    /// for downstream editing workflows and for licensed fonts whose
    /// licence requires full embedding.
    ///
    /// [`Font::new`]: crate::text::Font::new
    Full,
    /// Do not embed the font programme.
    ///
    /// The font descriptor is written without a `/FontFile2` or
    /// `/FontFile3` entry; consumers must resolve the glyph data from
    /// a host-installed font matching the descriptor name. This is
    /// fragile and incompatible with PDF/A and PDF/UA — callers must
    /// audit their validator configuration.
    None,
}

/// How glyph positions are emitted into the PDF content stream.
///
/// See [`SerializeSettings::glyph_layout`] for the full contract.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum GlyphLayout {
    /// Per-glyph individual positioning with kerning preserved.
    ///
    /// Glyph runs are written via a `TJ` positioned-show array. Each
    /// per-glyph `x_offset` and any discrepancy between the caller's
    /// supplied advance and the font's intrinsic advance is encoded as
    /// a numeric adjustment in the array. This is the default and
    /// produces the highest-quality output at the cost of a larger
    /// content stream.
    #[default]
    Optical,
    /// Advance-width-only glyph emission.
    ///
    /// Multi-glyph runs are written as a single `Tj` string and the
    /// consumer is expected to lay out the glyphs using the font's
    /// intrinsic advances. Per-glyph `x_offset` adjustments supplied
    /// by the caller are discarded; the content stream is smaller but
    /// kerning may degrade.
    Metric,
}

/// How aggressively path geometry should be simplified before being
/// written to the PDF content stream.
///
/// See [`SerializeSettings::shape_optimisation`] for the full
/// contract. Krilla does not currently apply any path simplification,
/// so all three variants behave identically at the content-stream
/// level; the enum exists so callers can plumb an authored value
/// through to the serialiser without losing it.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ShapeOptimisation {
    /// Let krilla decide whether to simplify path geometry.
    ///
    /// This is the default. Krilla currently writes every path
    /// segment verbatim into the content stream; that behaviour is
    /// not guaranteed by this contract and may change once a
    /// simplification pass is added.
    #[default]
    Auto,
    /// Never simplify path geometry.
    ///
    /// Every supplied path segment is written verbatim into the
    /// content stream. The contract is that no path simplification
    /// will ever be applied under this mode, regardless of what
    /// future heuristics `Auto` may grow.
    None,
    /// Apply the most aggressive path simplification krilla can
    /// without changing the rendered appearance of the page.
    ///
    /// Reserved for a future simplification pass; equivalent to
    /// `Auto` today.
    Full,
}

/// How text should be emitted into the PDF content stream.
///
/// Selects between glyph-based text-showing operators
/// ([`TextRendering::Glyphs`]) and vector-outline emission
/// ([`TextRendering::Vector`]). See
/// [`SerializeSettings::text_rendering`] for the full contract.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum TextRendering {
    /// Emit text as PDF text-showing operators (`Tj`, `TJ`, etc.).
    ///
    /// Glyphs are referenced by CID into embedded fonts, preserving
    /// searchable, copy-pasteable text. This is the default.
    #[default]
    Glyphs,
    /// Emit text as filled vector paths (`m`, `l`, `c`, `h`, `f`).
    ///
    /// Glyph outlines are extracted from the font (via the OpenType
    /// `glyf`/`CFF`/`CFF2` tables) and stroked into the content stream
    /// as path operators. Text is no longer selectable or searchable
    /// after this transformation, but the output is independent of the
    /// consumer's ability to render the embedded fonts.
    Vector,
    /// Emit text using PDF text rendering mode 3 (Invisible).
    ///
    /// Glyphs are shaped, positioned, and CID-mapped exactly as in
    /// [`TextRendering::Glyphs`] mode so consumers can still extract
    /// the text via copy/paste, search, screen readers, and
    /// `/ActualText` overrides. The text-rendering-mode operator
    /// `3 Tr` is emitted before the showing operator so the glyphs
    /// produce no marks on the page. No fill or stroke colour is
    /// set in the content stream — the glyphs are never painted.
    ///
    /// This is distinct from drawing with a fully transparent fill
    /// (`rgba(_, _, _, 0)`): a transparent fill still issues a paint
    /// operation (which may interact with overprint, blend modes,
    /// and tagged-PDF structure), whereas mode 3 instructs the
    /// consumer not to paint the glyph at all.
    ///
    /// Per ISO 32000-2 §9.3.6 Table 105 (text rendering modes) and
    /// §14.9.4 (`/ActualText`).
    Invisible,
}

pub type RenderSvgGlyphFn = fn(&[u8], rgb::Color, GlyphId, (f32, f32), &mut Surface) -> Option<()>;

/// A reference to an externally hosted output profile for PDF/X-4p and
/// PDF/X-6p.
///
/// Construction validates the required fields eagerly; the type guarantees by
/// construction that at least one non-empty URL, a non-empty output condition
/// identifier, and a non-empty informational string are present.
#[derive(Clone, Debug)]
pub struct ExternalOutputProfile {
    urls: Vec<String>,
    profile: GenericICCProfile,
    output_condition_identifier: String,
    output_condition: Option<String>,
    registry_name: Option<String>,
    info: String,
}

/// Reason construction of an [`ExternalOutputProfile`] failed.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ExternalOutputProfileError {
    /// The `urls` vector was empty or contained only empty/whitespace strings.
    EmptyUrls,
    /// The output condition identifier was empty or only whitespace.
    EmptyIdentifier,
    /// The information string was empty or only whitespace.
    EmptyInfo,
    /// The profile's ICC data colour space is not the one implied by the
    /// constructor — i.e. not `GRAY` for [`ExternalOutputProfile::luma`], `RGB `
    /// for [`ExternalOutputProfile::rgb`], or `CMYK` for
    /// [`ExternalOutputProfile::cmyk`]. A PDF/X output-intent profile must have a
    /// `GRAY`/`RGB `/`CMYK` data colour space (ISO 15930-7 §6.4.1, Annex A.2);
    /// a same-channel-count profile with a different signature (e.g. `Lab `,
    /// `1CLR`, `4CLR`/DeviceN) is rejected.
    WrongColorSpace,
}

impl core::fmt::Display for ExternalOutputProfileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let reason = match self {
            ExternalOutputProfileError::EmptyUrls => "at least one non-empty URL must be provided",
            ExternalOutputProfileError::EmptyIdentifier => {
                "the output condition identifier must be non-empty"
            }
            ExternalOutputProfileError::EmptyInfo => "the informational string must be non-empty",
            ExternalOutputProfileError::WrongColorSpace => {
                "the profile's data colour space must be GRAY, RGB or CMYK and match the constructor"
            }
        };
        f.write_str(reason)
    }
}

impl std::error::Error for ExternalOutputProfileError {}

impl ExternalOutputProfile {
    /// Create an external RGB output profile reference.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalOutputProfileError::EmptyUrls`],
    /// [`ExternalOutputProfileError::EmptyIdentifier`], or
    /// [`ExternalOutputProfileError::EmptyInfo`] if any of `urls`,
    /// `output_condition_identifier`, or `info` is empty (or contains only
    /// whitespace) after trimming; or [`ExternalOutputProfileError::WrongColorSpace`]
    /// if the profile's ICC data colour space is not the one implied by the
    /// constructor (`GRAY` for [`luma`](Self::luma), `RGB ` for
    /// [`rgb`](Self::rgb), `CMYK` for [`cmyk`](Self::cmyk)).
    pub fn rgb(
        profile: ICCProfile<3>,
        urls: Vec<String>,
        output_condition_identifier: String,
        info: String,
    ) -> Result<Self, ExternalOutputProfileError> {
        Self::new(
            GenericICCProfile::Rgb(profile),
            urls,
            output_condition_identifier,
            info,
        )
    }

    /// Create an external grayscale output profile reference.
    ///
    /// # Errors
    ///
    /// See [`ExternalOutputProfile::rgb`].
    pub fn luma(
        profile: ICCProfile<1>,
        urls: Vec<String>,
        output_condition_identifier: String,
        info: String,
    ) -> Result<Self, ExternalOutputProfileError> {
        Self::new(
            GenericICCProfile::Luma(profile),
            urls,
            output_condition_identifier,
            info,
        )
    }

    /// Create an external CMYK output profile reference.
    ///
    /// # Errors
    ///
    /// See [`ExternalOutputProfile::rgb`].
    pub fn cmyk(
        profile: ICCProfile<4>,
        urls: Vec<String>,
        output_condition_identifier: String,
        info: String,
    ) -> Result<Self, ExternalOutputProfileError> {
        Self::new(
            GenericICCProfile::Cmyk(profile),
            urls,
            output_condition_identifier,
            info,
        )
    }

    fn new(
        profile: GenericICCProfile,
        urls: Vec<String>,
        output_condition_identifier: String,
        info: String,
    ) -> Result<Self, ExternalOutputProfileError> {
        // ISO 15930-7 §6.4.1 / Annex A.2: a PDF/X output-intent profile shall
        // have a GRAY, RGB or CMYK data colour space. The typed constructors fix
        // the channel count, but a same-channel-count profile can still carry a
        // different signature (e.g. a 3-channel Lab profile), so verify it here.
        let expected = match &profile {
            GenericICCProfile::Luma(_) => ICCColorSpace::Gray,
            GenericICCProfile::Rgb(_) => ICCColorSpace::Rgb,
            GenericICCProfile::Cmyk(_) => ICCColorSpace::Cmyk,
        };
        if profile.metadata().color_space != expected {
            return Err(ExternalOutputProfileError::WrongColorSpace);
        }
        let urls = trim_url_list(urls).ok_or(ExternalOutputProfileError::EmptyUrls)?;
        let output_condition_identifier = trim_required(output_condition_identifier)
            .ok_or(ExternalOutputProfileError::EmptyIdentifier)?;
        let info = trim_required(info).ok_or(ExternalOutputProfileError::EmptyInfo)?;
        Ok(Self {
            urls,
            profile,
            output_condition_identifier,
            output_condition: None,
            registry_name: None,
            info,
        })
    }

    /// Set a human-readable output condition string. Empty or whitespace-only
    /// values are discarded.
    pub fn with_output_condition(mut self, output_condition: String) -> Self {
        self.output_condition = normalize_optional_string(output_condition);
        self
    }

    /// Set the registry name for the output condition identifier. Empty or
    /// whitespace-only values are discarded.
    pub fn with_registry_name(mut self, registry_name: String) -> Self {
        self.registry_name = normalize_optional_string(registry_name);
        self
    }

    /// Return the referenced profile URLs.
    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    /// Return the output condition identifier.
    pub fn output_condition_identifier(&self) -> &str {
        &self.output_condition_identifier
    }

    /// Return the optional human-readable output condition string.
    pub fn output_condition(&self) -> Option<&str> {
        self.output_condition.as_deref()
    }

    /// Return the optional registry name.
    pub fn registry_name(&self) -> Option<&str> {
        self.registry_name.as_deref()
    }

    /// Return the informational string for the output condition.
    pub fn info(&self) -> &str {
        &self.info
    }

    pub(crate) fn profile(&self) -> &GenericICCProfile {
        &self.profile
    }
}

fn normalize_optional_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn trim_required(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn trim_url_list(urls: Vec<String>) -> Option<Vec<String>> {
    let trimmed: Vec<String> = urls
        .into_iter()
        .filter_map(|url| {
            let t = url.trim();
            (!t.is_empty()).then(|| t.to_string())
        })
        .collect();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// Whether an ICC profile version is too new to be a PDF/X output-intent
/// profile for the given PDF version.
///
/// The PDF 1.4-based levels (PDF/X-1a, PDF/X-3) admit only ICC v2. The PDF
/// 1.6-based levels (PDF/X-4, PDF/X-4p) admit ICC v4 up to v4.2 (ISO 15930-7
/// §6.4.2.1, citing ISO 15076-1:2005). The PDF 2.0-based levels (PDF/X-6,
/// PDF/X-6p) admit ICC v4 up to v4.3 (ISO 15930-9, citing ISO 15076-1:2010).
fn output_profile_version_too_new(pdf_version: PdfVersion, major: u8, minor: u8) -> bool {
    match pdf_version {
        PdfVersion::Pdf14 => major > 2,
        PdfVersion::Pdf15 => major > 4,
        PdfVersion::Pdf16 | PdfVersion::Pdf17 => major > 4 || (major == 4 && minor > 2),
        PdfVersion::Pdf20 => major > 4 || (major == 4 && minor > 3),
    }
}

impl SerializeSettings {
    pub(crate) fn pdf_version(&self) -> PdfVersion {
        self.configuration.version()
    }

    pub(crate) fn validators(&self) -> Validators {
        self.configuration.validators()
    }

    /// Whether the `/AF` key is supported, accounting for the PDF version and active standards.
    pub(crate) fn supports_associated_files(&self) -> bool {
        self.configuration.version().specifies_associated_files()
            || self.configuration.validators().specifies_associated_files()
    }

    /// Whether the PDF/X (`GTS_PDFX`) output intent's profile is a CMYK device
    /// profile, which is required to characterize the DeviceCMYK content krilla
    /// emits under PDF/X. `None` if no output-target profile is configured; a
    /// missing profile is reported separately (`MissingCMYKProfile` /
    /// `MissingExternalOutputProfile`).
    pub(crate) fn pdfx_output_intent_is_cmyk(&self) -> Option<bool> {
        if self.validators().requires_external_output_profile() {
            // The wrapper variant now matches the ICC data colour space
            // (validated in `ExternalOutputProfile::new`), so an RGB or
            // grayscale external intent simply reports a non-CMYK colour space.
            self.external_output_profile
                .as_ref()
                .map(|p| p.profile().metadata().color_space == ICCColorSpace::Cmyk)
        } else {
            self.cmyk_profile
                .as_ref()
                .map(|p| p.metadata().color_space == ICCColorSpace::Cmyk)
        }
    }

    /// Whether the PDF/X (`GTS_PDFX`) output intent's profile is an RGB profile.
    /// Only the external (`-p`) variants can have an RGB output intent; the
    /// embedded variants always use the 4-channel `cmyk_profile`.
    pub(crate) fn pdfx_output_intent_is_rgb(&self) -> bool {
        self.validators().requires_external_output_profile()
            && self
                .external_output_profile
                .as_ref()
                .is_some_and(|p| p.profile().metadata().color_space == ICCColorSpace::Rgb)
    }
}

impl Default for SerializeSettings {
    fn default() -> Self {
        Self {
            pretty: false,
            ascii_compatible: false,
            compress_content_streams: true,
            no_device_cs: false,
            xmp_metadata: true,
            cmyk_profile: None,
            configuration: Configuration::default(),
            enable_tagging: true,
            render_svg_glyph_fn: |_, _, _, _, _| None,
            external_output_profile: None,
            output_intents: Vec::new(),
            text_rendering: TextRendering::Glyphs,
            font_embedding: FontEmbedding::Subset,
            glyph_layout: GlyphLayout::Optical,
            fallback_cmyk_profile: None,
            colour_conversion: ColourConversion::Auto,
            shape_optimisation: ShapeOptimisation::Auto,
            rgb_grey_to_devicegray: false,
            preserve_black: false,
            encryption: None,
            xref_streams: false,
        }
    }
}

fn normalise_optional_string(s: String) -> Option<String> {
    trim_required(s)
}

/// The `/S` (subtype) entry of an output intent dictionary, per
/// ISO 32000-2 §14.11.5.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum CustomOutputIntentSubtype {
    /// `GTS_PDFA1` — applicable to all PDF/A revisions.
    PdfA,
    /// `GTS_PDFX` — applicable to all PDF/X revisions.
    PdfX,
    /// `ISO_PDFE1` — PDF/E.
    PdfE,
    /// A custom subtype name defined by an ISO 32000 extension.
    ///
    /// The string is written verbatim as the `/S` name; callers are
    /// responsible for choosing an identifier that the consuming reader
    /// recognises.
    Custom(String),
}

/// An ICC destination output profile, accepted by [`CustomOutputIntent`].
///
/// The profile is embedded as a `/DestOutputProfile` stream in the
/// catalogue's `/OutputIntents` entry.
#[derive(Clone, Debug)]
pub enum OutputIntentProfile {
    /// A 1-channel (grayscale) ICC profile.
    Luma(ICCProfile<1>),
    /// A 3-channel (RGB) ICC profile.
    Rgb(ICCProfile<3>),
    /// A 4-channel (CMYK) ICC profile.
    Cmyk(ICCProfile<4>),
}

impl OutputIntentProfile {
    fn into_generic(self) -> GenericICCProfile {
        match self {
            OutputIntentProfile::Luma(p) => GenericICCProfile::Luma(p),
            OutputIntentProfile::Rgb(p) => GenericICCProfile::Rgb(p),
            OutputIntentProfile::Cmyk(p) => GenericICCProfile::Cmyk(p),
        }
    }
}

/// A caller-supplied entry for the catalogue's `/OutputIntents` array, per
/// ISO 32000-2 §14.11.5.
///
/// This is an additive surface that runs alongside any output intent
/// auto-generated by the active PDF/A validator. krilla emits
/// caller-supplied intents AFTER the validator-generated entries in the
/// catalogue's `/OutputIntents` array, in the order supplied.
///
/// Construction validates the required fields eagerly; the type guarantees
/// by construction that a non-empty output condition identifier and a
/// non-empty informational string are present.
#[derive(Clone, Debug)]
pub struct CustomOutputIntent {
    subtype: CustomOutputIntentSubtype,
    profile: GenericICCProfile,
    output_condition_identifier: String,
    info: String,
    output_condition: Option<String>,
    registry_name: Option<String>,
}

/// Reason construction of a [`CustomOutputIntent`] failed.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum CustomOutputIntentError {
    /// The output condition identifier was empty or only whitespace.
    EmptyIdentifier,
    /// The information string was empty or only whitespace.
    EmptyInfo,
    /// The custom subtype name was empty or only whitespace.
    EmptyCustomSubtype,
}

impl core::fmt::Display for CustomOutputIntentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let reason = match self {
            CustomOutputIntentError::EmptyIdentifier => {
                "the output condition identifier must be non-empty"
            }
            CustomOutputIntentError::EmptyInfo => "the informational string must be non-empty",
            CustomOutputIntentError::EmptyCustomSubtype => {
                "the custom subtype name must be non-empty"
            }
        };
        f.write_str(reason)
    }
}

impl std::error::Error for CustomOutputIntentError {}

impl CustomOutputIntent {
    /// Create a new caller-supplied output intent.
    ///
    /// # Errors
    ///
    /// Returns [`CustomOutputIntentError::EmptyIdentifier`] or
    /// [`CustomOutputIntentError::EmptyInfo`] if the corresponding string
    /// is empty (or whitespace-only) after trimming. Returns
    /// [`CustomOutputIntentError::EmptyCustomSubtype`] if `subtype` is
    /// [`CustomOutputIntentSubtype::Custom`] with an empty inner name.
    pub fn new(
        subtype: CustomOutputIntentSubtype,
        profile: OutputIntentProfile,
        output_condition_identifier: String,
        info: String,
    ) -> Result<Self, CustomOutputIntentError> {
        if let CustomOutputIntentSubtype::Custom(name) = &subtype {
            if name.trim().is_empty() {
                return Err(CustomOutputIntentError::EmptyCustomSubtype);
            }
        }
        let output_condition_identifier = trim_required(output_condition_identifier)
            .ok_or(CustomOutputIntentError::EmptyIdentifier)?;
        let info = trim_required(info).ok_or(CustomOutputIntentError::EmptyInfo)?;
        Ok(Self {
            subtype,
            profile: profile.into_generic(),
            output_condition_identifier,
            info,
            output_condition: None,
            registry_name: None,
        })
    }

    /// Set the optional `/OutputCondition` entry — a human-readable
    /// description of the output condition. Empty or whitespace-only
    /// values are discarded.
    pub fn with_output_condition(mut self, output_condition: String) -> Self {
        self.output_condition = normalise_optional_string(output_condition);
        self
    }

    /// Set the optional `/RegistryName` entry — the URI of the registry
    /// that contains the output condition identifier. Empty or
    /// whitespace-only values are discarded.
    pub fn with_registry_name(mut self, registry_name: String) -> Self {
        self.registry_name = normalise_optional_string(registry_name);
        self
    }

    /// Return the `/S` subtype.
    pub fn subtype(&self) -> &CustomOutputIntentSubtype {
        &self.subtype
    }

    /// Return the `/OutputConditionIdentifier` string.
    pub fn output_condition_identifier(&self) -> &str {
        &self.output_condition_identifier
    }

    /// Return the `/Info` string.
    pub fn info(&self) -> &str {
        &self.info
    }

    /// Return the optional `/OutputCondition` string.
    pub fn output_condition(&self) -> Option<&str> {
        self.output_condition.as_deref()
    }

    /// Return the optional `/RegistryName` string.
    pub fn registry_name(&self) -> Option<&str> {
        self.registry_name.as_deref()
    }

    pub(crate) fn profile(&self) -> &GenericICCProfile {
        &self.profile
    }

    pub(crate) fn pdf_writer_subtype(&self) -> pdf_writer::types::OutputIntentSubtype<'_> {
        use pdf_writer::types::OutputIntentSubtype;
        match &self.subtype {
            CustomOutputIntentSubtype::PdfA => OutputIntentSubtype::PDFA,
            CustomOutputIntentSubtype::PdfX => OutputIntentSubtype::PDFX,
            CustomOutputIntentSubtype::PdfE => OutputIntentSubtype::PDFE,
            CustomOutputIntentSubtype::Custom(name) => {
                OutputIntentSubtype::Custom(Name(name.as_bytes()))
            }
        }
    }
}

pub(crate) enum PageInfo {
    /// A page built with krilla.
    Krilla {
        /// The reference of the page in the chunk.
        ref_: Ref,
        /// The page size, necessary so that we can convert from PDF coordinates to
        /// krilla coordinates.
        surface_size: Size,
        /// The refs of the annotations that are used by that page, and optionally
        /// a ref to their struct parent in the tag tree.
        ///
        /// Note that this will be empty be default when adding a new `PageInfo` to
        /// `page_infos` in `SerializeContext`, and only once we actually serialize
        /// the page will the annotations be populated.
        annotations: Vec<(Ref, OnceCell<Ref>)>,
        /// The page label of the page.
        page_label: PageLabel,
    },
    /// A page embedded from an external PDF file.
    #[allow(dead_code)]
    Pdf {
        ref_: Ref,
        size: Size,
        page_label: PageLabel,
    },
}

impl PageInfo {
    pub(crate) fn ref_(&self) -> Ref {
        match self {
            PageInfo::Krilla { ref_, .. } => *ref_,
            PageInfo::Pdf { ref_, .. } => *ref_,
        }
    }

    pub(crate) fn size(&self) -> Size {
        match self {
            PageInfo::Krilla { surface_size, .. } => *surface_size,
            PageInfo::Pdf { size, .. } => *size,
        }
    }

    pub(crate) fn page_label(&self) -> &PageLabel {
        match self {
            PageInfo::Krilla { page_label, .. } => page_label,
            PageInfo::Pdf { page_label, .. } => page_label,
        }
    }

    pub(crate) fn annotations(&self) -> &[(Ref, OnceCell<Ref>)] {
        match self {
            PageInfo::Krilla { annotations, .. } => annotations,
            PageInfo::Pdf { .. } => &[],
        }
    }

    pub(crate) fn annotations_mut(&mut self) -> &mut [(Ref, OnceCell<Ref>)] {
        match self {
            PageInfo::Krilla { annotations, .. } => annotations,
            PageInfo::Pdf { .. } => &mut [],
        }
    }
}

enum StructParentElement {
    /// The index of the page and the number of marked content IDs present on that page.
    Page(usize, i32),
    /// The index of the page where the annotation is present, as well as the index of the
    /// annotation within that one page.
    Annotation(AnnotationIdentifier),
}

#[derive(Debug)]
pub(crate) enum MaybeDeviceColorSpace {
    DeviceRgb,
    DeviceGray,
    DeviceCMYK,
    ColorSpace(resource::ColorSpace),
}

/// The serializer context is more or less the core piece of krilla. It is passed around
/// throughout pretty much the whole conversion process, and contains all mutable state
/// that is needed when writing a PDF file. This includes for example:
/// - Storing all chunks that are produced.
/// - The mappings from OTF fonts to CID/Type 3 fonts.
/// - Annotations used in the document.
///   etc.
pub(crate) struct SerializeContext {
    /// The ref of the page tree.
    page_tree_ref: Ref,
    /// PDF 2.0 namespaces, allocated lazily on first use.
    ///
    /// The standard structure namespace (`ssn`) and the custom krilla
    /// namespace dictionaries are only written when serialising a
    /// tagged PDF 2.0 document. Allocating their indirect refs at
    /// construction time leaked two unused entries into `Ref` numbering
    /// for every PDF (including PDF 1.7 and untagged PDF 2.0) and
    /// caused the trailer `/Size` to overshoot the highest emitted
    /// object id by two — lopdf and other strict readers warn about
    /// this. `pdf2_namespaces()` allocates the pair on first call.
    pdf2_ns: OnceCell<Pdf2Namespaces>,
    /// All global objects, such as PDF fonts, that are populated over time.
    pub(crate) global_objects: GlobalObjects,
    /// Information for each page written so far, index by the page index.
    page_infos: Vec<PageInfo>,
    /// Keep track of object hashes and their corresponding reference. This is used for
    /// caching, so that for example same images will not be embedded twice in the document.
    cached_mappings: HashMap<u128, Ref>,
    /// The current ref in use. All serializers should use the `new_ref` method (which indirectly
    /// is based on this field) to generate a new Ref, instead of creating one manually with
    /// `Ref::new`.
    pub(crate) cur_ref: Ref,
    /// All validation errors that are collected as part of the export process
    /// alongside the validators that raised the error.
    validation_errors: Vec<(ValidationError, Validators)>,
    /// Settings used for serialization.
    serialize_settings: Arc<SerializeSettings>,
    /// Settings used for all PDF object chunks.
    chunk_settings: Settings,
    /// The limits created as part of the serialization process. In principle, we could
    /// just keep track of this in `ChunkContainer`, where all used chunks are stored.
    /// The only reason why `SerializeContext` needs to know about them is that we also
    /// need to merge limits from postscript functions, which are not directly accessible
    /// from the chunk they are written to.
    limits: Limits,
    /// Additional information stored during serialization that allows us to
    /// raise standards errors later.
    validation_store: ValidationStore,
    /// The current location, if set.
    pub(crate) location: Option<Location>,
    /// Indirect ref of the document-wide signature dictionary
    /// (`/FT /Sig` `/V`) emitted at finalise time when the
    /// document is configured with
    /// [`Document::with_digital_signature`](crate::Document::with_digital_signature).
    /// Allocated lazily by [`SerializeContext::signature_dict_ref`]
    /// the first time a [`SignatureField`](crate::annotation::SignatureField)
    /// widget needs to write `/V <ref>`. `None` if the document
    /// carries no digital signature, in which case
    /// [`SignatureField`](crate::annotation::SignatureField) widgets
    /// remain unsigned (`/V` omitted) — the original krilla
    /// behaviour preserved for callers that only need the field
    /// structure.
    pub(crate) signature_dict_ref: Option<Ref>,
    /// `true` when [`Document::with_digital_signature`] has been
    /// called and the document has at least one
    /// [`SignatureField`](crate::annotation::SignatureField)
    /// widget needing to wire `/V`. Drives the eager allocation
    /// of [`Self::signature_dict_ref`].
    pub(crate) signing_enabled: bool,
}

impl SerializeContext {
    pub(crate) fn new(mut serialize_settings: SerializeSettings) -> Self {
        // Override flags as required by the validator
        serialize_settings.no_device_cs |= serialize_settings.validators().requires_no_device_cs();
        serialize_settings.enable_tagging |= serialize_settings.validators().requires_tagging();
        serialize_settings.xmp_metadata |= serialize_settings.validators().requires_xmp_metadata();

        let mut cur_ref = Ref::new(1);
        let page_tree_ref = cur_ref.bump();

        let chunk_settings = Settings {
            pretty: serialize_settings.pretty,
        };

        // An external output profile is only meaningful for PDF/X-4p and
        // PDF/X-6p. If one was supplied but no active validator makes use of
        // it, record a validation error.
        let unsupported_external_output_profile =
            serialize_settings.external_output_profile.is_some()
                && !serialize_settings
                    .validators()
                    .requires_external_output_profile();

        let mut ctx = Self {
            cached_mappings: HashMap::new(),
            pdf2_ns: OnceCell::new(),
            global_objects: GlobalObjects::default(),
            cur_ref,
            page_tree_ref,
            page_infos: vec![],
            location: None,
            validation_errors: vec![],
            serialize_settings: Arc::new(serialize_settings),
            chunk_settings,
            limits: Limits::new(),
            validation_store: ValidationStore::new(),
            signature_dict_ref: None,
            signing_enabled: false,
        };

        if unsupported_external_output_profile {
            ctx.register_validation_error(
                ValidationError::ExternalOutputProfileUnsupportedByValidator,
            );
        }

        ctx
    }

    /// Mark this serialize context as carrying a digital signature.
    /// Idempotent — repeated calls only flip the flag, the actual
    /// signature dict ref is allocated lazily by
    /// [`Self::signature_dict_ref`] so we do not consume a `Ref` for
    /// documents that have no signature widget on any page.
    pub(crate) fn enable_signing(&mut self) {
        self.signing_enabled = true;
    }

    /// Returns the indirect ref of the signature dictionary,
    /// allocating it on first call. Returns `None` when
    /// [`Self::enable_signing`] has not been invoked — the
    /// `WidgetField::Signature` arm then leaves `/V` absent,
    /// preserving the legacy unsigned-widget behaviour.
    pub(crate) fn signature_dict_ref(&mut self) -> Option<Ref> {
        if !self.signing_enabled {
            return None;
        }
        if self.signature_dict_ref.is_none() {
            let r = self.cur_ref.bump();
            self.signature_dict_ref = Some(r);
        }
        self.signature_dict_ref
    }

    /// Return the PDF 2.0 namespace refs, allocating them on first
    /// call. Callers must only invoke this on paths that go on to
    /// emit the corresponding `Namespace` dictionaries — typically the
    /// PDF 2.0 tagged-document branch in `serialize`. The accessor is
    /// `&mut self` because allocating the refs requires bumping
    /// `cur_ref`; the returned borrow is read-only.
    pub(crate) fn pdf2_namespaces(&mut self) -> &Pdf2Namespaces {
        if self.pdf2_ns.get().is_none() {
            let ns = Pdf2Namespaces {
                ssn_ref: self.cur_ref.bump(),
                krilla_ref: self.cur_ref.bump(),
            };
            // `set` only fails if the cell is already initialised,
            // which we have just ruled out under the `&mut self`
            // borrow.
            let _ = self.pdf2_ns.set(ns);
        }
        self.pdf2_ns
            .get()
            .expect("pdf2_ns was initialised in the branch above")
    }

    pub(crate) fn page_infos(&self) -> &[PageInfo] {
        &self.page_infos
    }

    pub(crate) fn page_infos_mut(&mut self) -> &mut [PageInfo] {
        &mut self.page_infos
    }

    pub(crate) fn set_outline(&mut self, outline: Outline) {
        // Only set it if it's not empty or if the current validator requires an
        // outline.
        if !outline.is_empty()
            || self
                .serialize_settings
                .validators()
                .prohibits(&ValidationError::MissingDocumentOutline)
                .is_some()
        {
            self.global_objects.outline = MaybeTaken::new(Some(outline));
        }
    }

    pub(crate) fn set_location(&mut self, location: Location) {
        self.location = Some(location)
    }

    pub(crate) fn reset_location(&mut self) {
        self.location = None
    }

    pub(crate) fn embed_file(
        &mut self,
        chunk_container: &mut ChunkContainer,
        file: EmbeddedFile,
    ) -> Option<()> {
        let name = file.path.clone();
        let embed_location = file.embed_location;
        let ref_ = self.register_cacheable(chunk_container, file);
        if self
            .global_objects
            .embedded_files
            .insert(name, (ref_, embed_location))
            .is_some()
        {
            None
        } else {
            Some(())
        }
    }

    pub(crate) fn set_tag_tree(&mut self, root: TagTree) {
        // Only set the tag tree if the user actually enabled tagging.
        if self.serialize_settings.enable_tagging {
            self.global_objects.tag_tree = MaybeTaken::new(Some(root))
        }
    }

    pub(crate) fn new_ref(&mut self) -> Ref {
        self.cur_ref.bump()
    }

    /// Register an optional content group, allocating its indirect
    /// `/OCG` ref eagerly. Returns the opaque handle the caller will
    /// use with [`crate::surface::Surface::push_layer`].
    pub(crate) fn add_layer(
        &mut self,
        layer: crate::optional_content::Layer,
    ) -> crate::optional_content::LayerHandle {
        let ref_ = self.new_ref();
        // LayerHandle wraps a u32; refuse rather than silently
        // truncate if a pathological caller manages to register
        // more than 4G layers. The realistic upper bound is in the
        // low thousands.
        let index = self.global_objects.layers.len();
        assert!(
            index < u32::MAX as usize,
            "exceeded the {} layer registration limit",
            u32::MAX,
        );
        let handle = crate::optional_content::LayerHandle(index as u32);
        self.global_objects.layers.push(LayerRecord { layer, ref_ });
        handle
    }

    /// Resolve a [`LayerHandle`](crate::optional_content::LayerHandle)
    /// back to the indirect ref of its `/OCG` dictionary.
    ///
    /// # Panics
    /// Panics if the handle does not correspond to a layer registered
    /// on this document (which can only happen if the handle was
    /// fabricated by hand or originated on a different `Document`).
    pub(crate) fn layer_ref(&self, handle: crate::optional_content::LayerHandle) -> Ref {
        self.global_objects
            .layers
            .get(handle.0 as usize)
            .map(|r| r.ref_)
            .expect("LayerHandle out of bounds — was it created on a different Document?")
    }

    /// Register a custom external namespace by URI. Returns an opaque
    /// handle that can be passed to
    /// [`crate::tagging::TagNamespace::Custom`] (and ultimately to
    /// [`Tag::with_namespace`](crate::tagging::TagKind::with_namespace))
    /// to bind a structure element to the namespace. Repeated calls
    /// with the same URI return the same handle — the on-disk PDF
    /// carries at most one `Namespace` dict per URI.
    pub(crate) fn register_namespace(
        &mut self,
        uri: impl Into<String>,
    ) -> crate::interchange::tagging::NamespaceHandle {
        let uri = uri.into();
        if let Some(pos) = self
            .global_objects
            .custom_namespaces
            .iter()
            .position(|r| r.uri == uri)
        {
            return crate::interchange::tagging::NamespaceHandle(pos as u32);
        }
        let ref_ = self.new_ref();
        let index = self.global_objects.custom_namespaces.len();
        assert!(
            index < u32::MAX as usize,
            "exceeded the {} custom-namespace registration limit",
            u32::MAX,
        );
        self.global_objects
            .custom_namespaces
            .push(CustomNamespaceRecord { uri, ref_ });
        crate::interchange::tagging::NamespaceHandle(index as u32)
    }

    /// Resolve a [`NamespaceHandle`](crate::tagging::NamespaceHandle)
    /// back to the indirect ref of its `Namespace` dictionary.
    ///
    /// # Panics
    /// Panics if the handle does not correspond to a namespace
    /// registered on this document.
    pub(crate) fn custom_namespace_ref(
        &self,
        handle: crate::interchange::tagging::NamespaceHandle,
    ) -> Ref {
        self.global_objects
            .custom_namespaces
            .get(handle.0 as usize)
            .map(|r| r.ref_)
            .expect("NamespaceHandle out of bounds — was it created on a different Document?")
    }

    /// Indirect ref of the document-level Type1 Helvetica font dict
    /// used for AcroForm widget appearance streams (ISO 32000-2 §12.7.4).
    ///
    /// Lazily allocates the ref and emits the font dict into
    /// `chunk_container.fonts` on first call; subsequent calls return
    /// the same ref. Standard-14 Type1 (`/BaseFont /Helvetica`) with
    /// `WinAnsiEncoding` so widget appearance content streams can draw
    /// Latin-1 byte strings via `Tj`.
    ///
    /// The font dict is materialised at the end of serialisation by
    /// [`SerializeContext::flush_standard_helvetica`] when at least one
    /// caller requested it. Here we only allocate the ref so widget
    /// appearance streams can reference it before any chunk is emitted.
    pub(crate) fn standard_helvetica_ref(&mut self) -> Ref {
        if let Some(ref_) = self.global_objects.standard_helvetica_font {
            return ref_;
        }
        let font_ref = self.cur_ref.bump();
        self.global_objects.standard_helvetica_font = Some(font_ref);
        font_ref
    }

    /// Emit the document-level Type1 Helvetica font dict into
    /// `chunk_container.fonts` if any caller requested its ref via
    /// [`Self::standard_helvetica_ref`]. Idempotent; called once at the
    /// end of `serialize_fonts`.
    fn flush_standard_helvetica(&mut self, chunk_container: &mut ChunkContainer) {
        let Some(font_ref) = self.global_objects.standard_helvetica_font else {
            return;
        };
        let mut chunk = Chunk::new();
        chunk
            .indirect(font_ref)
            .dict()
            .pair(Name(b"Type"), Name(b"Font"))
            .pair(Name(b"Subtype"), Name(b"Type1"))
            .pair(Name(b"BaseFont"), Name(b"Helvetica"))
            .pair(Name(b"Encoding"), Name(b"WinAnsiEncoding"));
        chunk_container.streams.fonts.push(chunk);
    }

    pub(crate) fn serialize_settings(&self) -> Arc<SerializeSettings> {
        self.serialize_settings.clone()
    }

    // IMPORTANT: DO NEVER CALL `Chunk::new`, `Pdf::new` or `Content::new` directly! Instead,
    // always make sure to use the methods on `SerializeContext`, to ensure the
    // flags are applied consistently across all chunks.

    pub(crate) fn new_chunk(&self) -> Chunk {
        Chunk::with_settings(self.chunk_settings)
    }

    pub(crate) fn new_content(&self) -> Content {
        Content::with_settings(self.chunk_settings)
    }

    pub(crate) fn new_pdf_with_capacity(&self, capacity: usize) -> Pdf {
        Pdf::with_settings_and_capacity(self.chunk_settings, capacity)
    }

    #[cfg(feature = "pdf")]
    pub(crate) fn chunk_settings(&self) -> Settings {
        self.chunk_settings
    }

    #[cfg(feature = "pdf")]
    pub(crate) fn embed_pdf_pages(&mut self, pdf: &PdfDocument, page_indices: &[usize]) {
        for page_idx in page_indices {
            let page_ref = self.new_ref();
            let size = pdf
                .pages()
                .get(*page_idx)
                .and_then(|p| {
                    let (x, y) = p.render_dimensions();
                    Size::from_wh(x, y)
                })
                // In case the page doesn't exist, we will catch the error later, so just use
                // a dummy size.
                .unwrap_or(Size::from_wh(1.0, 1.0).unwrap());
            self.global_objects
                .pdf_ctx
                .add_page(pdf, *page_idx, page_ref, self.location);
            self.page_infos.push(PageInfo::Pdf {
                ref_: page_ref,
                size,
                // TODO: Maybe this should be configurable.
                page_label: PageLabel::default(),
            });
        }
    }

    #[cfg(feature = "pdf")]
    pub(crate) fn embed_pdf_page_as_xobject(&mut self, pdf: &PdfDocument, page_idx: usize) -> Ref {
        let xobj_ref = self.new_ref();

        // Note that `add_xobject` might return a different ref than the one we created.
        self.global_objects
            .pdf_ctx
            .add_xobject(pdf, page_idx, xobj_ref, self.location)
    }

    pub(crate) fn page_tree_ref(&mut self) -> Ref {
        self.page_tree_ref
    }

    pub(crate) fn register_font_container(&mut self, font: Font) -> Rc<RefCell<FontContainer>> {
        self.global_objects
            .font_map
            .entry(font.clone())
            .or_insert_with(|| Rc::new(RefCell::new(FontContainer::new(font.clone()))))
            .clone()
    }

    pub(crate) fn validation_store(&mut self) -> &mut ValidationStore {
        &mut self.validation_store
    }

    pub(crate) fn finish(mut self, mut chunk_container: ChunkContainer) -> KrillaResult<Vec<u8>> {
        // We need to be careful here that we serialize the objects in the right order,
        // as in some cases we use MaybeTake::take to remove an object, which means that
        // no object that is serialized afterwards must depend on it.

        // Serialize all objects that can only be written in the end.
        self.serialize_destination_profiles(&mut chunk_container);
        self.serialize_page_label_tree(&mut chunk_container);
        self.serialize_outline(&mut chunk_container);
        self.serialize_fonts(&mut chunk_container)?;
        self.serialize_pages(&mut chunk_container)?;
        // Widget appearance streams allocate the standard Helvetica ref
        // during page serialisation; emit its font dict once afterwards.
        self.flush_standard_helvetica(&mut chunk_container);
        self.serialize_page_tree(&mut chunk_container);
        #[cfg(feature = "pdf")]
        self.serialize_embedded_pdfs(&mut chunk_container)?;
        self.serialize_xyz_destinations(&mut chunk_container)?;
        // It is important that we serialize the tags AFTER we have serialized the pages,
        // because page serialization will update the annotation refs of the page infos,
        // and when serializing the parent tree map we need to know the refs of the annotations
        self.serialize_tag_tree(&mut chunk_container)?;

        // Create the final PDF. The companion `xref_stream_ref` is
        // pre-allocated alongside the other final-numbering refs in
        // `ChunkContainer::finish`; this finalises whether the trailer
        // is written as a `xref` table (default) or as a `/Type /XRef`
        // stream (when `xref_streams` is enabled).
        let (pdf, xref_stream_ref) = chunk_container.finish(&mut self)?;
        self.register_limits(pdf.limits());

        self.check_validator_limits();

        if !self.validation_errors.is_empty() {
            // Deduplicate errors, while still preserving order.
            let mut errors = vec![];
            let mut seen = HashSet::new();

            for error in self.validation_errors {
                if !seen.contains(&error) {
                    seen.insert(error.clone());
                    errors.push(error);
                }
            }

            return Err(KrillaError::Validation(errors));
        }

        if let Some(limit_error) = self.check_version_limits() {
            return Err(KrillaError::Limit(limit_error));
        }

        // Just a sanity check that we've actually processed all items.
        self.global_objects.assert_all_taken();

        Ok(match xref_stream_ref {
            Some(r) => pdf.finish_with_xref_stream(r),
            None => pdf.finish(),
        })
    }
}

/// Various registration methods.
impl SerializeContext {
    pub(crate) fn register_validation_error(&mut self, error: ValidationError) {
        if let Some(validators) = self.serialize_settings().validators().prohibits(&error) {
            self.validation_errors.push((error, validators))
        }
    }

    pub(crate) fn register_limits(&mut self, limits: &Limits) {
        self.limits.merge(limits);
    }

    pub(crate) fn register_page_struct_parent(
        &mut self,
        page_index: usize,
        num_mcids: i32,
    ) -> Option<i32> {
        if self.serialize_settings.enable_tagging {
            if num_mcids == 0 {
                return None;
            }

            let id = self.global_objects.struct_parents.len();
            self.global_objects
                .struct_parents
                .push(StructParentElement::Page(page_index, num_mcids));
            Some(i32::try_from(id).unwrap())
        } else {
            None
        }
    }

    /// Register the struct parent integer in the parent tree.
    /// The annotation parent must be later set using [`Self::set_annotation_parent`].
    pub(crate) fn register_annotation_parent(&mut self, ai: AnnotationIdentifier) -> Option<i32> {
        if self.serialize_settings.enable_tagging {
            let id = self.global_objects.struct_parents.len();
            self.global_objects
                .struct_parents
                .push(StructParentElement::Annotation(ai));
            Some(i32::try_from(id).unwrap())
        } else {
            None
        }
    }

    pub(crate) fn register_named_destination(&mut self, nd: NamedDestination) -> Option<Ref> {
        if let Some((dest_ref, existing)) =
            self.global_objects.named_destinations.get(nd.name.as_ref())
        {
            return (existing == nd.xyz_dest.as_ref()).then_some(*dest_ref);
        }

        let dest_ref = self.register_xyz_destination((*nd.xyz_dest).clone());
        self.global_objects
            .named_destinations
            .insert(nd.name.clone(), (dest_ref, (*nd.xyz_dest).clone()));
        Some(dest_ref)
    }

    /// Register the indirect reference of an AcroForm widget annotation
    /// so the document catalogue's `/AcroForm /Fields` array includes
    /// it (ISO 32000-2 §12.7.3). Called from
    /// [`crate::interactive::annotation::Annotation::serialize`] when
    /// the annotation type is [`AnnotationType::Widget`].
    pub(crate) fn register_widget_field(&mut self, ref_: Ref) {
        self.global_objects.widget_fields.push(ref_);
    }

    pub(crate) fn register_page(&mut self, page: InternalPage) {
        let ref_ = self.new_ref();
        self.page_infos.push(PageInfo::Krilla {
            ref_,
            surface_size: page.page_settings.surface_size(),
            // Will be populated when the page is serialized.
            annotations: vec![],
            page_label: page.page_settings.page_label().clone(),
        });
        self.global_objects.pages.push((ref_, page));
    }

    fn register_cached<T: SipHashable>(
        &mut self,
        item: T,
        mut func: impl FnMut(&mut Self, T, Ref),
    ) -> Ref {
        let hash = item.sip_hash();
        if let Some(_ref) = self.cached_mappings.get(&hash) {
            *_ref
        } else {
            let root_ref = self.new_ref();
            func(self, item, root_ref);
            self.cached_mappings.insert(hash, root_ref);
            root_ref
        }
    }

    pub(crate) fn register_cacheable<T>(
        &mut self,
        chunk_container: &mut ChunkContainer,
        object: T,
    ) -> Ref
    where
        T: Cacheable,
    {
        self.register_cached(object, |sc, object, root_ref| {
            object.serialize(sc, chunk_container, root_ref);
        })
    }

    pub(crate) fn register_resourceable<T>(
        &mut self,
        chunk_container: &mut ChunkContainer,
        object: T,
    ) -> T::Resource
    where
        T: Resourceable,
    {
        Resource::new(self.register_cacheable(chunk_container, object))
    }

    #[cfg(feature = "raster-images")]
    pub(crate) fn register_image(
        &mut self,
        chunk_container: &mut ChunkContainer,
        image: Image,
    ) -> Ref {
        self.register_cached(image, |sc, object, root_ref| {
            object.serialize(sc, chunk_container, root_ref);
        })
    }

    pub(crate) fn register_xyz_destination(&mut self, dest: XyzDestination) -> Ref {
        self.register_cached(dest, |sc, dest, root_ref| {
            sc.global_objects.xyz_destinations.push((root_ref, dest));
        })
    }

    pub(crate) fn register_page_label(
        &mut self,
        chunk_container: &mut ChunkContainer,
        page_label: PageLabel,
    ) -> Ref {
        let ref_ = self.new_ref();
        page_label.serialize(chunk_container, ref_);
        ref_
    }

    pub(crate) fn register_font_identifier(&mut self, f: FontIdentifier) -> resource::Font {
        let hash = f.sip_hash();
        if let Some(_ref) = self.cached_mappings.get(&hash) {
            resource::Font::new(*_ref)
        } else {
            let root_ref = self.new_ref();
            self.cached_mappings.insert(hash, root_ref);
            resource::Font::new(root_ref)
        }
    }

    pub(crate) fn register_colorspace(
        &mut self,
        chunk_container: &mut ChunkContainer,
        cs: ColorSpace,
    ) -> MaybeDeviceColorSpace {
        match cs {
            ColorSpace::CieBased(CieBasedColorSpace::Srgb) => {
                MaybeDeviceColorSpace::ColorSpace(self.register_resourceable(
                    chunk_container,
                    ICCBasedColorSpace(self.serialize_settings.pdf_version().rgb_icc()),
                ))
            }
            ColorSpace::CieBased(CieBasedColorSpace::Luma) => {
                MaybeDeviceColorSpace::ColorSpace(self.register_resourceable(
                    chunk_container,
                    ICCBasedColorSpace(self.serialize_settings.pdf_version().grey_icc()),
                ))
            }
            ColorSpace::CieBased(CieBasedColorSpace::Cmyk(cs)) => {
                MaybeDeviceColorSpace::ColorSpace(self.register_resourceable(chunk_container, cs))
            }
            ColorSpace::CieBased(CieBasedColorSpace::IccRgb(cs)) => {
                MaybeDeviceColorSpace::ColorSpace(self.register_resourceable(chunk_container, cs))
            }
            ColorSpace::CieBased(CieBasedColorSpace::CalRgb(cs)) => {
                MaybeDeviceColorSpace::ColorSpace(self.register_resourceable(chunk_container, cs))
            }
            ColorSpace::CieBased(CieBasedColorSpace::CalGray(cs)) => {
                MaybeDeviceColorSpace::ColorSpace(self.register_resourceable(chunk_container, cs))
            }
            ColorSpace::CieBased(CieBasedColorSpace::Lab(cs)) => {
                MaybeDeviceColorSpace::ColorSpace(self.register_resourceable(chunk_container, cs))
            }
            ColorSpace::Device(DeviceColorSpace::Gray) => MaybeDeviceColorSpace::DeviceGray,
            ColorSpace::Device(DeviceColorSpace::Rgb) => MaybeDeviceColorSpace::DeviceRgb,
            ColorSpace::Device(DeviceColorSpace::Cmyk) => MaybeDeviceColorSpace::DeviceCMYK,
            ColorSpace::Special(SpecialColorSpace::Separation(s)) => {
                MaybeDeviceColorSpace::ColorSpace(
                    self.register_resourceable(chunk_container, SeparationColorSpace::new(s)),
                )
            }
            ColorSpace::Special(SpecialColorSpace::DeviceN(s)) => {
                MaybeDeviceColorSpace::ColorSpace(
                    self.register_resourceable(chunk_container, DeviceNColorSpace::new(s)),
                )
            }
        }
    }
}

/// Various serialization methods.
/// All methods are supposed to only be called once in `SerializeContext::finish`!
impl SerializeContext {
    fn serialize_destination_profiles(&mut self, chunk_container: &mut ChunkContainer) {
        let validators = self.serialize_settings.validators();
        let subtypes = validators.output_intents();
        let custom_intents = self.serialize_settings.output_intents.clone();
        // Fallback CMYK profile fires only when no other intent source is
        // present. It is the colour-management default for documents that
        // contain CMYK content but neither pick a validator-driven intent
        // nor supply explicit caller intents.
        let fallback_cmyk = if subtypes.is_empty()
            && custom_intents.is_empty()
            && self.serialize_settings.fallback_cmyk_profile.is_some()
        {
            self.serialize_settings.fallback_cmyk_profile.clone()
        } else {
            None
        };

        if subtypes.is_empty() && custom_intents.is_empty() && fallback_cmyk.is_none() {
            return;
        }

        let root_ref = self.new_ref();
        let mut chunk = self.new_chunk();
        let mut oi_refs = Vec::new();

        for subtype in subtypes {
            let oi_ref = self.new_ref();

            // PDF/X-4p and PDF/X-6p reference the output profile externally
            // instead of embedding it.
            if validators.requires_external_output_profile() && subtype == OutputIntentSubtype::PDFX
            {
                let Some(external_profile) =
                    self.serialize_settings.external_output_profile.clone()
                else {
                    self.register_validation_error(ValidationError::MissingExternalOutputProfile);
                    continue;
                };

                // `ExternalOutputProfile` guarantees non-empty URLs / identifier / info
                // at construction time, so no runtime validation is needed here.
                let metadata = external_profile.profile().metadata();
                // Annex A.1 → §6.4.2.1: the referenced profile must characterize
                // an output device (Device Class `prtr`) and use an admissible
                // ICC version (v2, or v4 up to v4.2). Its colour space is
                // constrained to GRAY/RGB/CMYK by the `ExternalOutputProfile`
                // constructors.
                if !metadata.is_output_rendering_device() {
                    self.register_validation_error(
                        ValidationError::InvalidOutputProfileDeviceClass(None),
                    );
                }
                if output_profile_version_too_new(
                    self.serialize_settings.pdf_version(),
                    metadata.major,
                    metadata.minor,
                ) {
                    self.register_validation_error(
                        ValidationError::IncompatibleOutputProfileVersion(None),
                    );
                }
                let mut dict = chunk.indirect(oi_ref).dict();
                dict.pair(Name(b"Type"), Name(b"OutputIntent"));
                dict.pair(Name(b"S"), Name(b"GTS_PDFX"));
                dict.pair(
                    Name(b"OutputConditionIdentifier"),
                    TextStr(external_profile.output_condition_identifier()),
                );
                if let Some(output_condition) = external_profile.output_condition() {
                    dict.pair(Name(b"OutputCondition"), TextStr(output_condition));
                }
                if let Some(registry_name) = external_profile.registry_name() {
                    dict.pair(Name(b"RegistryName"), TextStr(registry_name));
                }
                dict.pair(Name(b"Info"), TextStr(external_profile.info()));

                {
                    let mut profile_ref = dict.insert(Name(b"DestOutputProfileRef")).dict();
                    profile_ref.pair(Name(b"CheckSum"), Str(&metadata.checksum));
                    profile_ref.pair(Name(b"ICCVersion"), Str(&metadata.version_bytes));
                    profile_ref.pair(Name(b"ProfileCS"), Str(&metadata.color_space_signature));
                    // ProfileName is required; fall back to the always-present
                    // output-condition info when the profile carries no parseable
                    // description tag.
                    let profile_name = metadata
                        .profile_name
                        .as_deref()
                        .unwrap_or_else(|| external_profile.info());
                    profile_ref.pair(Name(b"ProfileName"), TextStr(profile_name));

                    let mut urls = profile_ref.insert(Name(b"URLs")).array();
                    for url in external_profile.urls() {
                        let mut file_spec = urls.push().start::<FileSpec>();
                        file_spec
                            .file_system(Name(b"URL"))
                            .path(Str(url.as_bytes()));
                    }
                }

                dict.finish();
                oi_refs.push(oi_ref);
                continue;
            }

            let cmyk_desc = if validators.uses_cmyk_output_profile_for_subtype(subtype) {
                match self.serialize_settings.cmyk_profile.clone() {
                    Some(profile) => {
                        // The output-intent profile's ICC version must not exceed
                        // what the target PDF version admits (v2 for PDF 1.4, v4.2
                        // for PDF 1.6, v4.3 for PDF 2.0 — see
                        // `output_profile_version_too_new`). The output intent is
                        // mandatory, so a too-new version is an error (unlike an
                        // image profile, which is simply dropped).
                        let m = profile.metadata();
                        if output_profile_version_too_new(
                            self.serialize_settings.pdf_version(),
                            m.major,
                            m.minor,
                        ) {
                            self.register_validation_error(
                                ValidationError::IncompatibleOutputProfileVersion(None),
                            );
                        }
                        // A PDF/X output intent must characterize an output device
                        // (Device Class `prtr`).
                        if !m.is_output_rendering_device() {
                            self.register_validation_error(
                                ValidationError::InvalidOutputProfileDeviceClass(None),
                            );
                        }
                        // ISO 15930-7 §6.4.1 / ISO 15930-9 §6.6.1: the
                        // characterized printing condition must have a
                        // GRAY/RGB/CMYK data colour space. The embedded
                        // `cmyk_profile` is the CMYK output target, so a
                        // four-channel but non-`'CMYK'` profile (e.g. `'4CLR'`
                        // DeviceN) is not acceptable. The external (-p) path
                        // performs the equivalent check at construction time.
                        if m.color_space != ICCColorSpace::Cmyk {
                            self.register_validation_error(
                                ValidationError::InvalidOutputProfileColorSpace(None),
                            );
                        }
                        let major = m.major;
                        let minor = m.minor;
                        let profile_ref = self.register_cacheable(chunk_container, profile);
                        Some((profile_ref, major, minor))
                    }
                    None => {
                        // PDF/X requires a CMYK output intent profile. Fall back
                        // to sRGB so we still produce a structurally valid PDF
                        // while registering the validation error.
                        self.register_validation_error(ValidationError::MissingCMYKProfile);
                        None
                    }
                }
            } else {
                None
            };

            let mut oi = chunk.indirect(oi_ref).start::<OutputIntent>();
            if let Some((profile_ref, major, minor)) = cmyk_desc {
                oi.dest_output_profile(profile_ref)
                    .subtype(subtype)
                    // No RegistryName: ISO 15930-7 §6.4.2.1 requires that key
                    // only when the printing condition is registry-defined, which
                    // an embedded (unregistered "Custom") profile is not.
                    .output_condition_identifier(TextStr("Custom"))
                    .output_condition(TextStr("CMYK"))
                    .info(TextStr(format!("CMYK v{major}.{minor}").as_str()));
            } else {
                // sRGB output intent: PDF/A, or the fallback when a CMYK profile
                // was required but not supplied.
                let icc_profile = self.serialize_settings.pdf_version().rgb_icc();
                let major = icc_profile.metadata().major;
                let minor = icc_profile.metadata().minor;
                let profile_ref = self.register_cacheable(chunk_container, icc_profile);
                oi.dest_output_profile(profile_ref)
                    .subtype(subtype)
                    .output_condition_identifier(TextStr("Custom"))
                    .output_condition(TextStr("sRGB"))
                    .info(TextStr(format!("sRGB v{major}.{minor}").as_str()));
            }

            oi.finish();
            oi_refs.push(oi_ref);
        }

        for intent in custom_intents {
            let oi_ref = self.new_ref();
            let profile_ref = self.register_cacheable(chunk_container, intent.profile().clone());
            let subtype = intent.pdf_writer_subtype();
            let mut oi = chunk.indirect(oi_ref).start::<OutputIntent>();
            oi.dest_output_profile(profile_ref)
                .subtype(subtype)
                .output_condition_identifier(TextStr(intent.output_condition_identifier()))
                .info(TextStr(intent.info()));
            if let Some(condition) = intent.output_condition() {
                oi.output_condition(TextStr(condition));
            }
            if let Some(registry) = intent.registry_name() {
                oi.registry_name(TextStr(registry));
            }
            oi.finish();
            oi_refs.push(oi_ref);
        }

        // Fallback CMYK output intent: when no validator-generated and no
        // caller-supplied intents exist, emit a single default intent
        // referencing the fallback profile so colour-managed consumers can
        // resolve device CMYK content.
        if let Some(profile) = fallback_cmyk {
            let oi_ref = self.new_ref();
            let major = profile.metadata().major;
            let minor = profile.metadata().minor;
            let profile_ref = self.register_cacheable(chunk_container, profile);
            let mut oi = chunk.indirect(oi_ref).start::<OutputIntent>();
            oi.dest_output_profile(profile_ref)
                .subtype(pdf_writer::types::OutputIntentSubtype::PDFX)
                .output_condition_identifier(TextStr("Custom"))
                .output_condition(TextStr("CMYK"))
                .registry_name(TextStr(""))
                .info(TextStr(format!("CMYK v{}.{}", major, minor).as_str()));
            oi.finish();
            oi_refs.push(oi_ref);
        }

        if oi_refs.is_empty() {
            return;
        }

        let mut array = chunk.indirect(root_ref).array();
        for oi_ref in oi_refs {
            array.item(oi_ref);
        }
        array.finish();

        chunk_container.non_stream.destination_profiles = Some((root_ref, chunk));
    }

    fn serialize_page_label_tree(&mut self, chunk_container: &mut ChunkContainer) {
        if let Some(container) = PageLabelContainer::new(
            &self
                .page_infos
                .iter()
                .map(|page| page.page_label().clone())
                .collect::<Vec<_>>(),
        ) {
            let page_label_tree_ref = self.new_ref();
            container.serialize(self, chunk_container, page_label_tree_ref);
        }
    }

    fn serialize_outline(&mut self, chunk_container: &mut ChunkContainer) {
        let outline = self.global_objects.outline.take();
        if let Some(outline) = &outline {
            let outline_ref = self.new_ref();
            outline.serialize(self, chunk_container, outline_ref);
        } else {
            self.register_validation_error(ValidationError::MissingDocumentOutline);
        }
    }

    #[cfg(feature = "pdf")]
    fn serialize_embedded_pdfs(
        &mut self,
        chunk_container: &mut ChunkContainer,
    ) -> KrillaResult<()> {
        let pdf_ctx = self.global_objects.pdf_ctx.take();

        pdf_ctx.serialize(self, chunk_container)
    }

    fn serialize_fonts(&mut self, chunk_container: &mut ChunkContainer) -> KrillaResult<()> {
        let fonts = self.global_objects.font_map.take();
        for font_container in fonts.values() {
            let borrowed = font_container.borrow();

            if !borrowed.type3_mapper().is_empty() {
                for t3_font in borrowed.type3_mapper().fonts() {
                    let f = self.register_font_identifier(t3_font.identifier());
                    t3_font.serialize(self, chunk_container, f.get_ref());
                }
            }

            if !borrowed.cid_font().is_empty() {
                let f = self.register_font_identifier(borrowed.cid_font().identifier());
                borrowed
                    .cid_font()
                    .serialize(self, chunk_container, f.get_ref())?;
            }
        }

        Ok(())
    }

    fn serialize_pages(&mut self, chunk_container: &mut ChunkContainer) -> KrillaResult<()> {
        let pages = self.global_objects.pages.take();
        for (ref_, page) in pages {
            page.serialize(self, chunk_container, ref_)?;
        }

        Ok(())
    }

    fn serialize_page_tree(&mut self, chunk_container: &mut ChunkContainer) {
        let mut page_tree_chunk = self.new_chunk();
        page_tree_chunk
            .pages(self.page_tree_ref)
            .count(self.page_infos.len() as i32)
            .kids(self.page_infos.iter().map(|i| i.ref_()));
        chunk_container.non_stream.page_tree = Some((self.page_tree_ref, page_tree_chunk));
    }

    fn serialize_xyz_destinations(
        &mut self,
        chunk_container: &mut ChunkContainer,
    ) -> KrillaResult<()> {
        let xyz_destinations = self.global_objects.xyz_destinations.take();
        for (ref_, dest) in &xyz_destinations {
            dest.serialize(self, chunk_container, *ref_);
        }

        Ok(())
    }

    fn serialize_tag_tree(&mut self, chunk_container: &mut ChunkContainer) -> KrillaResult<()> {
        let tag_tree = self.global_objects.tag_tree.take();
        let struct_parents = self.global_objects.struct_parents.take();
        if let Some(root) = &tag_tree {
            let mut parent_tree_map = HashMap::new();
            let mut id_tree_map = BTreeMap::new();
            let struct_tree_root_ref = self.new_ref();
            let document_ref = root.serialize(
                self,
                chunk_container,
                &mut parent_tree_map,
                &mut id_tree_map,
                struct_tree_root_ref,
            )?;
            // Take the custom-namespace registry only AFTER tag
            // serialisation — `resolve_ns_override` inside
            // `write_kind` looks up handles through
            // `SerializeContext::custom_namespace_ref` during the
            // call above, which queries the registry via &self.
            // Taking before that call would `MaybeTaken::take` it
            // out from under those reads.
            let custom_namespaces = self.global_objects.custom_namespaces.take();

            root.validate(&id_tree_map)?;

            let mut chunk = self.new_chunk();
            let mut tree = chunk
                .indirect(struct_tree_root_ref)
                .start::<StructTreeRoot>();

            let mut sub_chunks = vec![];

            if self.serialize_settings.pdf_version() < PdfVersion::Pdf20 {
                // Built-in /RoleMap entries, in the historical
                // emission order (snapshot tests pin the dict's byte
                // layout). User-supplied entries below override
                // values in place when the key already exists, and
                // are appended otherwise — so the on-disk dict never
                // carries duplicate keys (PDF dict behaviour for
                // duplicates is implementation-defined).
                let mut entries: Vec<(Vec<u8>, StructRole)> = vec![
                    // Custom structure elements.
                    (b"Datetime".to_vec(), StructRole::Span),
                    (b"Terms".to_vec(), StructRole::Part),
                    // PDF 2.0 exclusive structure elements.
                    (b"Title".to_vec(), StructRole::P),
                    (b"Strong".to_vec(), StructRole::Span),
                    (b"Em".to_vec(), StructRole::Span),
                    // `Sub` is PDF 2.0 only in the SSN; emitting it
                    // as a custom kind on PDF 1.7 requires a /RoleMap
                    // entry so legacy consumers can still treat the
                    // subdivision as inline content.
                    (b"Sub".to_vec(), StructRole::Span),
                ];
                for level in self.global_objects.custom_heading_roles.iter() {
                    let role2 = StructRole2::Heading(*level);
                    let mut buf = [0; 6];
                    let name = role2.to_name(&mut buf);
                    entries.push((name.0.to_vec(), StructRole::P));
                }
                for (name, role) in root.role_map.iter() {
                    match entries.iter_mut().find(|(k, _)| k == name) {
                        Some(slot) => slot.1 = *role,
                        None => entries.push((name.clone(), *role)),
                    }
                }

                let mut role_map = tree.role_map();
                for (name, role) in &entries {
                    role_map.insert(Name(name.as_slice()), *role);
                }
            } else {
                // Allocate the standard structure and custom krilla
                // namespace refs only on this branch — the PDF 2.0
                // tagged-document path. PDF 1.7 documents and
                // untagged PDF 2.0 documents never reach here, so
                // their refs are never bumped and trailer `/Size`
                // matches the highest emitted object id.
                let pdf2_ns = *self.pdf2_namespaces();
                let mut namespaces = tree.namespaces();

                // PDF 2.0 standard structure namespace
                namespaces.item(pdf2_ns.ssn_ref);
                let mut ns_chunk = self.new_chunk();
                ns_chunk.namespace(pdf2_ns.ssn_ref).pdf_2_ns();
                sub_chunks.push(ns_chunk);

                // Custom krilla namspace
                namespaces.item(pdf2_ns.krilla_ref);
                let mut ns_chunk = self.new_chunk();
                let mut ns = ns_chunk.namespace(pdf2_ns.krilla_ref);
                ns.ns(TextStr("https://github.com/LaurenzV/krilla"));

                // Custom structure elements.
                ns.role_map_ns()
                    .to_pdf_2_0(Name(b"Datetime"), StructRole2::Span, pdf2_ns.ssn_ref)
                    .to_pdf_2_0(Name(b"Terms"), StructRole2::Part, pdf2_ns.ssn_ref);

                ns.finish();
                sub_chunks.push(ns_chunk);

                // Caller-registered external namespaces (MathML,
                // HTML 4, PDF Math, …) declared via
                // `Document::register_namespace`. Each entry adds
                // one ref to the catalogue's `/Namespaces` array
                // and one indirect `Namespace` dict whose `/NS`
                // entry carries the URI. The default-binding
                // tables in `write_kind_*` continue to use the
                // standard / krilla namespaces; structure elements
                // pick up a custom namespace only when the caller
                // sets `with_namespace(Some(TagNamespace::Custom(...)))`.
                for record in &custom_namespaces {
                    namespaces.item(record.ref_);
                    let mut ns_chunk = self.new_chunk();
                    let mut ns = ns_chunk.namespace(record.ref_);
                    ns.ns(TextStr(&record.uri));
                    ns.finish();
                    sub_chunks.push(ns_chunk);
                }
            }
            tree.children().item(document_ref);

            if !struct_parents.is_empty() {
                let mut parent_tree = tree.parent_tree();
                let mut tree_nums = parent_tree.nums();

                for (index, struct_parent) in struct_parents.iter().enumerate() {
                    match *struct_parent {
                        StructParentElement::Page(page_index, num_mcids) => {
                            let mut list_chunk = self.new_chunk();
                            let list_ref = self.new_ref();

                            let mut refs = list_chunk.indirect(list_ref).array();

                            for mcid in 0..num_mcids {
                                let rci = PageTagIdentifier::new(page_index, mcid);
                                refs.item(parent_tree_map.get(&rci.into()).unwrap_or_else(|| {
                                    panic!(
                                        "page tag identifier {rci:?} doesn't appear in the tag tree"
                                    )
                                }));
                            }

                            refs.finish();

                            sub_chunks.push(list_chunk);
                            tree_nums.insert(index as i32, list_ref);
                        }
                        StructParentElement::Annotation(ai) => {
                            // Write a reference to the parent structure element.
                            // From the PDF 1.7 spec (14.7.5.4 Finding structure elements from content items):
                            // > For an object identified as a content item by means of an object reference
                            // > (see 14.7.5.3, "PDF objects as content items"), the value shall be an
                            // > indirect reference to the parent structure element.
                            let page_annotations = &self.page_infos[ai.page_index].annotations();
                            let parent_ref =
                                *page_annotations[ai.annot_index].1.get().unwrap_or_else(|| {
                                    panic!("annotation identifier {ai:?} doesn't appear in the tag tree")
                                });
                            tree_nums.insert(index as i32, parent_ref);
                        }
                    }
                }

                tree_nums.finish();
                parent_tree.finish();
            }

            if !id_tree_map.is_empty() {
                let mut id_tree = tree.id_tree();
                let mut names = id_tree.names();

                for (name, ref_) in id_tree_map {
                    names.insert(Str(name.as_bytes()), ref_);
                }
            }

            if !struct_parents.is_empty() {
                tree.parent_tree_next_key(struct_parents.len() as i32);
            }
            tree.finish();

            for sub_chunk in sub_chunks {
                chunk.extend(&sub_chunk);
            }

            chunk_container.non_stream.struct_tree_root = Some((struct_tree_root_ref, chunk));
        } else {
            self.register_validation_error(ValidationError::MissingTagging);
        }

        if !self.global_objects.custom_namespaces.is_taken() {
            // Documents without a tag tree never entered the branch
            // above that took the registry; consume the MaybeTaken
            // slot here so `finish()`'s `assert_all_taken`
            // discipline holds. Registrations made on a document
            // that ultimately has no tag tree are silently dropped
            // — there is no structure element to bind them to.
            let _ = self.global_objects.custom_namespaces.take();
        }

        Ok(())
    }

    fn check_validator_limits(&mut self) {
        if self.cur_ref > Ref::new(8388607) {
            self.register_validation_error(ValidationError::TooManyIndirectObjects)
        }

        if self.limits.str_len() > STR_LEN {
            self.register_validation_error(ValidationError::TooLongString);
        }

        if self.limits.name_len() > NAME_LEN {
            self.register_validation_error(ValidationError::TooLongName);
        }

        if self.limits.real() > MAX_FLOAT {
            self.register_validation_error(ValidationError::TooLargeFloat);
        }

        if self.limits.array_len() > ARRAY_LEN {
            self.register_validation_error(ValidationError::TooLongArray);
        }

        if self.limits.dict_entries() > DICT_LEN {
            self.register_validation_error(ValidationError::TooLongDictionary);
        }
    }

    fn check_version_limits(&self) -> Option<LimitError> {
        if self.serialize_settings.pdf_version() != PdfVersion::Pdf14 {
            return None;
        }

        if self.limits.real() > MAX_FLOAT {
            return Some(LimitError::TooLargeFloat);
        }

        if self.limits.array_len() > ARRAY_LEN {
            return Some(LimitError::TooLongArray);
        }

        if self.limits.dict_entries() > DICT_LEN {
            return Some(LimitError::TooLongDictionary);
        }

        None
    }
}

/// This struct is essentially a thin wrapper around `std::mem::replace`. When finishing the
/// document, we need to take ownership of many of the items in `GlobalObjects` in order to
/// prevent having to clone them. However, the problem is that we cannot easily take ownership
/// of them, because they are part of the SerializeContext. Because of this, what we
/// do is that we `std::mem::replace` the elements step by step and then serialize them.
/// The `MaybeTaken` struct helps us to ensure that once we have taken a value, we do not
/// accidentally attempt to write/read it again.
pub(crate) struct MaybeTaken<T>(Option<T>);

impl<T> MaybeTaken<T> {
    pub(crate) fn new(item: T) -> Self {
        Self(Some(item))
    }

    pub(crate) fn is_taken(&self) -> bool {
        self.0.is_none()
    }
}

impl<T> MaybeTaken<T> {
    #[track_caller]
    pub(crate) fn take(&mut self) -> T {
        self.0.take().expect("value was already taken before")
    }
}

impl<T: Default> Default for MaybeTaken<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> Deref for MaybeTaken<T> {
    type Target = T;

    #[track_caller]
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("value was taken")
    }
}

impl<T> DerefMut for MaybeTaken<T> {
    #[track_caller]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("value was taken")
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Pdf2Namespaces {
    /// The ref of the PDF 2.0 standard structure namspace (`https://www.iso.org/pdf2/ssn`).
    pub(crate) ssn_ref: Ref,
    /// The ref of the custom krilla namespace used for role mapping.
    pub(crate) krilla_ref: Ref,
}

#[derive(Default)]
pub(crate) struct GlobalObjects {
    /// All named destinations that have been registered, including a Ref to their destination and
    /// the destination itself.
    // Needs to be pub(crate) because writing of named destinations happens in `ChunkContainer`.
    pub(crate) named_destinations: MaybeTaken<HashMap<Arc<String>, (Ref, XyzDestination)>>,
    /// Indirect references of every AcroForm widget annotation emitted
    /// across all pages. Populated by
    /// [`SerializeContext::register_widget_field`] during annotation
    /// serialisation; consumed in [`ChunkContainer::finish`] to write
    /// the document catalogue's `/AcroForm /Fields` array (ISO 32000-2
    /// §12.7.3).
    pub(crate) widget_fields: MaybeTaken<Vec<Ref>>,
    /// Indirect ref of the document-level Type1 Helvetica font dict
    /// used by AcroForm widget appearance streams. `None` until the
    /// first widget appearance stream requests it via
    /// [`SerializeContext::standard_helvetica_ref`]; serialised into
    /// [`ChunkContainer::fonts`] at the moment of allocation, so no
    /// take-once semantics are needed.
    pub(crate) standard_helvetica_font: Option<Ref>,
    /// A map from fonts to font container.
    font_map: MaybeTaken<IndexMap<Font, Rc<RefCell<FontContainer>>>>,
    /// All XYZ destinations used in the document. The reason we need to store them
    /// separately is that we can only serialize them in the very end, once all pages
    /// have been written, so that we know the Ref of the page they belong to.
    xyz_destinations: MaybeTaken<Vec<(Ref, XyzDestination)>>,
    /// All pages and their corresponding chunks. Similarly to destinations, they need
    /// to be written in the very end, because pages might contain annotations which in turn
    /// depend on future pages (not written yet), so pages must also only be written in the
    /// very end.
    pages: MaybeTaken<Vec<(Ref, InternalPage)>>,
    /// Stores the struct parent elements.
    struct_parents: MaybeTaken<Vec<StructParentElement>>,
    /// Stores the document outline.
    outline: MaybeTaken<Option<Outline>>,
    /// Stores the tag tree.
    tag_tree: MaybeTaken<Option<TagTree>>,
    /// Stores the association of the names of embedded files to their refs,
    /// for the catalog dictionary.
    /// File-name → (indirect ref of the FileSpec dict, attachment
    /// position) for every embedded file registered via
    /// `Document::embed_file`. The map is alphabetically sorted by
    /// name (BTreeMap order matches the PDF name-tree sort order
    /// per ISO 32000-1 §7.9.6). The position drives the
    /// catalogue's `/AF` array partitioning: `EmbedLocation::Before`
    /// entries are emitted ahead of `EmbedLocation::After` entries,
    /// preserving alphabetical order within each partition.
    pub(crate) embedded_files: MaybeTaken<BTreeMap<String, (Ref, crate::embed::EmbedLocation)>>,
    /// A list of custom headings numbers used in the document.
    pub(crate) custom_heading_roles: BTreeSet<NonZeroU16>,
    /// Optional content groups (layers) registered via
    /// [`crate::Document::add_layer`]. Each entry carries the
    /// caller-supplied [`crate::optional_content::Layer`] descriptor
    /// plus the indirect [`Ref`] krilla pre-allocated for the
    /// underlying `/OCG` object. Taken at finalise time when the
    /// catalogue's `/OCProperties` dict is written.
    pub(crate) layers: MaybeTaken<Vec<LayerRecord>>,
    /// External structure namespaces (`/NS`) registered via
    /// [`crate::Document::register_namespace`]. Each entry pairs
    /// the caller-supplied namespace URI (MathML, HTML 4, PDF Math,
    /// …) with the indirect ref of the `Namespace` dictionary
    /// krilla writes at finalise time. PDF 2.0 only — pre-2.0
    /// documents emit nothing here. Taken at the same point
    /// `serialize_tag_tree` emits the catalogue's `/Namespaces`
    /// array.
    pub(crate) custom_namespaces: MaybeTaken<Vec<CustomNamespaceRecord>>,
    /// The context tracking all of the pdfs and their pages that have been inserted.
    #[cfg(feature = "pdf")]
    pub(crate) pdf_ctx: MaybeTaken<PdfSerializerContext>,
}

/// A registered optional content group together with the indirect ref
/// of the `/OCG` dictionary that will represent it in the final PDF.
#[derive(Debug, Clone)]
pub(crate) struct LayerRecord {
    pub(crate) layer: crate::optional_content::Layer,
    pub(crate) ref_: Ref,
}

/// A caller-registered external namespace plus the indirect ref of
/// the `Namespace` dictionary that holds its URI in the final PDF.
#[derive(Debug, Clone)]
pub(crate) struct CustomNamespaceRecord {
    pub(crate) uri: String,
    pub(crate) ref_: Ref,
}

impl GlobalObjects {
    pub(crate) fn assert_all_taken(&self) {
        assert!(self.named_destinations.is_taken());
        assert!(self.widget_fields.is_taken());
        assert!(self.font_map.is_taken());
        assert!(self.xyz_destinations.is_taken());
        assert!(self.pages.is_taken());
        assert!(self.struct_parents.is_taken());
        assert!(self.outline.is_taken());
        assert!(self.tag_tree.is_taken());
        assert!(self.embedded_files.is_taken());
        assert!(self.layers.is_taken());
        assert!(self.custom_namespaces.is_taken());
        #[cfg(feature = "pdf")]
        assert!(self.pdf_ctx.is_taken());
    }
}

pub(crate) trait Cacheable: SipHashable {
    fn serialize(
        self,
        sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    );
}
