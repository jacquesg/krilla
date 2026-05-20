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
use pdf_writer::Finish;
use xmp_writer::{Namespace, XmpWriter};

use crate::color::separation::SeparationColorant;
use crate::color::separation::SeparationSpace;
use crate::color::RegularColor;
use crate::configure::PdfVersion;
use crate::interchange::embed::EmbedError;
use crate::surface::Location;
use crate::text::Font;
use crate::text::GlyphId;

/// An error that occurred during validation.
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
    /// No external output profile reference was provided for PDF/X-4p or
    /// PDF/X-6p.
    ///
    /// Occurs if the export target is PDF/X-4p or PDF/X-6p and the caller did
    /// not set [`SerializeSettings::external_output_profile`].
    ///
    /// [`SerializeSettings::external_output_profile`]:
    /// crate::SerializeSettings::external_output_profile
    MissingExternalOutputProfile,
    /// An external output profile reference was provided for a validator
    /// other than PDF/X-4p or PDF/X-6p.
    ///
    /// Occurs if a non-PDF/X-*p validator is configured but
    /// [`SerializeSettings::external_output_profile`] is `Some`.
    ///
    /// [`SerializeSettings::external_output_profile`]:
    /// crate::SerializeSettings::external_output_profile
    ExternalOutputProfileUnsupportedByValidator,
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
    /// The PDF contains an RGB color, which is forbidden by PDF/X-1a.
    ///
    /// Occurs if an RGB color was used in fills, strokes, gradients, images,
    /// or separation fallback colors when exporting to PDF/X-1a. Grayscale
    /// colors are permitted.
    ContainsRgb(Option<Location>),
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
    /// Occurs if a page does not have either a TrimBox or an ArtBox set in
    /// its [`PageSettings`](crate::page::PageSettings). The first field is
    /// the zero-based index of the offending page.
    MissingTrimOrArtBox(usize, Option<Location>),
    /// The PDF contains annotations which are forbidden by PDF/X-1a.
    ///
    /// PDF/X-1a only allows TrapNet and PrinterMark annotations, neither of
    /// which is supported by krilla.
    ContainsAnnotation(Option<Location>),
}

/// A validator for exporting PDF documents to a specific subset of PDF.
///
/// # Variant naming
///
/// Variants follow the short-form ISO identifier of the standard they check:
///
/// - PDF/A part-and-conformance variants use `{Part}_{Conformance}` form
///   (e.g. `A1_B`, `A2_U`, `A3_A`).
/// - PDF/A-4 subforms (`A4`, `A4F`, `A4E`), PDF/UA (`UA1`, `UA2`), and PDF/X
///   variants (`X1A`, `X4P`, `X6P`, …) use the ISO short-form without an
///   internal separator, matching their specification names.
/// - Combined PDF/A + PDF/X validators join the two short forms with an
///   underscore, e.g. `A1B_X1A` = "PDF/A-1b + PDF/X-1a".
/// - The PDF Association Well-Tagged PDF profile uses the upper-case acronym
///   (`WTPDF`).
///
/// All identifiers are uppercase to match PDF-library conventions. The
/// `#[allow(non_camel_case_types)]` attribute is required by the `A1_A`-style
/// names and applies enum-wide.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[allow(non_camel_case_types)]
pub enum Validator {
    /// A dummy validator, that does not perform any actual validation.
    ///
    /// **Requirements**: -
    #[default]
    None,
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
    /// Tables:
    /// - Tables should include headers and be tagged accordingly.
    /// - Tables should only be used to represent content within a logical
    ///   row/column relationship.
    ///
    /// Lists:
    /// - List items should be tagged with Li tags, if necessary also with
    ///   Lbl and LBody tags.
    /// - Lists should only be used when the content is intended to be read
    ///   as a list.
    ///
    /// Mathematical expressions:
    /// - All mathematical expressions should be enclosed with a `Formula`
    ///   tag. Use an `AF` associated MathML file where the expression is
    ///   non-trivial.
    ///
    /// Headers and footers:
    /// - Headers and footers should be marked as corresponding artifacts.
    ///
    /// Notes and references:
    /// - Footnotes, endnotes, note labels and references should be tagged
    ///   accordingly and use tagged annotations.
    /// - Footnotes and end notes should use the `Note` tag.
    ///
    /// Navigation:
    /// - The document should contain an outline reflecting the reading
    ///   order of the document.
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
    /// (`http://iso.org/pdf2/ssn`). It is the structural foundation of
    /// [`UA2`](Self::UA2); UA-2 normatively references WTPDF and layers the
    /// accessibility-specific requirements on top.
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
    ///   [`TagGroup`] and
    ///   [`Surface::start_tagged`].
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
    /// Combined PDF/A-1b + PDF/X-1a:2003 validator.
    ///
    /// Both standards are enforced simultaneously. The most restrictive
    /// requirement from each standard applies.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-1b.
    /// - All requirements of PDF/X-1a.
    A1B_X1A,
    /// Combined PDF/A-2b + PDF/X-4 validator.
    ///
    /// Both standards are enforced simultaneously. The most restrictive
    /// requirement from each standard applies.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-2b.
    /// - All requirements of PDF/X-4.
    A2B_X4,
    /// Combined PDF/A-3b + PDF/X-4 validator.
    ///
    /// Both standards are enforced simultaneously. The most restrictive
    /// requirement from each standard applies.
    ///
    /// **Requirements**:
    /// - All requirements of PDF/A-3b.
    /// - All requirements of PDF/X-4.
    A3B_X4,
}

