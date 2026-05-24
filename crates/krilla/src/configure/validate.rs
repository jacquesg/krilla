//! Exporting with a specific PDF conformance level.
//!
//! PDF defines a number of additional conformance levels that restrict the features of PDF that
//! can be used to a specific subset.
//!
//! You can use a [`Validator`] by creating a corresponding [`Configuration`]
//! you want to build the document with. There are three important aspects that play into this:
//! - krilla will internally write the file in a way that conforms to the given standard, i.e.
//!   by settings appropriate metadata. This happens under-the-hood and is completely abstracted
//!   away from the user.
//! - For aspects that are out of control of krilla and dependent on the input, krilla will perform
//!   a validation that the input is compatible with the standard. krilla will record all violations,
//!   and when calling `document.finish()`, in case there is at least one violation, krilla will
//!   return them as an error, instead of returning the finished document. See [`ValidationError`].
//! - Finally, some standards have requirements that cannot possibly be validated by krilla, as
//!   they are semantic in nature. It is upon you, as a user of that library, to ensure that those
//!   requirements are fulfilled. Therefore, while krilla tries to make it as easy as possible
//!   to generate compliant PDFs, it is still highly recommended that you familiarize yourself
//!   with the PDF specification as well as the specifications for the substandards. This is
//!   especially true for standards related to universal accessibility.
//!   
//!  You can find some requirements below **Requirements** for each [`Validator`].
//!
//! [`Configuration`]: crate::configure::Configuration

use std::collections::HashMap;
use std::fmt::Debug;

use pdf_writer::types::OutputIntentSubtype;
use xmp_writer::pdfa::PdfAExtSchemasWriter;
use xmp_writer::XmpWriter;

use crate::color::devicen::DeviceNSpace;
use crate::color::separation::SeparationColorant;
use crate::color::separation::SeparationSpace;
use crate::color::RegularColor;
use crate::configure::PdfVersion;
use crate::interchange::embed::EmbedError;
use crate::surface::Location;
use crate::text::Font;
use crate::text::GlyphId;

/// An error that occurred during validation/
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValidationError {
    /// There was a string that was longer than the maximum allowed length (32767).
    ///
    /// Can for example occur if you set a title or an author that is longer than
    /// the given length.
    TooLongString,
    /// There was a name that was longer than the maximum allowed length (127).
    ///
    /// Can for example occur if the font name is too long.
    TooLongName,
    /// There was an array that was longer than the maximum allowed length (8191).
    /// Can only occur for PDF 1.4.
    ///
    /// Can for example occur if a text too long was written.
    TooLongArray,
    /// There was a dictionary with more entries than the maximum allowed (4095).
    /// Can only occur for PDF 1.4.
    ///
    /// Can for example occur if too many annotations are added to a page.
    TooLongDictionary,
    /// There was a float that is higher than the maximum allowed (32767).
    /// Can only occur for PDF 1.4.
    TooLargeFloat,
    /// The PDF exceeds the upper limit for indirect objects (8388607).
    ///
    /// Occurs if the PDF is simply too long.
    TooManyIndirectObjects,
    /// The PDF contains a content stream that exceeds maximum allowed q/Q nesting level (28).
    ///
    /// Can only occur if the user stacks many clip paths.
    TooHighQNestingLevel,
    /// The PDF contains PostScript code, which is forbidden by some export formats.
    ///
    /// Occurs if a gradient with spread method `Repeat`/`Reflect` or a sweep gradient was used.
    ContainsPostScript(Option<Location>),
    /// No CMYK ICC profile was provided, even though one is necessary.
    ///
    /// Occurs if the export format requires a device-independent color representation,
    /// and a CMYK color was used in the document.
    MissingCMYKProfile,
    /// The same Separation colorant was used with multiple different fallback colors.
    ///
    /// Occurs if the user specified multiple Separation color spaces with the same colorant but a different fallback color.
    InconsistentSeparationFallback(SeparationColorant),
    /// The `.notdef` glyph was used, which is forbidden by some export formats.
    ///
    /// Can occur if a glyph could not be found in the font for a corresponding codepoint
    /// in the input text, or if it was explicitly mapped that way.
    ///
    /// The third argument contains the text range of the glyph.
    ContainsNotDefGlyph(Font, Option<Location>, String),
    /// A glyph was mapped to no codepoint at all, which is forbidden by some
    /// standards.
    NoCodepointMapping(Font, GlyphId, Option<Location>),
    /// A glyph was mapped either to the codepoint 0x0, 0xFEFF or 0xFFFE, which
    /// is forbidden by some standards.
    ///
    /// Can occur if those codepoints appeared in the input text, or were
    /// explicitly mapped to that glyph.
    InvalidCodepointMapping(Font, GlyphId, char, Option<Location>),
    /// A glyph was mapped to a codepoint in the Unicode private use area, which is forbidden
    /// by some standards, like for example PDF/A-2a.
    // Note that the standard doesn't explicitly forbid it, but instead requires an ActualText
    // attribute to be present. But we just completely forbid it, for simplicity.
    UnicodePrivateArea(Font, GlyphId, char, Option<Location>),
    /// A font has a license that requires explicit permission of the legal owner for embedding
    /// but the standard requires font programs to be legally embeddable for universal rendering.
    RestrictedLicense(Font),
    /// No document language was set via the metadata, even though it is required
    /// by the standard.
    NoDocumentLanguage,
    /// No title was provided for the document, even though it is required by
    /// the standard.
    NoDocumentTitle,
    /// A figure or formula is missing an alt text.
    MissingAltText(Option<Location>),
    /// A heading is missing a title.
    MissingHeadingTitle,
    /// The document does not contain an outline.
    MissingDocumentOutline,
    /// An annotation is missing an alt text.
    MissingAnnotationAltText(Option<Location>),
    /// The date of the document is missing.
    // We need this because for some standards we need to add the
    // xmp:History attribute.
    MissingDocumentDate,
    /// The PDF contains transparency, which is forbidden by some standards (e.g. PDF/A-1).
    Transparency(Option<Location>),
    /// The PDF contains an image with `interpolate` set to `true`.
    ImageInterpolation(Option<Location>),
    /// The PDF contains an embedded file.
    EmbeddedFile(EmbedError, Option<Location>),
    /// The PDF contains no tagging.
    MissingTagging,
    /// The PDF contains another embedded PDF.
    ///
    /// This is currently forbidden in validated export because we cannot manually verify
    /// whether the file actually fulfills all the criteria for the export mode.
    EmbeddedPDF(Option<Location>),
    /// A feature only available in a later PDF version was required.
    RequiresNewerPdfVersion(VersionedFeature, Option<Location>),
    /// The PDF contains an RGB color, which is forbidden by PDF/X-1a.
    ///
    /// Occurs if an RGB color was used in fills, strokes, gradients, images,
    /// or separation fallback colors when exporting to PDF/X-1a. Grayscale
    /// colors are permitted.
    ContainsRgb(Option<Location>),
    /// The PDF contains a DeviceN colour space, which is forbidden by
    /// PDF/A-1 (ISO 19005-1 §6.2.4). PDF/A-2 onward and every PDF/X
    /// profile admit DeviceN.
    ///
    /// Occurs if a DeviceN colour was used in fills, strokes,
    /// gradients, or shadings when exporting to PDF/A-1a / PDF/A-1b.
    ContainsDeviceN(Option<Location>),
    /// A gradient's stops are not all in the same color space.
    ///
    /// Occurs if the [`Stop`](crate::paint::Stop)s supplied to a
    /// [`LinearGradient`](crate::paint::LinearGradient),
    /// [`RadialGradient`](crate::paint::RadialGradient), or
    /// [`SweepGradient`](crate::paint::SweepGradient) resolve to different
    /// color spaces. krilla normalises the stops to the first stop's color
    /// space when this happens.
    MixedGradientColorSpaces(Option<Location>),
    /// A page is missing both a TrimBox and an ArtBox, which is required by
    /// PDF/X.
    ///
    /// The first field is the zero-based index of the offending page.
    MissingTrimOrArtBox(usize, Option<Location>),
    /// The PDF contains annotations which are forbidden by PDF/X-1a.
    ///
    /// PDF/X-1a only allows TrapNet and PrinterMark annotations, neither of
    /// which is supported by krilla.
    ContainsAnnotation(Option<Location>),
    /// An external output profile reference was provided for a validator
    /// other than PDF/X-4p or PDF/X-6p, or none was provided when one of
    /// those validators is active.
    ExternalOutputProfileRequiresX4P,
    /// The document was configured to be encrypted (via
    /// [`SerializeSettings::encryption`](crate::SerializeSettings::encryption))
    /// while an archival or print validator that forbids the
    /// `/Encrypt` dictionary is also active.
    ///
    /// PDF/A (ISO 19005, every profile) and PDF/X (ISO 15930,
    /// every profile) both reject encrypted files because a
    /// conforming long-term-preservation or print-exchange document
    /// must be readable without a password by anyone (PDF/A
    /// preservation tooling; PDF/X RIPs). The two settings are
    /// therefore mutually exclusive: pick one.
    ContainsEncryption,
}