impl Validator {
    pub(crate) fn prohibits(&self, validation_error: &ValidationError) -> bool {
        match self {
            Validator::None => matches!(
                validation_error,
                ValidationError::MixedGradientColorSpaces(_)
                    | ValidationError::ExternalOutputProfileUnsupportedByValidator
            ),
            Validator::A1_A | Validator::A1_B => match validation_error {
                ValidationError::TooLongString => true,
                ValidationError::TooLongName => true,
                ValidationError::TooLongArray => true,
                ValidationError::TooLargeFloat => true,
                ValidationError::TooLongDictionary => true,
                ValidationError::TooManyIndirectObjects => true,
                ValidationError::TooHighQNestingLevel => true,
                ValidationError::ContainsPostScript(_) => true,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => false,
                ValidationError::ContainsNotDefGlyph(_, _, _) => self.requires_codepoint_mappings(),
                ValidationError::NoCodepointMapping(_, _, _) => self.requires_codepoint_mappings(),
                ValidationError::InvalidCodepointMapping(_, _, _, _) => false,
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => *self == Validator::A1_A,
                ValidationError::NoDocumentTitle => false,
                ValidationError::MissingAltText(_) => *self == Validator::A1_A,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => *self == Validator::A1_A,
                ValidationError::Transparency(_) => true,
                ValidationError::ImageInterpolation(_) => true,
                // PDF/A-1 doesn't strictly forbid, but it disallows the EF key,
                // which we always insert. So we just forbid it overall.
                ValidationError::EmbeddedFile(e, _) => match e {
                    EmbedError::Existence => true,
                    // Since existence is forbidden in the first place,
                    // we can just set the others to `false` to prevent unnecessary
                    // validation errors.
                    EmbedError::MissingDate => false,
                    EmbedError::MissingDescription => false,
                    EmbedError::MissingMimeType => false,
                },
                ValidationError::MissingTagging => *self == Validator::A1_A,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => false,
                ValidationError::ContainsAnnotation(_) => false,
            },
            Validator::A2_A | Validator::A2_B | Validator::A2_U => match validation_error {
                ValidationError::TooLongString => true,
                ValidationError::TooLongName => true,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooLongArray => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooManyIndirectObjects => true,
                ValidationError::TooHighQNestingLevel => true,
                ValidationError::ContainsPostScript(_) => true,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => true,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => {
                    self.requires_codepoint_mappings()
                }
                ValidationError::UnicodePrivateArea(_, _, _, _) => *self == Validator::A2_A,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => *self == Validator::A2_A,
                ValidationError::NoDocumentTitle => false,
                ValidationError::MissingAltText(_) => *self == Validator::A2_A,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => *self == Validator::A2_A,
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => true,
                // Also not strictly forbidden, but we can't ensure that it is PDF/A-2 compliant,
                // so we just forbid it completely.
                ValidationError::EmbeddedFile(e, _) => match e {
                    EmbedError::Existence => true,
                    // Since existence is forbidden in the first place,
                    // we can just set the others to `false` to prevent unnecessary
                    // validation errors.
                    EmbedError::MissingDate => false,
                    EmbedError::MissingDescription => false,
                    EmbedError::MissingMimeType => false,
                },
                ValidationError::MissingTagging => *self == Validator::A2_A,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => false,
                ValidationError::ContainsAnnotation(_) => false,
            },
            Validator::A3_A | Validator::A3_B | Validator::A3_U => match validation_error {
                ValidationError::TooLongString => true,
                ValidationError::TooLongName => true,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooLongArray => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooManyIndirectObjects => true,
                ValidationError::TooHighQNestingLevel => true,
                ValidationError::ContainsPostScript(_) => true,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => true,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => {
                    self.requires_codepoint_mappings()
                }
                ValidationError::UnicodePrivateArea(_, _, _, _) => *self == Validator::A3_A,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => *self == Validator::A3_A,
                ValidationError::NoDocumentTitle => false,
                ValidationError::MissingAltText(_) => *self == Validator::A3_A,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => *self == Validator::A3_A,
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => true,
                ValidationError::EmbeddedFile(er, _) => match er {
                    EmbedError::Existence => false,
                    EmbedError::MissingDate => true,
                    EmbedError::MissingDescription => true,
                    EmbedError::MissingMimeType => true,
                },
                ValidationError::MissingTagging => *self == Validator::A3_A,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => false,
                ValidationError::ContainsAnnotation(_) => false,
            },
            Validator::A4 | Validator::A4F | Validator::A4E => match validation_error {
                ValidationError::TooLongString => false,
                ValidationError::TooLongName => false,
                ValidationError::TooLongArray => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooManyIndirectObjects => false,
                ValidationError::TooHighQNestingLevel => false,
                ValidationError::ContainsPostScript(_) => false,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => true,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => true,
                // Not strictly forbidden if we surround with actual text, but
                // easier to just forbid it.
                ValidationError::UnicodePrivateArea(_, _, _, _) => true,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => false,
                ValidationError::MissingAltText(_) => false,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => false,
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => true,
                ValidationError::EmbeddedFile(e, _) => match e {
                    EmbedError::Existence => matches!(self, Validator::A4),
                    // Since existence is forbidden in the first place for A4,
                    // we can just set the others to `false` to prevent
                    // unnecessary validation errors.
                    EmbedError::MissingDate => false,
                    EmbedError::MissingDescription => {
                        matches!(self, Validator::A4E | Validator::A4F)
                    }
                    EmbedError::MissingMimeType => false,
                },
                // Only recommended, not required.
                ValidationError::MissingTagging => false,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => false,
                ValidationError::ContainsAnnotation(_) => false,
            },
            Validator::UA1 => match validation_error {
                ValidationError::TooLongString => false,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooLongName => false,
                ValidationError::TooLongArray => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooManyIndirectObjects => false,
                ValidationError::TooHighQNestingLevel => false,
                ValidationError::ContainsPostScript(_) => false,
                ValidationError::MissingCMYKProfile => false,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => false,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => {
                    self.requires_codepoint_mappings()
                }
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => true,
                ValidationError::MissingAltText(_) => true,
                ValidationError::MissingHeadingTitle => true,
                ValidationError::MissingDocumentOutline => true,
                ValidationError::MissingAnnotationAltText(_) => true,
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => false,
                ValidationError::EmbeddedFile(er, _) => match er {
                    EmbedError::Existence => false,
                    EmbedError::MissingDate => false,
                    EmbedError::MissingDescription => true,
                    EmbedError::MissingMimeType => false,
                },
                ValidationError::MissingTagging => true,
                ValidationError::MissingDocumentDate => false,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => false,
                ValidationError::ContainsAnnotation(_) => false,
            },
            // WTPDF is the structural base; UA-2 layers the accessibility-
            // metadata requirements (alt text, title, language, outline,
            // annotation Contents, embedded-file Description) on top, gated
            // via `*self == Validator::UA2`. Anything that is identical
            // between the two profiles stays in the shared body.
            Validator::WTPDF | Validator::UA2 => match validation_error {
                // PDF 2.0 lifts the PDF 1.4-era object-graph limits, so the
                // krilla-tracked maxima do not apply.
                ValidationError::TooLongString => false,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooLongName => false,
                ValidationError::TooLongArray => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooManyIndirectObjects => false,
                ValidationError::TooHighQNestingLevel => false,
                ValidationError::ContainsPostScript(_) => false,
                // No output intent is required by either profile.
                ValidationError::MissingCMYKProfile => false,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => true,
                // WTPDF §6.4 / UA-2 §7.1.2: every glyph drawn must map to a
                // sensible code point.
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => {
                    self.requires_codepoint_mappings()
                }
                // UA-2 §7.1.3 permits PUA glyphs provided an `ActualText`
                // attribute is supplied; krilla cannot verify the latter, so
                // we mirror the UA-1 lenient stance rather than reject.
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                // UA-2 §7.2.4 requires the document language; WTPDF only
                // recommends it.
                ValidationError::NoDocumentLanguage => *self == Validator::UA2,
                // UA-2 §7.2.5 requires the document title; WTPDF does not.
                ValidationError::NoDocumentTitle => *self == Validator::UA2,
                // UA-2 §7.18 requires alt text for figures and formulas;
                // WTPDF does not.
                ValidationError::MissingAltText(_) => *self == Validator::UA2,
                ValidationError::MissingHeadingTitle => *self == Validator::UA2,
                // UA-2 §7.16 mandates an outline for any document where
                // navigation requires one; krilla treats the requirement as
                // absolute, matching UA-1.
                ValidationError::MissingDocumentOutline => *self == Validator::UA2,
                ValidationError::MissingAnnotationAltText(_) => *self == Validator::UA2,
                // PDF 2.0 supports live transparency; no restriction.
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => false,
                ValidationError::EmbeddedFile(er, _) => match er {
                    EmbedError::Existence => false,
                    EmbedError::MissingDate => false,
                    // UA-2 §7.20 requires a description on every embedded
                    // file; WTPDF does not.
                    EmbedError::MissingDescription => *self == Validator::UA2,
                    EmbedError::MissingMimeType => false,
                },
                // Both profiles require a tag tree.
                ValidationError::MissingTagging => true,
                ValidationError::MissingDocumentDate => false,
                // krilla cannot inspect an embedded PDF for conformance.
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => false,
                ValidationError::ContainsAnnotation(_) => false,
            },
            Validator::X1A => match validation_error {
                ValidationError::TooLongString => true,
                ValidationError::TooLongName => true,
                ValidationError::TooLongArray => true,
                ValidationError::TooLargeFloat => true,
                ValidationError::TooLongDictionary => true,
                ValidationError::TooManyIndirectObjects => true,
                ValidationError::TooHighQNestingLevel => true,
                ValidationError::ContainsPostScript(_) => true,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => false,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => false,
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => true,
                ValidationError::MissingAltText(_) => false,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => false,
                ValidationError::Transparency(_) => true,
                ValidationError::ImageInterpolation(_) => false,
                // ISO 15930-4 forbids embedded files.
                ValidationError::EmbeddedFile(e, _) => match e {
                    EmbedError::Existence => true,
                    EmbedError::MissingDate => false,
                    EmbedError::MissingDescription => false,
                    EmbedError::MissingMimeType => false,
                },
                ValidationError::MissingTagging => false,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => true,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => true,
                ValidationError::ContainsAnnotation(_) => true,
            },
            Validator::X3 => match validation_error {
                ValidationError::TooLongString => true,
                ValidationError::TooLongName => true,
                ValidationError::TooLongArray => true,
                ValidationError::TooLargeFloat => true,
                ValidationError::TooLongDictionary => true,
                ValidationError::TooManyIndirectObjects => true,
                ValidationError::TooHighQNestingLevel => true,
                ValidationError::ContainsPostScript(_) => true,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => true,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => false,
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => true,
                ValidationError::MissingAltText(_) => false,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => false,
                ValidationError::Transparency(_) => true,
                ValidationError::ImageInterpolation(_) => false,
                ValidationError::EmbeddedFile(_, _) => false,
                ValidationError::MissingTagging => false,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => true,
                ValidationError::ContainsAnnotation(_) => false,
            },
            Validator::X4 | Validator::X4P => match validation_error {
                ValidationError::TooLongString => false,
                ValidationError::TooLongName => false,
                ValidationError::TooLongArray => false,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooManyIndirectObjects => false,
                ValidationError::TooHighQNestingLevel => false,
                ValidationError::ContainsPostScript(_) => false,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => *self == Validator::X4P,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => {
                    *self != Validator::X4P
                }
                ValidationError::InconsistentSeparationFallback(_) => true,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => false,
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => false,
                ValidationError::MissingAltText(_) => false,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => false,
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => false,
                ValidationError::EmbeddedFile(_, _) => false,
                ValidationError::MissingTagging => false,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => true,
                ValidationError::ContainsAnnotation(_) => false,
            },
            Validator::X6 | Validator::X6P => match validation_error {
                ValidationError::TooLongString => false,
                ValidationError::TooLongName => false,
                ValidationError::TooLongArray => false,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooManyIndirectObjects => false,
                ValidationError::TooHighQNestingLevel => false,
                ValidationError::ContainsPostScript(_) => false,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => *self == Validator::X6P,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => {
                    *self != Validator::X6P
                }
                ValidationError::InconsistentSeparationFallback(_) => true,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => false,
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => false,
                ValidationError::MissingAltText(_) => false,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => false,
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => false,
                ValidationError::EmbeddedFile(_, _) => false,
                ValidationError::MissingTagging => false,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => true,
                ValidationError::ContainsAnnotation(_) => false,
            },
            // Composite: union of A1_B and X1A restrictions.
            Validator::A1B_X1A => match validation_error {
                ValidationError::TooLongString => true,
                ValidationError::TooLongName => true,
                ValidationError::TooLongArray => true,
                ValidationError::TooLargeFloat => true,
                ValidationError::TooLongDictionary => true,
                ValidationError::TooManyIndirectObjects => true,
                ValidationError::TooHighQNestingLevel => true,
                ValidationError::ContainsPostScript(_) => true,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => false,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => false,
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => true,
                ValidationError::MissingAltText(_) => false,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => false,
                ValidationError::Transparency(_) => true,
                ValidationError::ImageInterpolation(_) => true,
                ValidationError::EmbeddedFile(e, _) => match e {
                    EmbedError::Existence => true,
                    EmbedError::MissingDate => false,
                    EmbedError::MissingDescription => false,
                    EmbedError::MissingMimeType => false,
                },
                ValidationError::MissingTagging => false,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => true,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => true,
                ValidationError::ContainsAnnotation(_) => true,
            },
            // Composite: union of A2_B and X4 restrictions.
            Validator::A2B_X4 => match validation_error {
                ValidationError::TooLongString => true,
                ValidationError::TooLongName => true,
                ValidationError::TooLongArray => false,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooManyIndirectObjects => true,
                ValidationError::TooHighQNestingLevel => true,
                ValidationError::ContainsPostScript(_) => true,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => true,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => false,
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => false,
                ValidationError::MissingAltText(_) => false,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => false,
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => true,
                ValidationError::EmbeddedFile(e, _) => match e {
                    EmbedError::Existence => true,
                    EmbedError::MissingDate => false,
                    EmbedError::MissingDescription => false,
                    EmbedError::MissingMimeType => false,
                },
                ValidationError::MissingTagging => false,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => true,
                ValidationError::ContainsAnnotation(_) => false,
            },
            // Composite: union of A3_B and X4 restrictions.
            Validator::A3B_X4 => match validation_error {
                ValidationError::TooLongString => true,
                ValidationError::TooLongName => true,
                ValidationError::TooLongArray => false,
                ValidationError::TooLargeFloat => false,
                ValidationError::TooLongDictionary => false,
                ValidationError::TooManyIndirectObjects => true,
                ValidationError::TooHighQNestingLevel => true,
                ValidationError::ContainsPostScript(_) => true,
                ValidationError::MissingCMYKProfile => true,
                ValidationError::MissingExternalOutputProfile => false,
                ValidationError::ExternalOutputProfileUnsupportedByValidator => true,
                ValidationError::InconsistentSeparationFallback(_) => true,
                ValidationError::ContainsNotDefGlyph(_, _, _) => true,
                ValidationError::NoCodepointMapping(_, _, _)
                | ValidationError::InvalidCodepointMapping(_, _, _, _) => false,
                ValidationError::UnicodePrivateArea(_, _, _, _) => false,
                ValidationError::RestrictedLicense(_) => true,
                ValidationError::NoDocumentLanguage => false,
                ValidationError::NoDocumentTitle => false,
                ValidationError::MissingAltText(_) => false,
                ValidationError::MissingHeadingTitle => false,
                ValidationError::MissingDocumentOutline => false,
                ValidationError::MissingAnnotationAltText(_) => false,
                ValidationError::Transparency(_) => false,
                ValidationError::ImageInterpolation(_) => true,
                ValidationError::EmbeddedFile(er, _) => match er {
                    EmbedError::Existence => false,
                    EmbedError::MissingDate => true,
                    EmbedError::MissingDescription => true,
                    EmbedError::MissingMimeType => true,
                },
                ValidationError::MissingTagging => false,
                ValidationError::MissingDocumentDate => true,
                ValidationError::EmbeddedPDF(_) => true,
                ValidationError::ContainsRgb(_) => false,
                ValidationError::MixedGradientColorSpaces(_) => true,
                ValidationError::MissingTrimOrArtBox(_, _) => true,
                ValidationError::ContainsAnnotation(_) => false,
            },
        }
    }

    /// Check whether the validator is compatible with a specific pdf version.
    pub fn compatible_with_version(&self, pdf_version: PdfVersion) -> bool {
        match self {
            Validator::None => true,
            Validator::A1_A | Validator::A1_B => pdf_version <= PdfVersion::Pdf14,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => pdf_version <= PdfVersion::Pdf17,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => pdf_version <= PdfVersion::Pdf17,
            // It can be any 2.x version, but we're not there yet.
            Validator::A4 | Validator::A4F | Validator::A4E => pdf_version == PdfVersion::Pdf20,
            Validator::UA1 => pdf_version <= PdfVersion::Pdf17,
            Validator::UA2 | Validator::WTPDF => pdf_version == PdfVersion::Pdf20,
            Validator::X1A | Validator::X3 | Validator::A1B_X1A => pdf_version <= PdfVersion::Pdf14,
            Validator::X4 | Validator::X4P | Validator::A2B_X4 | Validator::A3B_X4 => {
                pdf_version == PdfVersion::Pdf16
            }
            Validator::X6 | Validator::X6P => pdf_version == PdfVersion::Pdf20,
        }
    }

    /// Get the recommended PDF version of a validator.
    pub fn recommended_version(&self) -> PdfVersion {
        match self {
            Validator::None => PdfVersion::Pdf17,
            Validator::A1_A | Validator::A1_B => PdfVersion::Pdf14,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => PdfVersion::Pdf17,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => PdfVersion::Pdf17,
            Validator::A4 | Validator::A4F | Validator::A4E => PdfVersion::Pdf20,
            Validator::UA1 => PdfVersion::Pdf17,
            Validator::UA2 | Validator::WTPDF => PdfVersion::Pdf20,
            Validator::X1A | Validator::X3 | Validator::A1B_X1A => PdfVersion::Pdf14,
            Validator::X4 | Validator::X4P | Validator::A2B_X4 | Validator::A3B_X4 => {
                PdfVersion::Pdf16
            }
            Validator::X6 | Validator::X6P => PdfVersion::Pdf20,
        }
    }