/// Features that may require a later PDF version than the current one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionedFeature {
    /// Tabbing through the document according to the structure order.
    StructureOrderTabbing,
    /// Header and footer artifact subtypes.
    HeaderFooterArtifactSubtypes,
    /// Scope attribute for table header cells.
    TableHeaderScope,
}

impl VersionedFeature {
    /// Get the minimum PDF version required for this feature.
    pub fn minimum_pdf_version(&self) -> PdfVersion {
        match self {
            VersionedFeature::StructureOrderTabbing => PdfVersion::Pdf15,
            VersionedFeature::HeaderFooterArtifactSubtypes => PdfVersion::Pdf17,
            VersionedFeature::TableHeaderScope => PdfVersion::Pdf15,
        }
    }
}

/// Collection of validators with at most one validator for each standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct Validators {
    a: Option<Archival>,
    ua: Option<Accessibility>,
    pdfx: Option<Pdfx>,
}

impl Validators {
    /// Returns a filtered `Validators` containing only validators that prohibit the given error,
    /// or `None` if no validator prohibits it.
    pub fn prohibits(self, error: &ValidationError) -> Option<Self> {
        let a = self.a.filter(|v| v.prohibits(error));
        let ua = self.ua.filter(|v| v.prohibits(error));
        let pdfx = self.pdfx.filter(|v| v.prohibits(error));

        let any = a.is_some() || ua.is_some() || pdfx.is_some();
        any.then_some(Self { a, ua, pdfx })
    }

    /// Returns `true` if no validators are set.
    pub fn is_empty(self) -> bool {
        self.a.is_none() && self.ua.is_none() && self.pdfx.is_none()
    }

    /// Returns the number of set validators.
    pub fn len(self) -> usize {
        (if self.a.is_some() { 1 } else { 0 })
            + (if self.ua.is_some() { 1 } else { 0 })
            + (if self.pdfx.is_some() { 1 } else { 0 })
    }

    /// Returns the PDF/A validator, if set.
    pub fn archival(self) -> Option<Archival> {
        self.a
    }

    /// Returns the PDF/UA accessibility validator, if set.
    pub fn accessibility(self) -> Option<Accessibility> {
        self.ua
    }

    /// Returns the PDF/X validator, if set.
    pub fn pdfx(self) -> Option<Pdfx> {
        self.pdfx
    }

    /// Whether the font must supply valid Unicode code points for each of the
    /// drawn glyphs.
    pub(crate) fn requires_codepoint_mappings(self) -> bool {
        self.into_iter().any(Validator::requires_codepoint_mappings)
    }

    /// Force the `DisplayDocTitle` flag set.
    pub(crate) fn requires_display_doc_title(self) -> bool {
        self.ua
            .is_some_and(Accessibility::requires_display_doc_title)
    }

    /// Force sRGB profiles for `DeviceGray` and `DeviceRgb` colorspaces.
    pub(crate) fn requires_no_device_cs(self) -> bool {
        self.a.is_some_and(Archival::requires_no_device_cs)
    }

    /// Force the `Print` flag set and the `Hidden`, `Invisible`,
    /// `ToggleNoView`, and `NoView` flags unset.
    pub(crate) fn requires_annotation_flags(self) -> bool {
        self.a.is_some_and(Archival::requires_annotation_flags)
    }

    /// Whether Tagged PDF must be enabled.
    pub(crate) fn requires_tagging(self) -> bool {
        self.into_iter().any(Validator::requires_tagging)
    }

    /// Whether XMP metadata must be written.
    pub(crate) fn requires_xmp_metadata(self) -> bool {
        self.into_iter().any(Validator::requires_xmp_metadata)
    }

    /// Whether any extension schemata should be descibed using the "pdfaSchema"
    /// namespace.
    pub(crate) fn requires_xmp_metadata_extension_schema(self) -> bool {
        self.a
            .is_some_and(Archival::requires_xmp_metadata_extension_schema)
    }

    /// Whether the `instanceID` field is allowed in XMP.
    pub(crate) fn prohibits_instance_id_in_xmp_metadata(self) -> bool {
        self.a
            .is_some_and(Archival::prohibits_instance_id_in_xmp_metadata)
    }

    /// Whether the xmpMM:History entry is required.
    pub(crate) fn requires_file_provenance_information(self) -> bool {
        self.a
            .is_some_and(Archival::requires_file_provenance_information)
    }

    /// Whether the `/Info` dictionary is allowed in the file trailer.
    pub(crate) fn prohibits_info_dict(self) -> bool {
        self.a.is_some_and(Archival::prohibits_info_dict)
    }

    /// Whether a non-printable file header is mandatory.
    pub(crate) fn requires_binary_header(self) -> bool {
        self.a.is_some_and(Archival::requires_binary_header)
    }

    /// Whether the `EmbeddedFiles` key in the name dictionary of the document
    /// catalog dictionary should be written even if empty.
    pub(crate) fn requires_embedded_files_when_empty(self) -> bool {
        self.a
            .is_some_and(Archival::requires_embedded_files_when_empty)
    }

    /// Whether any of these standards explicitly specifies the `/AF` key.
    ///
    /// The `/AF` key may be supported by the underlying PDF version instead:
    /// Starting at PDF 2.0, the key is specified by ISO 32000 and does not need
    /// to be added by PDF/A.
    pub(crate) fn specifies_associated_files(self) -> bool {
        self.a.is_some_and(Archival::specifies_associated_files)
    }

    /// Returns the dominant single output-intent subtype, if any. Retained
    /// for back-compatibility with single-intent callers; new code should
    /// prefer [`Self::output_intents`].
    #[allow(dead_code)]
    pub(crate) fn output_intent(self) -> Option<OutputIntentSubtype<'static>> {
        self.pdfx
            .map(Pdfx::output_intent)
            .or_else(|| self.a.map(Archival::output_intent))
    }

    /// Every output-intent subtype that the active validators require, in
    /// emission order. For a combined PDF/A + PDF/X document this returns
    /// both `PDFA` and `PDFX`.
    pub(crate) fn output_intents(self) -> Vec<OutputIntentSubtype<'static>> {
        let mut intents = Vec::with_capacity(2);
        if let Some(a) = self.a {
            intents.push(a.output_intent());
        }
        if let Some(pdfx) = self.pdfx {
            intents.push(pdfx.output_intent());
        }
        intents
    }

    /// Whether this set of validators requires CMYK-only colour (PDF/X-1a).
    pub(crate) fn requires_cmyk_only(self) -> bool {
        self.pdfx.is_some_and(Pdfx::requires_cmyk_only)
    }

    /// Whether this set of validators forbids annotations entirely (PDF/X-1a).
    pub(crate) fn forbids_annotations(self) -> bool {
        self.pdfx.is_some_and(Pdfx::forbids_annotations)
    }

    /// Whether every page needs either a TrimBox or an ArtBox (any PDF/X).
    pub(crate) fn requires_trim_or_art_box(self) -> bool {
        self.pdfx.is_some()
    }

    /// Whether the PDF/X `/GTS_PDFXVersion` entry must be emitted.
    pub(crate) fn requires_pdfx_identification(self) -> bool {
        self.pdfx.is_some()
    }

    /// Whether a `Trapped` value must be present in the document info
    /// dictionary (PDF/X mandates it).
    pub(crate) fn requires_trapping_metadata(self) -> bool {
        self.pdfx.is_some()
    }

    /// Whether the caller must supply an `external_output_profile` for the
    /// active PDF/X profile (X-4p / X-6p).
    pub(crate) fn requires_external_output_profile(self) -> bool {
        self.pdfx.is_some_and(Pdfx::requires_external_output_profile)
    }

    /// `GTS_PDFXVersion` string for the active PDF/X profile, if any.
    pub(crate) fn gts_pdfx_version_string(self) -> Option<&'static str> {
        self.pdfx.and_then(Pdfx::gts_pdfx_version_string)
    }

    pub(crate) fn write_xmp(self, xmp: &mut XmpWriter) {
        if self.requires_xmp_metadata_extension_schema() {
            let mut extension_schemas = xmp.extension_schemas();
            if let Some(a) = self.a {
                a.write_xmp_extension_schema_description(&mut extension_schemas);
            }
            if let Some(ua) = self.ua {
                ua.write_xmp_extension_schema_description(&mut extension_schemas);
            }
        }

        if let Some(a) = self.a {
            a.write_xmp(xmp);
        }

        if let Some(ua) = self.ua {
            ua.write_xmp(xmp);
        }

        if let Some(pdfx) = self.pdfx {
            pdfx.write_xmp(xmp);
        }
    }

    /// Returns the maximum PDF version allowed by all active validators.
    pub fn max(self) -> PdfVersion {
        self.a
            .map_or(PdfVersion::MAX, |v| v.max())
            .min(self.ua.map_or(PdfVersion::MAX, |v| v.max()))
            .min(self.pdfx.map_or(PdfVersion::MAX, |v| v.max()))
    }

    /// Returns the minimum PDF version required by all active validators, if any.
    pub fn min(self) -> Option<PdfVersion> {
        self.a
            .and_then(|v| v.min())
            .max(self.ua.and_then(|v| v.min()))
            .max(self.pdfx.and_then(|v| v.min()))
    }
}

impl IntoIterator for Validators {
    type Item = Validator;
    type IntoIter = std::iter::Flatten<std::array::IntoIter<Option<Validator>, 3>>;

    fn into_iter(self) -> Self::IntoIter {
        [
            self.a.map(Validator::A),
            self.ua.map(Validator::Ua),
            self.pdfx.map(Validator::Pdfx),
        ]
        .into_iter()
        .flatten()
    }
}

/// A builder for constructing a [`Validators`] collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct ValidatorsBuilder(Validators);

impl ValidatorsBuilder {
    /// Set a validator, overwriting the current one if the same standard family is already set.
    pub fn set_validator(self, validator: Validator) -> Self {
        match validator {
            Validator::A(a) => self.with_archival_validator(a),
            Validator::Ua(ua) => self.with_accessibility_validator(ua),
            Validator::Pdfx(pdfx) => self.with_pdfx_validator(pdfx),
        }
    }

    /// Set the PDF/A validator, overwriting the current one if already set.
    pub fn with_archival_validator(mut self, archival: Archival) -> Self {
        self.0.a = Some(archival);
        self
    }

    /// Set the PDF/UA accessibility validator, overwriting the current one if already set.
    pub fn with_accessibility_validator(mut self, accessibility: Accessibility) -> Self {
        self.0.ua = Some(accessibility);
        self
    }

    /// Set the PDF/X validator, overwriting the current one if already set.
    pub fn with_pdfx_validator(mut self, pdfx: Pdfx) -> Self {
        self.0.pdfx = Some(pdfx);
        self
    }

    pub(crate) fn finish(self) -> Result<Validators, Validators> {
        let min = self.0.min().unwrap_or(PdfVersion::MIN);
        let max = self.0.max();

        if min > max {
            Err(self.0)
        } else {
            Ok(self.0)
        }
    }
}

/// A PDF validator for a specific conformance standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Validator {
    /// A PDF/A validator.
    A(Archival),
    /// A PDF/UA accessibility validator.
    Ua(Accessibility),
    /// A PDF/X prepress validator.
    Pdfx(Pdfx),
}

impl Validator {
    fn requires_codepoint_mappings(self) -> bool {
        match self {
            Self::A(a) => a.requires_codepoint_mappings(),
            Self::Ua(ua) => ua.requires_codepoint_mappings(),
            Self::Pdfx(pdfx) => pdfx.requires_codepoint_mappings(),
        }
    }

    fn requires_tagging(self) -> bool {
        match self {
            Self::A(a) => a.requires_tagging(),
            Self::Ua(ua) => ua.requires_tagging(),
            Self::Pdfx(pdfx) => pdfx.requires_tagging(),
        }
    }

    fn requires_xmp_metadata(self) -> bool {
        match self {
            Self::A(a) => a.requires_xmp_metadata(),
            Self::Ua(ua) => ua.requires_xmp_metadata(),
            Self::Pdfx(pdfx) => pdfx.requires_xmp_metadata(),
        }
    }

    /// Minimum PDF version required to use this validator, if any.
    pub fn min(self) -> Option<PdfVersion> {
        match self {
            Self::A(a) => a.min(),
            Self::Ua(ua) => ua.min(),
            Self::Pdfx(pdfx) => pdfx.min(),
        }
    }

    /// Maximum PDF version this standard can be used with.
    pub fn max(self) -> PdfVersion {
        match self {
            Self::A(a) => a.max(),
            Self::Ua(ua) => ua.max(),
            Self::Pdfx(pdfx) => pdfx.max(),
        }
    }

    /// Returns a human-readable string representation of the validator.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A(a) => a.as_str(),
            Self::Ua(ua) => ua.as_str(),
            Self::Pdfx(pdfx) => pdfx.as_str(),
        }
    }
}

impl From<Archival> for Validator {
    fn from(a: Archival) -> Self {
        Self::A(a)
    }
}

impl From<Accessibility> for Validator {
    fn from(ua: Accessibility) -> Self {
        Self::Ua(ua)
    }
}

impl From<Pdfx> for Validator {
    fn from(pdfx: Pdfx) -> Self {
        Self::Pdfx(pdfx)
    }
}

/// A PDF/A conformance level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum Archival {
    /// The validator for the PDF/A-1a standard.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-1b.
    /// - You need to follow all requirements outlined in the _Other Notes_ section of the
    ///   [`tagging`] module.
    /// - You need to follow all best practices when using [tags](`crate::interchange::tagging::Tag`), as outlined in the documentation
    ///   of each tag.
    /// - Artifacts such as page numbers, backgrounds, cut marks and color bars should be specified
    ///   correspondingly as artifacts.
    /// - Word boundaries need to be explicitly specified with a space. The same applies to words at
    ///   the end of a line that are not followed by punctuation.
    /// - To the fullest extent possible, the logical structure of the document should be encoded
    ///   correspondingly in the tag tree using appropriate grouping tags.
    /// - Language identifiers used must be valid according to RFC 3066.
    /// - You should provide an alternate text to span content tags, if applicable.
    /// - You should provide the expansion of abbreviations to span content tags, if applicable.
    ///
    /// [`tagging`]: crate::interchange::tagging
    A1_A,
    /// The validator for the PDF/A-1b standard.
    ///
    /// **Requirements**: -
    A1_B,
    /// The validator for the PDF/A-2a standard.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-2b.
    /// - You need to follow all requirements outlined in the _Other Notes_ section of the
    ///   [`tagging`] module.
    /// - You need to follow all best practices when using [tags](`crate::interchange::tagging::Tag`), as outlined in the documentation
    ///   of each tag.
    /// - Artifacts such as page numbers, backgrounds, cut marks and color bars should be specified
    ///   correspondingly as artifacts.
    /// - Word boundaries need to be explicitly specified with a space. The same applies to words at
    ///   the end of a line that are not followed by punctuation.
    /// - To the fullest extent possible, the logical structure of the document should be encoded
    ///   correspondingly in the tag tree using appropriate grouping tags.
    /// - Language identifiers used must be valid according to RFC 3066.
    /// - You should provide an alternate text to span content tags, if applicable.
    /// - You should provide the expansion of abbreviations to span content tags, if applicable.
    ///
    /// [`tagging`]: crate::interchange::tagging
    A2_A,
    /// The validator for the PDF/A-2b standard.
    ///
    /// **Requirements**:
    /// - You should only use fonts that are legally embeddable in a file for unlimited,
    ///   universal rendering.
    A2_B,
    /// The validator for the PDF/A-2u standard.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-2b
    A2_U,
    /// The validator for the PDF/A-3a standard.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-2a
    A3_A,
    /// The validator for the PDF/A-3b standard.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-2b
    A3_B,
    /// The validator for the PDF/A-3u standard.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-2b
    A3_U,
    /// The validator for the PDF/A-4 standard.
    ///
    /// **Requirements**:
    /// - While not required, it's recommended to enable tagging.
    A4,
    /// The validator for the PDF/A-4f standard.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-4
    A4F,
    /// The validator for the PDF/A-4e standard.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-4
    A4E,
}