    /// Whether this validator enforces any PDF/A standard.
    ///
    /// Includes combined PDF/A + PDF/X validators (`A1B_X1A`, `A2B_X4`,
    /// `A3B_X4`) — those must satisfy both the PDF/A and the PDF/X
    /// requirements, so the PDF/A leg reports `true` here.
    fn is_pdf_a(&self) -> bool {
        matches!(
            self,
            Validator::A1_A
                | Validator::A1_B
                | Validator::A2_A
                | Validator::A2_B
                | Validator::A2_U
                | Validator::A3_A
                | Validator::A3_B
                | Validator::A3_U
                | Validator::A4
                | Validator::A4F
                | Validator::A4E
                | Validator::A1B_X1A
                | Validator::A2B_X4
                | Validator::A3B_X4
        )
    }

    /// Whether this validator enforces any PDF/X standard.
    pub(crate) fn is_pdf_x(&self) -> bool {
        matches!(
            self,
            Validator::X1A
                | Validator::X3
                | Validator::X4
                | Validator::X4P
                | Validator::X6
                | Validator::X6P
                | Validator::A1B_X1A
                | Validator::A2B_X4
                | Validator::A3B_X4
        )
    }

    pub(crate) fn write_xmp(&self, xmp: &mut XmpWriter) {
        // TODO: Also needed for PDF/UA?
        if self.is_pdf_a() {
            let mut extension_schemas = xmp.extension_schemas();
            extension_schemas
                .xmp_media_management()
                .properties()
                .describe_instance_id();
            extension_schemas.pdf().properties().describe_all();
            if self.requires_pdfx_extension_schema() {
                let mut schema = extension_schemas.add_schema();
                schema.namespace(Namespace::PdfXId);
                schema
                    .properties()
                    .add_property()
                    .category(true)
                    .description("Version of the PDF/X standard to which the document conforms")
                    .name("GTS_PDFXVersion")
                    .value_type("Text");
            }
            extension_schemas.finish();
        }

        match self {
            Validator::None => {}
            Validator::A1_A => {
                xmp.pdfa_part(1);
                xmp.pdfa_conformance("A");
            }
            Validator::A1_B => {
                xmp.pdfa_part(1);
                xmp.pdfa_conformance("B");
            }
            Validator::A2_A => {
                xmp.pdfa_part(2);
                xmp.pdfa_conformance("A");
            }
            Validator::A2_B => {
                xmp.pdfa_part(2);
                xmp.pdfa_conformance("B");
            }
            Validator::A2_U => {
                xmp.pdfa_part(2);
                xmp.pdfa_conformance("U");
            }
            Validator::A3_A => {
                xmp.pdfa_part(3);
                xmp.pdfa_conformance("A");
            }
            Validator::A3_B => {
                xmp.pdfa_part(3);
                xmp.pdfa_conformance("B");
            }
            Validator::A3_U => {
                xmp.pdfa_part(3);
                xmp.pdfa_conformance("U");
            }
            Validator::A4 => {
                xmp.pdfa_part(4);
                xmp.pdfa_rev(2020);
            }
            Validator::A4F => {
                xmp.pdfa_part(4);
                xmp.pdfa_rev(2020);
                xmp.pdfa_conformance("F");
            }
            Validator::A4E => {
                xmp.pdfa_part(4);
                xmp.pdfa_rev(2020);
                xmp.pdfa_conformance("E");
            }
            Validator::UA1 => {
                xmp.pdfua_part(1);
            }
            // PDF/UA-2 (ISO 14289-2:2024) identifies itself through
            // `pdfuaid:part = 2` and `pdfuaid:rev = 2024`.
            Validator::UA2 => {
                xmp.pdfua_part(2);
                xmp.pdfua_rev(2024);
            }
            // WTPDF 1.0 does not define a dedicated XMP identification
            // property; conformance is recognised through the well-tagged
            // PDF 2.0 structure (Namespaces, RoleMapNS, MarkInfo).
            Validator::WTPDF => {}
            Validator::X1A => {
                xmp.pdfx_version("PDF/X-1a:2003");
            }
            Validator::X3 => {
                xmp.pdfx_version("PDF/X-3:2003");
            }
            Validator::X4 | Validator::X4P | Validator::X6 | Validator::X6P => {
                if let Some(v) = self.gts_pdfx_version_string() {
                    xmp.pdfx_version(v);
                }
            }
            Validator::A1B_X1A => {
                xmp.pdfa_part(1);
                xmp.pdfa_conformance("B");
                xmp.pdfx_version("PDF/X-1a:2003");
            }
            Validator::A2B_X4 => {
                xmp.pdfa_part(2);
                xmp.pdfa_conformance("B");
                xmp.pdfx_version("PDF/X-4");
            }
            Validator::A3B_X4 => {
                xmp.pdfa_part(3);
                xmp.pdfa_conformance("B");
                xmp.pdfx_version("PDF/X-4");
            }
        }
    }