impl Archival {
    fn prohibits(self, error: &ValidationError) -> bool {
        match (self, error) {
            // Forbidden under all PDF/A-1 profiles.
            (
                Self::A1_A | Self::A1_B,
                ValidationError::TooLongString
                | ValidationError::TooLongName
                | ValidationError::TooLongArray
                | ValidationError::TooLongDictionary
                | ValidationError::TooLargeFloat
                | ValidationError::TooManyIndirectObjects
                | ValidationError::TooHighQNestingLevel
                | ValidationError::ContainsPostScript(_)
                | ValidationError::MissingCMYKProfile
                | ValidationError::RestrictedLicense(_)
                | ValidationError::MissingDocumentDate
                | ValidationError::Transparency(_)
                | ValidationError::ImageInterpolation(_)
                | ValidationError::EmbeddedFile(EmbedError::Existence, _)
                | ValidationError::EmbeddedPDF(_),
            ) => true,
            // Allowed under all PDF/A-1 profiles.
            (
                Self::A1_A | Self::A1_B,
                ValidationError::InconsistentSeparationFallback(_)
                | ValidationError::InvalidCodepointMapping(_, _, _, _)
                | ValidationError::UnicodePrivateArea(_, _, _, _)
                | ValidationError::NoDocumentTitle
                | ValidationError::MissingHeadingTitle
                | ValidationError::MissingDocumentOutline
                | ValidationError::EmbeddedFile(_, _)
                | ValidationError::RequiresNewerPdfVersion(
                    VersionedFeature::HeaderFooterArtifactSubtypes
                    | VersionedFeature::StructureOrderTabbing
                    | VersionedFeature::TableHeaderScope,
                    _,
                ),
            ) => false,
            // Forbidden under PDF/A-1a but allowed under PDF/A-1b.
            (
                Self::A1_A | Self::A1_B,
                ValidationError::ContainsNotDefGlyph(_, _, _)
                | ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::NoDocumentLanguage
                | ValidationError::MissingAltText(_)
                | ValidationError::MissingAnnotationAltText(_)
                | ValidationError::MissingTagging,
            ) => self == Self::A1_A,

            // Forbidden under all PDF/A-2 and PDF/A-3 profiles.
            (
                Self::A2_A | Self::A2_B | Self::A2_U | Self::A3_A | Self::A3_B | Self::A3_U,
                ValidationError::TooLongString
                | ValidationError::TooLongName
                | ValidationError::TooManyIndirectObjects
                | ValidationError::TooHighQNestingLevel
                | ValidationError::ContainsPostScript(_)
                | ValidationError::MissingCMYKProfile
                | ValidationError::InconsistentSeparationFallback(_)
                | ValidationError::ContainsNotDefGlyph(_, _, _)
                | ValidationError::RestrictedLicense(_)
                | ValidationError::MissingDocumentDate
                | ValidationError::ImageInterpolation(_)
                | ValidationError::EmbeddedPDF(_),
            ) => true,
            // Allowed under all PDF/A-2 and PDF/A-3 profiles.
            (
                Self::A2_A | Self::A2_B | Self::A2_U | Self::A3_A | Self::A3_B | Self::A3_U,
                ValidationError::TooLongArray
                | ValidationError::TooLongDictionary
                | ValidationError::TooLargeFloat
                | ValidationError::NoDocumentTitle
                | ValidationError::Transparency(_)
                | ValidationError::MissingHeadingTitle
                | ValidationError::MissingDocumentOutline
                | ValidationError::RequiresNewerPdfVersion(
                    VersionedFeature::HeaderFooterArtifactSubtypes
                    | VersionedFeature::StructureOrderTabbing
                    | VersionedFeature::TableHeaderScope,
                    _,
                ),
            ) => false,
            // Forbidden under PDF/A-2 but allowed under PDF/A-3.
            (
                Self::A2_A | Self::A2_B | Self::A2_U | Self::A3_A | Self::A3_B | Self::A3_U,
                ValidationError::EmbeddedFile(EmbedError::Existence, _),
            ) => self == Self::A2_A || self == Self::A2_B || self == Self::A2_U,
            // Forbidden under PDF/A-3 but allowed under PDF/A-2.
            (
                Self::A2_A | Self::A2_B | Self::A2_U | Self::A3_A | Self::A3_B | Self::A3_U,
                ValidationError::EmbeddedFile(
                    EmbedError::MissingDate
                    | EmbedError::MissingDescription
                    | EmbedError::MissingMimeType,
                    _,
                ),
            ) => self == Self::A3_A || self == Self::A3_B || self == Self::A3_U,
            // Forbidden under PDF/A-2 and PDF/A-3 accessible profiles.
            (
                Self::A2_A | Self::A2_B | Self::A2_U | Self::A3_A | Self::A3_B | Self::A3_U,
                ValidationError::UnicodePrivateArea(_, _, _, _)
                | ValidationError::NoDocumentLanguage
                | ValidationError::MissingAltText(_)
                | ValidationError::MissingAnnotationAltText(_)
                | ValidationError::MissingTagging,
            ) => self == Self::A2_A || self == Self::A3_A,
            // Forbidden under PDF/A-2 and PDF/A-3 accessible and Unicode profiles.
            (
                Self::A2_A | Self::A2_B | Self::A2_U | Self::A3_A | Self::A3_B | Self::A3_U,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _),
            ) => {
                self == Self::A2_A || self == Self::A2_U || self == Self::A3_A || self == Self::A3_U
            }

            // Forbidden under all PDF/A-4 profiles.
            (
                Self::A4 | Self::A4F | Self::A4E,
                ValidationError::MissingCMYKProfile
                | ValidationError::InconsistentSeparationFallback(_)
                | ValidationError::ContainsNotDefGlyph(_, _, _)
                | ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _)
                | ValidationError::UnicodePrivateArea(_, _, _, _)
                | ValidationError::RestrictedLicense(_)
                | ValidationError::MissingDocumentDate
                | ValidationError::ImageInterpolation(_)
                | ValidationError::EmbeddedPDF(_),
            ) => true,
            // Allowed under all PDF/A-4 profiles.
            (
                Self::A4 | Self::A4F | Self::A4E,
                ValidationError::TooLongString
                | ValidationError::TooLongName
                | ValidationError::TooLongArray
                | ValidationError::TooLongDictionary
                | ValidationError::TooLargeFloat
                | ValidationError::TooManyIndirectObjects
                | ValidationError::TooHighQNestingLevel
                | ValidationError::ContainsPostScript(_)
                | ValidationError::NoDocumentLanguage
                | ValidationError::NoDocumentTitle
                | ValidationError::MissingAltText(_)
                | ValidationError::MissingHeadingTitle
                | ValidationError::MissingDocumentOutline
                | ValidationError::MissingAnnotationAltText(_)
                | ValidationError::Transparency(_)
                | ValidationError::EmbeddedFile(
                    EmbedError::MissingDate | EmbedError::MissingMimeType,
                    _,
                )
                | ValidationError::MissingTagging
                | ValidationError::RequiresNewerPdfVersion(
                    VersionedFeature::HeaderFooterArtifactSubtypes
                    | VersionedFeature::StructureOrderTabbing
                    | VersionedFeature::TableHeaderScope,
                    _,
                ),
            ) => false,
            // Forbidden under PDF/A-4 but allowed under other PDF/A-4 profiles.
            (
                Self::A4 | Self::A4F | Self::A4E,
                ValidationError::EmbeddedFile(EmbedError::Existence, _),
            ) => self == Self::A4,
            // Allowed under PDF/A-4 but forbidden under other profiles.
            (
                Self::A4 | Self::A4F | Self::A4E,
                ValidationError::EmbeddedFile(EmbedError::MissingDescription, _),
            ) => self == Self::A4,

            // ISO 19005-1 §6.2.4 forbids DeviceN under PDF/A-1; every
            // subsequent revision (A-2 onward) admits it. Carry this
            // as a single PDF/A-1-targeted rule before the catch-all
            // PDF/X-specific bucket below.
            (Self::A1_A | Self::A1_B, ValidationError::ContainsDeviceN(_)) => true,
            (_, ValidationError::ContainsDeviceN(_)) => false,