    pub(crate) fn requires_codepoint_mappings(&self) -> bool {
        match self {
            Validator::None => false,
            Validator::A1_A | Validator::A1_B => *self != Validator::A1_B,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => *self != Validator::A2_B,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => *self != Validator::A3_B,
            Validator::A4 | Validator::A4F | Validator::A4E => true,
            Validator::UA1 => true,
            Validator::UA2 | Validator::WTPDF => true,
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P => false,
            // Composites inherit from their PDF/A constituent.
            Validator::A1B_X1A => false, // A1_B doesn't require it
            Validator::A2B_X4 => false,  // A2_B doesn't require it
            Validator::A3B_X4 => false,  // A3_B doesn't require it
        }
    }

    pub(crate) fn requires_display_doc_title(&self) -> bool {
        match self {
            Validator::None => false,
            Validator::A1_A | Validator::A1_B => false,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => false,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => false,
            Validator::A4 | Validator::A4F | Validator::A4E => false,
            Validator::UA1 => true,
            // UA-2 §7.2.5 mandates the DisplayDocTitle viewer preference.
            // WTPDF does not.
            Validator::UA2 => true,
            Validator::WTPDF => false,
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P
            | Validator::A1B_X1A
            | Validator::A2B_X4
            | Validator::A3B_X4 => false,
        }
    }

    pub(crate) fn requires_no_device_cs(&self) -> bool {
        match self {
            Validator::None => false,
            Validator::A1_A | Validator::A1_B => true,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => true,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => true,
            Validator::A4 | Validator::A4F | Validator::A4E => true,
            Validator::UA1 => false,
            Validator::UA2 | Validator::WTPDF => false,
            Validator::X1A | Validator::A1B_X1A => false,
            Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P
            | Validator::A2B_X4
            | Validator::A3B_X4 => true,
        }
    }

    pub(crate) fn requires_annotation_flags(&self) -> bool {
        match self {
            Validator::None | Validator::UA1 | Validator::UA2 | Validator::WTPDF => false,
            Validator::A1_A | Validator::A1_B => true,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => true,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => true,
            Validator::A4 | Validator::A4F | Validator::A4E => true,
            // X1A forbids annotations entirely, so flags are irrelevant.
            Validator::X1A | Validator::A1B_X1A => false,
            Validator::X3 | Validator::X4 | Validator::X4P | Validator::X6 | Validator::X6P => true,
            Validator::A2B_X4 | Validator::A3B_X4 => true,
        }
    }

    pub(crate) fn requires_tagging(&self) -> bool {
        match self {
            Validator::None => false,
            Validator::A1_A => true,
            Validator::A1_B => false,
            Validator::A2_A => true,
            Validator::A2_B | Validator::A2_U => false,
            Validator::A3_A => true,
            Validator::A3_B | Validator::A3_U => false,
            Validator::A4 | Validator::A4F | Validator::A4E => false,
            Validator::UA1 => true,
            Validator::UA2 | Validator::WTPDF => true,
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P
            | Validator::A1B_X1A
            | Validator::A2B_X4
            | Validator::A3B_X4 => false,
        }
    }

    pub(crate) fn xmp_metadata(&self) -> bool {
        match self {
            Validator::None => false,
            Validator::A1_A | Validator::A1_B => true,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => true,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => true,
            Validator::A4 | Validator::A4F | Validator::A4E => true,
            Validator::UA1 => true,
            Validator::UA2 | Validator::WTPDF => true,
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P
            | Validator::A1B_X1A
            | Validator::A2B_X4
            | Validator::A3B_X4 => true,
        }
    }