            // PDF/X-specific errors: PDF/A is silent on them, so allow.
            (
                _,
                ValidationError::ContainsRgb(_)
                | ValidationError::MissingTrimOrArtBox(_, _)
                | ValidationError::ContainsAnnotation(_),
            ) => false,
            // Krilla-internal soundness and PDF/X-specific configuration
            // checks: surface under every PDF/A profile.
            (
                _,
                ValidationError::MixedGradientColorSpaces(_)
                | ValidationError::ExternalOutputProfileRequiresX4P,
            ) => true,
            // ISO 19005 (every PDF/A revision) forbids the `/Encrypt`
            // dictionary — a conformant archival document must be
            // openable without a password by future preservation
            // tooling.
            (_, ValidationError::ContainsEncryption) => true,
        }
    }

    fn requires_codepoint_mappings(self) -> bool {
        match self {
            Self::A1_A
            | Self::A2_A
            | Self::A2_U
            | Self::A3_A
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => true,
            Self::A1_B | Self::A2_B | Self::A3_B => false,
        }
    }

    fn requires_no_device_cs(self) -> bool {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => true,
        }
    }

    fn requires_annotation_flags(self) -> bool {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => true,
        }
    }

    fn requires_tagging(self) -> bool {
        match self {
            Self::A1_A | Self::A2_A | Self::A3_A => true,
            Self::A1_B
            | Self::A2_B
            | Self::A2_U
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => false,
        }
    }

    fn requires_xmp_metadata(self) -> bool {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => true,
        }
    }

    fn requires_xmp_metadata_extension_schema(self) -> bool {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U => true,
            // Clause 6.7.2.3 of PDF/A-4 recommends ("should") a RELAX NG
            // definition of its metadata contents to be embedded as an
            // associated file. It no longer uses the inline schema definition
            // using the "pdfaSchema" namespaces for extension schemata.
            Self::A4 | Self::A4F | Self::A4E => false,
        }
    }

    fn prohibits_instance_id_in_xmp_metadata(self) -> bool {
        match self {
            Self::A1_A | Self::A1_B => true,
            Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => false,
        }
    }

    fn requires_file_provenance_information(self) -> bool {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => true,
        }
    }

    fn prohibits_info_dict(self) -> bool {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U => false,
            Self::A4 | Self::A4F | Self::A4E => true,
        }
    }

    fn requires_binary_header(self) -> bool {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => true,
        }
    }

    fn requires_embedded_files_when_empty(self) -> bool {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4E => false,
            Self::A4F => true,
        }
    }

    /// Whether this standard explicitly specifies the `/AF` key.
    ///
    /// The `/AF` key may be supported by the underlying PDF version instead:
    /// Starting at PDF 2.0, the key is specified by ISO 32000 and does not need
    /// to be added by PDF/A.
    fn specifies_associated_files(self) -> bool {
        match self {
            Self::A3_A | Self::A3_B | Self::A3_U => true,
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A4
            | Self::A4F
            | Self::A4E => false,
        }
    }

    fn output_intent(self) -> OutputIntentSubtype<'static> {
        match self {
            Self::A1_A
            | Self::A1_B
            | Self::A2_A
            | Self::A2_B
            | Self::A2_U
            | Self::A3_A
            | Self::A3_B
            | Self::A3_U
            | Self::A4
            | Self::A4F
            | Self::A4E => OutputIntentSubtype::PDFA,
        }
    }

    fn write_xmp(self, xmp: &mut XmpWriter) {
        match self {
            Self::A1_A => {
                xmp.pdfa_part(1);
                xmp.pdfa_conformance("A");
            }
            Self::A1_B => {
                xmp.pdfa_part(1);
                xmp.pdfa_conformance("B");
            }
            Self::A2_A => {
                xmp.pdfa_part(2);
                xmp.pdfa_conformance("A");
            }
            Self::A2_B => {
                xmp.pdfa_part(2);
                xmp.pdfa_conformance("B");
            }
            Self::A2_U => {
                xmp.pdfa_part(2);
                xmp.pdfa_conformance("U");
            }
            Self::A3_A => {
                xmp.pdfa_part(3);
                xmp.pdfa_conformance("A");
            }
            Self::A3_B => {
                xmp.pdfa_part(3);
                xmp.pdfa_conformance("B");
            }
            Self::A3_U => {
                xmp.pdfa_part(3);
                xmp.pdfa_conformance("U");
            }
            Self::A4 => {
                xmp.pdfa_part(4);
                xmp.pdfa_rev(2020);
            }
            Self::A4F => {
                xmp.pdfa_part(4);
                xmp.pdfa_rev(2020);
                xmp.pdfa_conformance("F");
            }
            Self::A4E => {
                xmp.pdfa_part(4);
                xmp.pdfa_rev(2020);
                xmp.pdfa_conformance("E");
            }
        }
    }

    fn write_xmp_extension_schema_description(
        self,
        extension_schemas: &mut PdfAExtSchemasWriter<'_, '_>,
    ) {
        if !self.requires_xmp_metadata_extension_schema() {
            return;
        }

        extension_schemas
            .xmp_media_management()
            .properties()
            .describe_instance_id();
        extension_schemas.pdf().properties().describe_all();
    }

    /// Returns a human-readable string representation of the conformance level.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A1_A => "PDF/A-1a",
            Self::A1_B => "PDF/A-1b",
            Self::A2_A => "PDF/A-2a",
            Self::A2_B => "PDF/A-2b",
            Self::A2_U => "PDF/A-2u",
            Self::A3_A => "PDF/A-3a",
            Self::A3_B => "PDF/A-3b",
            Self::A3_U => "PDF/A-3u",
            Self::A4 => "PDF/A-4",
            Self::A4F => "PDF/A-4f",
            Self::A4E => "PDF/A-4e",
        }
    }

    /// Minimum PDF version required to use this standard, if any.
    pub const fn min(self) -> Option<PdfVersion> {
        match self {
            // PDF/A-1 through 3 require XMP `/Metadata` streams, which require PDF 1.4.
            Self::A1_A | Self::A1_B => Some(PdfVersion::Pdf14),
            Self::A2_A | Self::A2_B | Self::A2_U => Some(PdfVersion::Pdf14),
            Self::A3_A | Self::A3_B | Self::A3_U => Some(PdfVersion::Pdf14),
            Self::A4 | Self::A4F | Self::A4E => Some(PdfVersion::Pdf20),
        }
    }

    /// Maximum PDF version this standard can be used with.
    pub const fn max(self) -> PdfVersion {
        match self {
            Self::A1_A | Self::A1_B => PdfVersion::Pdf14,
            Self::A2_A | Self::A2_B | Self::A2_U | Self::A3_A | Self::A3_B | Self::A3_U => {
                PdfVersion::Pdf17
            }
            Self::A4 | Self::A4F | Self::A4E => PdfVersion::Pdf20,
        }
    }
}

/// A validator for exporting PDF documents to a specific subset of PDF.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum Accessibility {
    /// The validator for the PDF/UA-1 standard.
    ///
    /// **Requirements**:
    ///
    /// General:
    /// - All real content should be tagged accordingly.
    /// - All artifacts should be marked accordingly.
    /// - The tag tree should reflect the logical reading order of the
    ///   document.
    /// - Information should not be conveyed by contrast, color, format
    ///   or layout.
    /// - All "best practice" notes in [`TagKind`] need to be complied with.
    ///
    /// Text:
    /// - You should make use of the `Alt`, `ActualText`, `Lang` and `Expansion` attributes
    ///   whenever possible.
    /// - Usually, you can provide an empty string as `Lang` to indicate that a language is unknown.
    ///   You should not do that in PDF/UA.
    /// - Stretchable characters (such as brackets, which often consist of several glyphs)
    ///   should be marked accordingly with `ActualText`.
    ///
    ///  Graphics:
    /// - Graphics should be tagged as figures (unless they are an artifact).
    /// - Graphics need to be followed by a caption.
    /// - Graphics that possess semantic values only in combination with other graphics
    ///   should be tagged with a single Figure tag for each figure.
    /// - If a more accessible representation exists, it should be used over graphics.
    ///
    /// Headings:
    /// - Headings should be tagged as such.
    /// - For not strongly structured documents, H1 should be the first
    ///   heading.
    ///
    /// Tables:
    /// - Tables should include headers and be tagged accordingly.
    /// - Tables should only be used to represent content within logical row/column relationship.
    ///
    /// Lists:
    /// - List items should be tagged with Li tags, if necessary also with
    ///   Lbl and LBody tags.
    /// - Lists should only be used when the content is intended to be read
    ///   as a list.
    ///
    /// Mathematical expressions:
    /// - All mathematical expressions should be enclosed with
    ///   a `Formula` tag.
    ///
    /// Headers and footers:
    /// - Headers and footers should be marked as corresponding
    ///   artifacts.
    ///
    /// Notes and references:
    /// - Footnotes, endnotes, note labels and references should be
    ///   tagged accordingly and use tagged annotations.
    /// - Footnotes and end notes should use the `Note` tag.
    ///
    /// Navigation:
    /// - The document must contain an outline, and it should reflect
    ///   the reading order of the document.
    /// - Page labels should be semantically appropriate.
    ///
    /// Annotations:
    /// - Annotations should be present in the tag tree in the correct
    ///   reading order.
    ///
    /// Fonts:
    /// - You should only use fonts that are legally embeddable in a file for unlimited,
    ///   universal rendering.
    ///
    /// [`TagKind`]: crate::interchange::tagging::TagKind
    UA1,
    /// The validator for the PDF/UA-2 standard (ISO 14289-2:2024).
    ///
    /// PDF/UA-2 builds on ISO 32000-2 (PDF 2.0) and normatively references
    /// the PDF Association's Well-Tagged PDF (WTPDF) profile. Every WTPDF
    /// requirement applies, plus the universal-accessibility additions
    /// documented below. Use [`WTPDF`](Self::WTPDF) instead when you need a
    /// well-tagged PDF 2.0 document without the stricter accessibility
    /// metadata requirements.
    ///
    /// **Requirements**:
    ///
    /// All requirements of [`WTPDF`](Self::WTPDF), plus:
    ///
    /// General:
    /// - Information should not be conveyed by contrast, colour, format,
    ///   or layout alone.
    /// - All "best practice" notes in [`TagKind`] need to be complied with.
    ///
    /// Text:
    /// - You should make use of the `Alt`, `ActualText`, `Lang` and
    ///   `Expansion` attributes whenever possible.
    /// - You should not provide an empty string as `Lang`.
    /// - Stretchable characters (such as brackets, which often consist of
    ///   several glyphs) should be marked accordingly with `ActualText`.
    ///
    /// Graphics:
    /// - Graphics should be tagged as figures (unless they are an artifact).
    /// - Graphics need to be followed by a caption.
    /// - Graphics that possess semantic value only in combination with other
    ///   graphics should be tagged with a single Figure tag for each figure.
    /// - If a more accessible representation exists, it should be used over
    ///   graphics.
    ///
    /// Headings:
    /// - Headings should be tagged as such.
    /// - For not strongly structured documents, H1 should be the first
    ///   heading.
    ///
    /// Navigation:
    /// - The document must contain an outline, and it should reflect
    ///   the reading order of the document.
    /// - Page labels should be semantically appropriate.
    ///
    /// Annotations:
    /// - Annotations should be present in the tag tree in the correct
    ///   reading order.
    /// - Every annotation needs an alternate description (`Contents`).
    /// - Embedded files need a `Description`.
    ///
    /// Fonts:
    /// - You should only use fonts that are legally embeddable in a file
    ///   for unlimited, universal rendering.
    ///
    /// Metadata (enforced by krilla):
    /// - A document title must be set via [`Metadata`].
    /// - A document language must be set via [`Metadata`].
    ///
    /// [`TagKind`]: crate::interchange::tagging::TagKind
    /// [`Metadata`]: crate::metadata::Metadata
    UA2,
    /// The validator for the Well-Tagged PDF (WTPDF) 1.0 profile, published
    /// by the PDF Association in 2024.
    ///
    /// WTPDF is a profile of ISO 32000-2 (PDF 2.0) requiring the document to
    /// be well-tagged using the PDF 2.0 standard structure namespace
    /// (`https://www.iso.org/pdf2/ssn`, as emitted by the pdf-writer crate).
    /// It is the structural foundation of [`UA2`](Self::UA2); UA-2
    /// normatively references WTPDF and layers the accessibility-specific
    /// requirements on top.
    ///
    /// Use this variant when every consumer must receive a tagged
    /// reading-order PDF 2.0 document, but the stricter accessibility
    /// requirements of PDF/UA-2 (alternative text on every figure, mandatory
    /// title and language, display-doc-title viewer preference, document
    /// outline, …) are not desired.
    ///
    /// **Requirements**:
    ///
    /// General:
    /// - All real content should be tagged accordingly using a
    ///   [`TagGroup`] and [`Surface::start_tagged`].
    /// - All artifacts should be marked accordingly with
    ///   [`ContentTag::Artifact`].
    /// - The tag tree should reflect the logical reading order of the
    ///   document.
    ///
    /// Text:
    /// - Word boundaries need to be explicitly specified with a space. The
    ///   same applies to words at the end of a line that are not followed
    ///   by punctuation.
    /// - Hyphenation should be represented as a soft hyphen character
    ///   (U+00AD) instead of a hard hyphen (U+002D).
    ///
    /// Tagging:
    /// - Custom structure types must be mapped via the role map to a
    ///   standard structure type. krilla emits the mapping automatically
    ///   when the standard types are not sufficient.
    /// - To the fullest extent possible, the logical structure of the
    ///   document should be encoded in the tag tree using appropriate
    ///   grouping tags.
    /// - Language identifiers used must be valid according to RFC 3066.
    ///
    /// Fonts:
    /// - You should only use fonts that are legally embeddable in a file
    ///   for unlimited, universal rendering.
    ///
    /// [`TagGroup`]: crate::interchange::tagging::TagGroup
    /// [`Surface::start_tagged`]: crate::surface::Surface::start_tagged
    /// [`ContentTag::Artifact`]: crate::interchange::tagging::ContentTag::Artifact
    WTPDF,
}