    pub(crate) fn requires_binary_header(&self) -> bool {
        match self {
            Validator::None => false,
            Validator::A1_A | Validator::A1_B => true,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => true,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => true,
            Validator::A4 | Validator::A4F | Validator::A4E => true,
            Validator::UA1 => false,
            // ISO 14289-2 / WTPDF 1.0 do not mandate the binary marker;
            // ISO 32000-2 only recommends it. Honour the caller's
            // `ascii_compatible` setting like UA-1.
            Validator::UA2 | Validator::WTPDF => false,
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P
            | Validator::A1B_X1A
            | Validator::A2B_X4
            | Validator::A3B_X4 => true,
        }
    }

    pub(crate) fn requires_file_provenance_information(&self) -> bool {
        match self {
            Validator::None => false,
            Validator::A1_A | Validator::A1_B => true,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => true,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => true,
            Validator::A4 | Validator::A4F | Validator::A4E => true,
            Validator::UA1 => false,
            Validator::UA2 | Validator::WTPDF => false,
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P => false,
            // Composites inherit from PDF/A.
            Validator::A1B_X1A | Validator::A2B_X4 | Validator::A3B_X4 => true,
        }
    }

    pub(crate) fn prohibits_instance_id_in_xmp_metadata(&self) -> bool {
        match self {
            Validator::None => false,
            Validator::A1_A | Validator::A1_B => true,
            Validator::A2_A | Validator::A2_B | Validator::A2_U => false,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => false,
            Validator::A4 | Validator::A4F | Validator::A4E => false,
            Validator::UA1 => false,
            Validator::UA2 | Validator::WTPDF => false,
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P => false,
            // A1B_X1A inherits from A1_B.
            Validator::A1B_X1A => true,
            Validator::A2B_X4 | Validator::A3B_X4 => false,
        }
    }

    /// Return the output intent subtypes required by this validator.
    pub(crate) fn output_intents(&self) -> Vec<OutputIntentSubtype<'_>> {
        match self {
            Validator::None | Validator::UA1 | Validator::UA2 | Validator::WTPDF => vec![],
            Validator::A1_A | Validator::A1_B => vec![OutputIntentSubtype::PDFA],
            Validator::A2_A | Validator::A2_B | Validator::A2_U => {
                vec![OutputIntentSubtype::PDFA]
            }
            Validator::A3_A | Validator::A3_B | Validator::A3_U => {
                vec![OutputIntentSubtype::PDFA]
            }
            Validator::A4 | Validator::A4F | Validator::A4E => vec![OutputIntentSubtype::PDFA],
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P => vec![OutputIntentSubtype::PDFX],
            // Composites need both output intents.
            Validator::A1B_X1A | Validator::A2B_X4 | Validator::A3B_X4 => {
                vec![OutputIntentSubtype::PDFA, OutputIntentSubtype::PDFX]
            }
        }
    }

    pub(crate) fn allows_info_dict(&self) -> bool {
        match self {
            Validator::None
            | Validator::A1_A
            | Validator::A1_B
            | Validator::A2_A
            | Validator::A2_B
            | Validator::A2_U
            | Validator::A3_A
            | Validator::A3_B
            | Validator::A3_U
            | Validator::UA1 => true,
            // ISO 32000-2 deprecates the Info dictionary but still permits
            // CreationDate / ModDate entries; UA-2 and WTPDF inherit that
            // stance. Krilla's PDF 2.0 path already restricts the Info dict
            // to those two fields, so we keep it on rather than suppressing
            // it entirely.
            Validator::UA2 | Validator::WTPDF => true,
            Validator::A4 | Validator::A4F | Validator::A4E => false,
            Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P
            | Validator::A1B_X1A
            | Validator::A2B_X4
            | Validator::A3B_X4 => true,
        }
    }

    pub(crate) fn write_embedded_files(&self, is_empty: bool) -> bool {
        match self {
            Validator::None
            | Validator::A1_A
            | Validator::A1_B
            | Validator::A2_A
            | Validator::A2_B
            | Validator::A2_U
            | Validator::A3_A
            | Validator::A3_B
            | Validator::A3_U
            | Validator::A4
            | Validator::A4E
            | Validator::UA1
            | Validator::UA2
            | Validator::WTPDF
            | Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P
            | Validator::A1B_X1A
            | Validator::A2B_X4
            | Validator::A3B_X4 => !is_empty,
            // For this one we always need to write an `EmbeddedFiles` entry,
            // even if empty.
            Validator::A4F => true,
        }
    }

    pub(crate) fn allows_associated_files(&self) -> bool {
        match self {
            // PDF 2.0 _does_ support associated files. However, in this case the document has to
            // provide a modification date, since it's a required field. Therefore, it's easier to
            // just use the associated files feature, apart from PDF/A-3.
            Validator::None => false,
            Validator::A3_A | Validator::A3_B | Validator::A3_U => true,
            Validator::A4 | Validator::A4F | Validator::A4E => true,
            // PDF 2.0 supports associated files; both PDF/UA-2 and WTPDF
            // permit them.
            Validator::UA2 | Validator::WTPDF => true,
            Validator::A1_A
            | Validator::A1_B
            | Validator::A2_A
            | Validator::A2_B
            | Validator::A2_U
            | Validator::UA1
            | Validator::X1A
            | Validator::X3
            | Validator::X4
            | Validator::X4P
            | Validator::X6
            | Validator::X6P
            | Validator::A1B_X1A
            | Validator::A2B_X4 => false,
            // A3B_X4 inherits from A3_B which allows associated files.
            Validator::A3B_X4 => true,
        }
    }

    /// The string representation of the validator.
    pub fn as_str(self) -> &'static str {
        match self {
            Validator::None => "None",
            Validator::A1_A => "PDF/A-1a",
            Validator::A1_B => "PDF/A-1b",
            Validator::A2_A => "PDF/A-2a",
            Validator::A2_B => "PDF/A-2b",
            Validator::A2_U => "PDF/A-2u",
            Validator::A3_A => "PDF/A-3a",
            Validator::A3_B => "PDF/A-3b",
            Validator::A3_U => "PDF/A-3u",
            Validator::A4 => "PDF/A-4",
            Validator::A4F => "PDF/A-4f",
            Validator::A4E => "PDF/A-4e",
            Validator::UA1 => "PDF/UA-1",
            Validator::UA2 => "PDF/UA-2",
            Validator::WTPDF => "WTPDF 1.0",
            Validator::X1A => "PDF/X-1a",
            Validator::X3 => "PDF/X-3",
            Validator::X4 => "PDF/X-4",
            Validator::X4P => "PDF/X-4p",
            Validator::X6 => "PDF/X-6",
            Validator::X6P => "PDF/X-6p",
            Validator::A1B_X1A => "PDF/A-1b + PDF/X-1a",
            Validator::A2B_X4 => "PDF/A-2b + PDF/X-4",
            Validator::A3B_X4 => "PDF/A-3b + PDF/X-4",
        }
    }
}