impl Accessibility {
    fn prohibits(self, error: &ValidationError) -> bool {
        match (self, error) {
            // PDF/UA-1 (PDF 1.4–1.7 base).
            (
                Self::UA1,
                ValidationError::ContainsNotDefGlyph(_, _, _)
                | ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _)
                | ValidationError::RestrictedLicense(_)
                | ValidationError::NoDocumentTitle
                | ValidationError::MissingAltText(_)
                | ValidationError::MissingHeadingTitle
                | ValidationError::MissingDocumentOutline
                | ValidationError::MissingAnnotationAltText(_)
                | ValidationError::EmbeddedFile(EmbedError::MissingDescription, _)
                | ValidationError::MissingTagging
                | ValidationError::EmbeddedPDF(_)
                | ValidationError::RequiresNewerPdfVersion(
                    VersionedFeature::HeaderFooterArtifactSubtypes
                    | VersionedFeature::StructureOrderTabbing
                    | VersionedFeature::TableHeaderScope,
                    _,
                ),
            ) => true,
            (
                Self::UA1,
                ValidationError::TooLongString
                | ValidationError::TooLongName
                | ValidationError::TooLongArray
                | ValidationError::TooLongDictionary
                | ValidationError::TooLargeFloat
                | ValidationError::TooManyIndirectObjects
                | ValidationError::TooHighQNestingLevel
                | ValidationError::ContainsPostScript(_)
                | ValidationError::MissingCMYKProfile
                | ValidationError::InconsistentSeparationFallback(_)
                | ValidationError::UnicodePrivateArea(_, _, _, _)
                | ValidationError::NoDocumentLanguage
                | ValidationError::Transparency(_)
                | ValidationError::ImageInterpolation(_)
                | ValidationError::EmbeddedFile(
                    EmbedError::Existence | EmbedError::MissingDate | EmbedError::MissingMimeType,
                    _,
                )
                | ValidationError::MissingDocumentDate,
            ) => false,

            // WTPDF + UA-2 share a body; UA-2-only checks are gated by
            // matching `self`.
            (
                Self::UA2 | Self::WTPDF,
                ValidationError::ContainsNotDefGlyph(_, _, _)
                | ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _)
                | ValidationError::RestrictedLicense(_)
                | ValidationError::MissingTagging
                | ValidationError::EmbeddedPDF(_),
            ) => true,
            (
                Self::UA2 | Self::WTPDF,
                ValidationError::TooLongString
                | ValidationError::TooLongName
                | ValidationError::TooLongArray
                | ValidationError::TooLongDictionary
                | ValidationError::TooLargeFloat
                | ValidationError::TooManyIndirectObjects
                | ValidationError::TooHighQNestingLevel
                | ValidationError::ContainsPostScript(_)
                | ValidationError::MissingCMYKProfile
                | ValidationError::InconsistentSeparationFallback(_)
                | ValidationError::UnicodePrivateArea(_, _, _, _)
                | ValidationError::Transparency(_)
                | ValidationError::ImageInterpolation(_)
                | ValidationError::EmbeddedFile(
                    EmbedError::Existence | EmbedError::MissingDate | EmbedError::MissingMimeType,
                    _,
                )
                | ValidationError::MissingDocumentDate
                | ValidationError::RequiresNewerPdfVersion(
                    VersionedFeature::HeaderFooterArtifactSubtypes
                    | VersionedFeature::StructureOrderTabbing
                    | VersionedFeature::TableHeaderScope,
                    _,
                ),
            ) => false,
            // UA-2 mandates these; WTPDF does not.
            (
                Self::UA2 | Self::WTPDF,
                ValidationError::NoDocumentLanguage
                | ValidationError::NoDocumentTitle
                | ValidationError::MissingAltText(_)
                | ValidationError::MissingHeadingTitle
                | ValidationError::MissingDocumentOutline
                | ValidationError::MissingAnnotationAltText(_)
                | ValidationError::EmbeddedFile(EmbedError::MissingDescription, _),
            ) => self == Self::UA2,
            // PDF/X-specific errors: PDF/UA accessibility is silent on them.
            (
                _,
                ValidationError::ContainsRgb(_)
                | ValidationError::ContainsDeviceN(_)
                | ValidationError::MissingTrimOrArtBox(_, _)
                | ValidationError::ContainsAnnotation(_),
            ) => false,
            // Krilla-internal soundness + PDF/X configuration checks:
            // surface under PDF/UA accessibility too.
            (
                _,
                ValidationError::MixedGradientColorSpaces(_)
                | ValidationError::ExternalOutputProfileRequiresX4P,
            ) => true,
            // ISO 14289 (PDF/UA-1, PDF/UA-2) and WTPDF are silent on
            // encryption — accessibility conformance is orthogonal to
            // the security handler — so allow.
            (_, ValidationError::ContainsEncryption) => false,
        }
    }

    fn requires_codepoint_mappings(self) -> bool {
        match self {
            Self::UA1 | Self::UA2 | Self::WTPDF => true,
        }
    }

    fn requires_display_doc_title(self) -> bool {
        match self {
            // UA-1 and UA-2 mandate the DisplayDocTitle viewer preference;
            // WTPDF does not.
            Self::UA1 | Self::UA2 => true,
            Self::WTPDF => false,
        }
    }

    const fn requires_tagging(self) -> bool {
        true
    }

    fn requires_xmp_metadata(self) -> bool {
        match self {
            Self::UA1 | Self::UA2 | Self::WTPDF => true,
        }
    }

    fn write_xmp(self, xmp: &mut XmpWriter) {
        match self {
            Self::UA1 => {
                xmp.pdfua_part(1);
            }
            Self::UA2 => {
                // PDF/UA-2 (ISO 14289-2:2024) identifies itself through
                // `pdfuaid:part = 2` and `pdfuaid:rev = 2024`.
                xmp.pdfua_part(2);
                xmp.pdfua_rev(2024);
            }
            // WTPDF 1.0 does not define a dedicated XMP identification
            // property; conformance is recognised through the well-tagged
            // PDF 2.0 structure (Namespaces, RoleMapNS, MarkInfo).
            Self::WTPDF => {}
        }
    }

    fn write_xmp_extension_schema_description(
        self,
        extension_schemas: &mut PdfAExtSchemasWriter<'_, '_>,
    ) {
        // Needs to be updated if [`Self::write_xmp`] gains more properties.
        extension_schemas.pdfua_id().properties().describe_part();
    }

    /// Returns a human-readable string representation of the accessibility level.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UA1 => "PDF/UA-1",
            Self::UA2 => "PDF/UA-2",
            Self::WTPDF => "WTPDF 1.0",
        }
    }

    /// Minimum PDF version required to use this standard, if any.
    pub const fn min(self) -> Option<PdfVersion> {
        match self {
            // PDF/UA-1 requires Tagged PDF and XMP `/Metadata` streams, which both require PDF 1.4.
            Self::UA1 => Some(PdfVersion::Pdf14),
            // PDF/UA-2 and WTPDF are PDF 2.0 only.
            Self::UA2 | Self::WTPDF => Some(PdfVersion::Pdf20),
        }
    }

    /// Maximum PDF version this standard can be used with.
    pub const fn max(self) -> PdfVersion {
        match self {
            // PDF/UA-1 is specified against PDF 1.7.
            Self::UA1 => PdfVersion::Pdf17,
            // PDF/UA-2 and WTPDF are PDF 2.0 only.
            Self::UA2 | Self::WTPDF => PdfVersion::Pdf20,
        }
    }
}

/// A PDF/X prepress conformance standard.
///
/// PDF/X validators address prepress-specific concerns: predictable colour
/// rendering, embedded output intents (ICC profiles), trim/art boxes, and
/// trapping metadata. They are composable with [`Archival`] via
/// [`ValidatorsBuilder::with_pdfx_validator`] — for example, a document
/// conforming to both PDF/A-1b and PDF/X-1a is configured by setting both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum Pdfx {
    /// The validator for the PDF/X-1a:2003 standard (ISO 15930-4).
    ///
    /// **Requirements**:
    /// - A CMYK ICC profile must be provided via the `cmyk_profile` setting.
    /// - Only CMYK, grayscale, and Separation colors may be used (no RGB).
    /// - No transparency is allowed.
    /// - No annotations are allowed (krilla only supports Link annotations,
    ///   which are not permitted by PDF/X-1a).
    /// - Every page must have a TrimBox or ArtBox set.
    /// - A document title must be set via metadata.
    /// - A creation date must be set via metadata.
    X1A,
    /// The validator for the PDF/X-3:2003 standard (ISO 15930-6).
    ///
    /// **Requirements**:
    /// - A printer/output ICC profile must be provided via the `cmyk_profile`
    ///   setting for the embedded PDF/X output intent.
    /// - No transparency is allowed.
    /// - Every page must have a TrimBox or ArtBox set.
    /// - A document title must be set via metadata.
    /// - A creation date must be set via metadata.
    X3,
    /// The validator for the PDF/X-4 standard (ISO 15930-7).
    ///
    /// **Requirements**:
    /// - A printer/output ICC profile must be provided via the `cmyk_profile`
    ///   setting for the embedded PDF/X output intent.
    /// - Every page must have a TrimBox or ArtBox set.
    /// - A creation date must be set via metadata.
    X4,
    /// The validator for the PDF/X-4p standard (ISO 15930-7).
    ///
    /// Like PDF/X-4, but the output intent ICC profile is referenced
    /// externally instead of being embedded.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/X-4.
    /// - The `external_output_profile` setting must be provided.
    X4P,
    /// The validator for the PDF/X-6 standard (ISO 15930-9).
    ///
    /// Based on PDF 2.0.
    ///
    /// **Requirements**:
    /// - Every page must have a TrimBox or ArtBox set.
    /// - A creation date must be set via metadata.
    /// - A printer/output ICC profile must be provided via the `cmyk_profile`
    ///   setting for the embedded PDF/X output intent.
    X6,
    /// The validator for the PDF/X-6p standard (ISO 15930-9).
    ///
    /// Like PDF/X-6, but the output intent ICC profile is referenced
    /// externally instead of being embedded.
    ///
    /// Based on PDF 2.0.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/X-6.
    /// - The `external_output_profile` setting must be provided.
    X6P,
}

impl Pdfx {
    fn prohibits(self, error: &ValidationError) -> bool {
        match (self, error) {
            // Universally forbidden across the PDF/X family.
            (
                _,
                ValidationError::TooManyIndirectObjects
                | ValidationError::TooHighQNestingLevel
                | ValidationError::ContainsNotDefGlyph(_, _, _)
                | ValidationError::InconsistentSeparationFallback(_)
                | ValidationError::RestrictedLicense(_)
                | ValidationError::MissingDocumentDate
                | ValidationError::MissingCMYKProfile
                | ValidationError::MixedGradientColorSpaces(_)
                | ValidationError::EmbeddedPDF(_)
                | ValidationError::MissingTrimOrArtBox(_, _),
            ) => true,
            // Universally allowed across the PDF/X family.
            (
                _,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _)
                | ValidationError::UnicodePrivateArea(_, _, _, _)
                | ValidationError::NoDocumentLanguage
                | ValidationError::MissingAltText(_)
                | ValidationError::MissingHeadingTitle
                | ValidationError::MissingDocumentOutline
                | ValidationError::MissingAnnotationAltText(_)
                | ValidationError::ImageInterpolation(_)
                | ValidationError::EmbeddedFile(_, _)
                | ValidationError::MissingTagging
                | ValidationError::ContainsDeviceN(_)
                | ValidationError::RequiresNewerPdfVersion(_, _),
            ) => false,
            // PDF/X-1a and PDF/X-3 (PDF 1.4 base) enforce the PDF 1.4 limits.
            (
                Self::X1A | Self::X3,
                ValidationError::TooLongString
                | ValidationError::TooLongName
                | ValidationError::TooLongArray
                | ValidationError::TooLongDictionary
                | ValidationError::TooLargeFloat
                | ValidationError::ContainsPostScript(_),
            ) => true,
            // PDF/X-4 onward (PDF 1.6+) lifts the PDF 1.4 caps and permits
            // PostScript-calculator functions.
            (
                Self::X4 | Self::X4P | Self::X6 | Self::X6P,
                ValidationError::TooLongString
                | ValidationError::TooLongName
                | ValidationError::TooLongArray
                | ValidationError::TooLongDictionary
                | ValidationError::TooLargeFloat
                | ValidationError::ContainsPostScript(_),
            ) => false,
            // PDF/X-1a forbids RGB and annotations entirely. The others allow
            // them.
            (
                _,
                ValidationError::ContainsRgb(_) | ValidationError::ContainsAnnotation(_),
            ) => self == Self::X1A,
            // Transparency is forbidden up to PDF/X-3, allowed from PDF/X-4
            // onward.
            (_, ValidationError::Transparency(_)) => {
                matches!(self, Self::X1A | Self::X3)
            }
            // Mandatory document title for PDF/X-1a/X-3 only; PDF/X-4+
            // dropped the requirement.
            (_, ValidationError::NoDocumentTitle) => matches!(self, Self::X1A | Self::X3),
            // PDF/X-4p and PDF/X-6p require the external output profile to
            // be supplied; the others must NOT have one set.
            (_, ValidationError::ExternalOutputProfileRequiresX4P) => true,
            // ISO 15930 (every PDF/X revision) forbids the `/Encrypt`
            // dictionary — print-exchange RIPs cannot be assumed to
            // know a password.
            (_, ValidationError::ContainsEncryption) => true,
        }
    }

    fn requires_codepoint_mappings(self) -> bool {
        false
    }

    const fn requires_tagging(self) -> bool {
        false
    }

    fn requires_xmp_metadata(self) -> bool {
        true
    }

    fn output_intent(self) -> OutputIntentSubtype<'static> {
        OutputIntentSubtype::PDFX
    }

    /// Whether this PDF/X profile requires the caller to supply an
    /// `external_output_profile`.
    pub(crate) fn requires_external_output_profile(self) -> bool {
        matches!(self, Self::X4P | Self::X6P)
    }

    /// Whether this PDF/X profile forbids RGB content (X-1a only).
    pub(crate) fn requires_cmyk_only(self) -> bool {
        matches!(self, Self::X1A)
    }

    /// Whether this PDF/X profile forbids annotations entirely (X-1a only).
    pub(crate) fn forbids_annotations(self) -> bool {
        matches!(self, Self::X1A)
    }

    fn write_xmp(self, xmp: &mut XmpWriter) {
        if let Some(version) = self.gts_pdfx_version_string() {
            xmp.pdfx_version(version);
        }
    }

    /// The `GTS_PDFXVersion` identification string for this validator.
    pub fn gts_pdfx_version_string(self) -> Option<&'static str> {
        Some(match self {
            Self::X1A => "PDF/X-1a:2003",
            Self::X3 => "PDF/X-3:2003",
            Self::X4 => "PDF/X-4",
            Self::X4P => "PDF/X-4p",
            Self::X6 => "PDF/X-6",
            Self::X6P => "PDF/X-6p",
        })
    }

    /// Returns a human-readable string representation of the standard.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::X1A => "PDF/X-1a",
            Self::X3 => "PDF/X-3",
            Self::X4 => "PDF/X-4",
            Self::X4P => "PDF/X-4p",
            Self::X6 => "PDF/X-6",
            Self::X6P => "PDF/X-6p",
        }
    }

    /// Minimum PDF version required to use this standard, if any.
    pub const fn min(self) -> Option<PdfVersion> {
        match self {
            Self::X1A | Self::X3 => Some(PdfVersion::Pdf14),
            Self::X4 | Self::X4P => Some(PdfVersion::Pdf16),
            Self::X6 | Self::X6P => Some(PdfVersion::Pdf20),
        }
    }

    /// Maximum PDF version this standard can be used with.
    pub const fn max(self) -> PdfVersion {
        match self {
            Self::X1A | Self::X3 => PdfVersion::Pdf14,
            Self::X4 | Self::X4P => PdfVersion::Pdf16,
            Self::X6 | Self::X6P => PdfVersion::Pdf20,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ValidationStore {
    /// Maps from the name of a Separation colorant to a hash of its fallback
    /// color. Used to track that a name is only ever matched with a single
    /// fallback color. Since Krilla manages the `tintTransform` functions,
    /// those are always equivalent.
    separation_fallback_map: HashMap<SeparationColorant, RegularColor>,
}

impl ValidationStore {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    /// Register a DeviceN colour-space registration. Currently this
    /// only surfaces the "DeviceN was used at all" signal for PDF/A-1
    /// validation; future revisions may also enforce per-colorant
    /// consistency analogous to [`Self::validate_separation`].
    pub(crate) fn validate_devicen(
        &mut self,
        _space: &DeviceNSpace,
    ) -> Result<(), ValidationError> {
        // Raise the "contains DeviceN" signal unconditionally; the
        // profile-level `prohibits` table decides whether it actually
        // fires (PDF/A-1 forbids; everyone else allows).
        Err(ValidationError::ContainsDeviceN(None))
    }

    /// Register a colorant and its fallback and raise an error if it already
    /// exists.
    pub(crate) fn validate_separation(
        &mut self,
        separation: &SeparationSpace,
    ) -> Result<(), ValidationError> {
        // `RegularColor` is no longer `Copy` after the `IccBased`
        // variant landed; clone the fallback once and compare the
        // already-stored entry against it by reference.
        if self
            .separation_fallback_map
            .entry(separation.colorant.clone())
            .or_insert_with(|| separation.fallback.clone())
            == &separation.fallback
        {
            Ok(())
        } else {
            Err(ValidationError::InconsistentSeparationFallback(
                separation.colorant.clone(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PDF/A-1 (ISO 19005-1 §6.2.4) prohibits DeviceN; both the
    /// accessibility-aware and accessibility-blind profiles must
    /// surface the violation.
    #[test]
    fn pdf_a_1_prohibits_devicen() {
        let err = ValidationError::ContainsDeviceN(None);
        assert!(Archival::A1_A.prohibits(&err));
        assert!(Archival::A1_B.prohibits(&err));
    }

    /// PDF/A-2 onward — and PDF/A-3, PDF/A-4 in every flavour —
    /// admit DeviceN.
    #[test]
    fn pdf_a_2_and_later_admit_devicen() {
        let err = ValidationError::ContainsDeviceN(None);
        for profile in [
            Archival::A2_A,
            Archival::A2_B,
            Archival::A2_U,
            Archival::A3_A,
            Archival::A3_B,
            Archival::A3_U,
            Archival::A4,
            Archival::A4F,
            Archival::A4E,
        ] {
            assert!(!profile.prohibits(&err), "{profile:?} unexpectedly forbids DeviceN");
        }
    }

    /// Every PDF/X profile admits DeviceN.
    #[test]
    fn pdf_x_admits_devicen() {
        let err = ValidationError::ContainsDeviceN(None);
        for profile in [
            Pdfx::X1A,
            Pdfx::X3,
            Pdfx::X4,
            Pdfx::X4P,
            Pdfx::X6,
            Pdfx::X6P,
        ] {
            assert!(!profile.prohibits(&err), "{profile:?} unexpectedly forbids DeviceN");
        }
    }

    /// PDF/UA / WTPDF (accessibility-only) profiles are silent on
    /// colour-space choice — DeviceN passes.
    #[test]
    fn pdf_ua_admits_devicen() {
        let err = ValidationError::ContainsDeviceN(None);
        for profile in [Accessibility::UA1, Accessibility::UA2, Accessibility::WTPDF] {
            assert!(!profile.prohibits(&err), "{profile:?} unexpectedly forbids DeviceN");
        }
    }
}