impl core::fmt::Display for Validator {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Validator {
    /// Whether this validator requires CMYK-only colors (no RGB).
    pub(crate) fn requires_cmyk_only(&self) -> bool {
        matches!(self, Validator::X1A | Validator::A1B_X1A)
    }

    pub(crate) fn requires_external_output_profile(&self) -> bool {
        matches!(self, Validator::X4P | Validator::X6P)
    }

    pub(crate) fn requires_pdfx_extension_schema(&self) -> bool {
        self.is_pdf_a() && self.is_pdf_x()
    }

    /// Whether this validator forbids all annotations.
    pub(crate) fn forbids_annotations(&self) -> bool {
        matches!(self, Validator::X1A | Validator::A1B_X1A)
    }

    /// Whether this validator requires a TrimBox or ArtBox on every page.
    pub(crate) fn requires_trim_or_art_box(&self) -> bool {
        self.is_pdf_x()
    }

    /// Whether this validator requires trapping metadata to be written.
    pub(crate) fn requires_trapping_metadata(&self) -> bool {
        self.is_pdf_x()
    }

    /// Whether a CMYK output profile should be used when emitting the
    /// OutputIntent dictionary for the given subtype.
    ///
    /// For plain PDF/X validators this is true for the PDFX subtype; the PDFA
    /// subtype (if emitted) uses sRGB.
    ///
    /// For combined PDF/A + PDF/X validators this returns `true` for **both**
    /// subtypes: the PDFA and PDFX OutputIntent dictionaries reference the
    /// same CMYK device target. That is expected by prepress workflows and is
    /// permitted by ISO 15930-7 (PDF/X-4) and ISO 19005-2 §6.2.2 (PDF/A-2),
    /// which allow multiple OutputIntents provided they name the same output
    /// condition.
    pub(crate) fn uses_cmyk_output_profile_for_subtype(
        &self,
        subtype: OutputIntentSubtype<'_>,
    ) -> bool {
        self.is_pdf_x()
            && !self.requires_external_output_profile()
            && (subtype == OutputIntentSubtype::PDFX || self.is_pdf_a())
    }

    pub(crate) fn requires_xmp_metadata_date(&self) -> bool {
        matches!(
            self,
            Validator::X4
                | Validator::X4P
                | Validator::X6
                | Validator::X6P
                | Validator::A2B_X4
                | Validator::A3B_X4
        )
    }

    pub(crate) fn requires_xmp_version_id(&self) -> bool {
        matches!(
            self,
            Validator::X4
                | Validator::X4P
                | Validator::X6
                | Validator::X6P
                | Validator::A2B_X4
                | Validator::A3B_X4
        )
    }

    /// The GTS_PDFXVersion identification string for this validator.
    ///
    /// Returns `None` for non-PDF/X validators.
    pub(crate) fn gts_pdfx_version_string(&self) -> Option<&'static str> {
        match self {
            Validator::None
            | Validator::A1_A
            | Validator::A1_B
            | Validator::A2_A
            | Validator::A2_B
            | Validator::A2_U
            | Validator::A3_A
            | Validator::A3_B
            | Validator::A3_U
            | Validator::A4
            | Validator::A4F
            | Validator::A4E
            | Validator::UA1
            | Validator::UA2
            | Validator::WTPDF => None,
            Validator::X1A | Validator::A1B_X1A => Some("PDF/X-1a:2003"),
            Validator::X3 => Some("PDF/X-3:2003"),
            Validator::X4 | Validator::A2B_X4 | Validator::A3B_X4 => Some("PDF/X-4"),
            Validator::X4P => Some("PDF/X-4p"),
            Validator::X6 => Some("PDF/X-6"),
            Validator::X6P => Some("PDF/X-6p"),
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

    /// Register a colorant and its fallback and raise an error if it already
    /// exists.
    pub(crate) fn validate_separation(
        &mut self,
        separation: &SeparationSpace,
    ) -> Result<(), ValidationError> {
        if self
            .separation_fallback_map
            .entry(separation.colorant.clone())
            .or_insert(separation.fallback)
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
