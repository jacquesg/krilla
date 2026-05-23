//! Setting document metadata.
//!
//! PDF allows for the inclusion of metadata in a PDF document. To do so in krilla,
//! you can simply create a [`Metadata`] object, set the data, and then include it
//! in the document via [`Document::set_metadata`].
//!
//! [`Document::set_metadata`]: crate::document::Document::set_metadata
use pdf_writer::types::TrappingStatus;
use pdf_writer::{Finish, Name, Pdf, Ref, TextStr};
use std::cell::LazyCell;
use std::ops::DerefMut;
use xmp_writer::{LangId, Namespace, Timezone, XmpWriter};

use crate::configure::{Configuration, PdfVersion, ValidationError, Validators};
use crate::serialize::SerializeContext;

/// Metadata for a PDF document.
#[derive(Default, Clone, Debug)]
pub struct Metadata {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) creator: Option<String>,
    pub(crate) producer: Option<String>,
    pub(crate) keywords: Option<Vec<String>>,
    pub(crate) authors: Option<Vec<String>>,
    pub(crate) document_id: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) creation_date: Option<DateTime>,
    pub(crate) text_direction: Option<TextDirection>,
    pub(crate) page_layout: Option<PageLayout>,
    pub(crate) page_mode: Option<PageMode>,
    pub(crate) viewer_preferences: ViewerPreferences,
    pub(crate) trapped: Option<Trapping>,
    /// G5b — `/OpenAction` document action (ISO 32000-2 §12.6.4.3).
    /// `None` (the default) emits no `/OpenAction` slot in the
    /// catalogue dictionary; PDF readers open the document at the
    /// first page at their preferred zoom level.
    pub(crate) open_action: Option<OpenAction>,
    /// Author-supplied verbatim XMP packet. When set, it replaces the
    /// stream payload that krilla would otherwise build via [`XmpWriter`].
    /// See [`Metadata::raw_xmp`].
    pub(crate) raw_xmp: Option<Vec<u8>>,
}

/// Trapping status for a PDF document.
///
/// PDF/X-1a through PDF/X-6p require the `/Trapped` entry in the Document
/// Info dictionary to be either `/True` or `/False`. `Trapping::Unknown`
/// is permitted by base PDF but forbidden by PDF/X; krilla downgrades it
/// to `NotTrapped` in that case to keep the output conformant.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum Trapping {
    /// The document has been fully trapped for prepress.
    Trapped,
    /// The document has not been trapped.
    NotTrapped,
    /// Trapping state is unspecified. Not permitted by PDF/X.
    Unknown,
}

impl Trapping {
    fn to_pdf_status(self) -> TrappingStatus {
        match self {
            Trapping::Trapped => TrappingStatus::Trapped,
            Trapping::NotTrapped => TrappingStatus::NotTrapped,
            Trapping::Unknown => TrappingStatus::Unknown,
        }
    }
}

fn resolve_trapping(trapped: Option<Trapping>, validators: Validators) -> Trapping {
    match trapped {
        Some(Trapping::Unknown) if validators.requires_trapping_metadata() => {
            Trapping::NotTrapped
        }
        Some(t) => t,
        None if validators.requires_trapping_metadata() => Trapping::NotTrapped,
        None => Trapping::Unknown,
    }
}

impl Metadata {
    /// Create new metadata.
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    /// The title of the document.
    pub fn title(mut self, title: String) -> Self {
        if !title.is_empty() {
            self.title = Some(title);
        }
        self
    }

    /// The description of the document.
    ///
    /// This should be a short, human-readable abstract, summary, or description
    /// of the topic of the document.
    pub fn description(mut self, description: String) -> Self {
        if !description.is_empty() {
            self.description = Some(description);
        }
        self
    }

    /// The keywords that describe the document.
    pub fn keywords(mut self, keywords: Vec<String>) -> Self {
        if !keywords.is_empty() {
            self.keywords = Some(keywords);
        }
        self
    }

    /// The main language of the document, as an RFC 3066 language tag.
    ///
    /// This property is required for some export modes, like for example PDF/A-3a.
    pub fn language(mut self, language: String) -> Self {
        self.language = Some(language);
        self
    }

    /// The creator tool of the document.
    pub fn creator(mut self, creator: String) -> Self {
        if !creator.is_empty() {
            self.creator = Some(creator);
        }
        self
    }

    /// The producer tool of the document.
    pub fn producer(mut self, producer: String) -> Self {
        if !producer.is_empty() {
            self.producer = Some(producer);
        }
        self
    }

    /// The authors of the document.
    pub fn authors(mut self, authors: Vec<String>) -> Self {
        if !authors.is_empty() {
            self.authors = Some(authors);
        }
        self
    }

    /// The creation date of the document.
    pub fn creation_date(mut self, creation_date: DateTime) -> Self {
        self.creation_date = Some(creation_date);
        self
    }

    /// A document ID.
    ///
    /// This attribute will be used as an identifier for identifying
    /// different versions of the same document.
    pub fn document_id(mut self, document_id: String) -> Self {
        self.document_id = Some(document_id);
        self
    }

    /// The main text direction of the document.
    pub fn text_direction(mut self, text_direction: TextDirection) -> Self {
        self.text_direction = Some(text_direction);
        self
    }

    /// How the viewer should lay out the pages.
    pub fn page_layout(mut self, page_layout: PageLayout) -> Self {
        self.page_layout = Some(page_layout);
        self
    }

    /// Which document chrome (outlines, thumbs, full-screen, …) the
    /// viewer should display when the document is opened.
    pub fn page_mode(mut self, page_mode: PageMode) -> Self {
        self.page_mode = Some(page_mode);
        self
    }

    /// Set the document's `/ViewerPreferences` dictionary (ISO
    /// 32000-2 §12.4.4). Every field on [`ViewerPreferences`] is
    /// optional; absent fields are omitted from the emitted dictionary.
    /// The `Direction` and `DisplayDocTitle` slots interoperate with
    /// the existing [`Self::text_direction`] hint and the
    /// validator-driven `DisplayDocTitle` enforcement (PDF/UA-1) —
    /// values from this struct take precedence when set.
    pub fn viewer_preferences(mut self, preferences: ViewerPreferences) -> Self {
        self.viewer_preferences = preferences;
        self
    }

    /// Whether the document has been adjusted with traps for colorant
    /// misregistration during the printing process.
    ///
    /// This property is required for PDF/X export modes. If not set for
    /// PDF/X, it will default to [`Trapping::NotTrapped`]. PDF/X forbids
    /// [`Trapping::Unknown`]; krilla downgrades it to `NotTrapped` when a
    /// PDF/X validator is active.
    pub fn trapped(mut self, trapped: Trapping) -> Self {
        self.trapped = Some(trapped);
        self
    }

    /// Set the document's `/OpenAction` (ISO 32000-2 §12.6.4.3).
    ///
    /// The viewer executes this action when the document is opened.
    /// Krilla supports the `[<page-ref> <destination>]` direct-link
    /// form via [`OpenAction::go_to_page_with_zoom`]; this is the only
    /// form needed for the moegoe G5b PDFreactor parity surface
    /// (`-bd-initial-page` + `-bd-initial-zoom`). Other forms
    /// (`/JavaScript`, `/Named`, `/SubmitForm`, etc.) are not exposed.
    ///
    /// The 0-indexed page reference is resolved at serialise time
    /// against the `PageInfo` table; an out-of-range page index is
    /// not detected here (it would panic during serialisation
    /// mirroring `XyzDestination::serialize`'s behaviour). Callers
    /// must clamp into `[0, page_count)` before invoking this
    /// setter.
    pub fn open_action(mut self, action: OpenAction) -> Self {
        self.open_action = Some(action);
        self
    }

    /// Override the XMP metadata stream with the supplied bytes verbatim.
    ///
    /// When set, the `/Metadata` stream the catalogue points at will contain
    /// exactly the bytes the caller supplied (typically a fully formed
    /// `<?xpacket begin="..."?>...<?xpacket end="w"?>` packet), and krilla
    /// will not invoke [`XmpWriter`] for this document. The fields normally
    /// reflected into XMP (title, authors, dates, validator metadata, etc.)
    /// are still written to the Document Information dictionary where
    /// applicable, but they will not be merged into the XMP packet — the
    /// caller is responsible for the full contents.
    ///
    /// Supplying raw XMP forces emission of a `/Metadata` stream regardless
    /// of [`crate::SerializeSettings::xmp_metadata`]: the explicit override
    /// is treated as opt-in.
    ///
    /// Default behaviour (no call to this method) is unchanged.
    ///
    /// [`XmpWriter`]: xmp_writer::XmpWriter
    pub fn raw_xmp(mut self, bytes: Vec<u8>) -> Self {
        self.raw_xmp = Some(bytes);
        self
    }


    pub(crate) fn has_document_info(&self) -> bool {
        self.title.is_some()
            || self.producer.is_some()
            || self.keywords.is_some()
            || self.authors.is_some()
            || self.creator.is_some()
            || self.creation_date.is_some()
            || self.description.is_some()
    }

    pub(crate) fn serialize_xmp_metadata(
        &self,
        xmp: &mut XmpWriter,
        sc: &mut SerializeContext,
        instance_id: &str,
    ) {
        if let Some(title) = &self.title {
            xmp.title([(None, title.as_str())]);
        }

        if let Some(description) = &self.description {
            xmp.description([(None, description.as_str())]);
        }

        if let Some(keywords) = &self.keywords {
            let joined = keywords.join(", ");
            xmp.pdf_keywords(joined.as_str());
        }

        match &self.authors {
            Some(authors) if sc.serialize_settings().pdf_version() >= PdfVersion::Pdf20 => {
                // PDF 2.0+ deprecates the document information dictionary, so
                // we can use the array here.
                xmp.creator(authors.iter().map(String::as_str));
            }
            Some(authors) => {
                // Turns out that if the authors are given in both the document
                // information dictionary and the XMP metadata, Acrobat takes a
                // little bit of both: The first author from the document
                // information dictionary and the remaining authors from the XMP
                // metadata.
                //
                // To fix this for Acrobat, we could omit the remaining authors
                // or all metadata from the document information catalog (it is
                // optional) and only write XMP. However, not all other tools
                // (including Apple Preview) read the XMP data. This means we do
                // want to include all authors in the document information
                // dictionary.
                //
                // Thus, the only alternative is to fold all authors into a
                // single `<rdf:li>` in the XMP metadata. This is, in fact,
                // exactly what the PDF/A spec Part 1 section 6.7.3 has to say
                // about the matter. It's a bit weird to not use the array (and
                // it makes Acrobat show the author list in quotes), but there's
                // not much we can do about that.
                let joined = authors.join(", ");
                xmp.creator([joined.as_str()]);
            }
            None => {}
        }

        if let Some(creator) = &self.creator {
            xmp.creator_tool(creator);
        }

        if let Some(producer) = &self.producer {
            xmp.producer(producer);
        }

        if let Some(lang) = &self.language {
            xmp.language([LangId(lang)]);
        }

        if let Some(date) = self.creation_date.map(xmp_date) {
            xmp.modify_date(date);
            xmp.create_date(date);

            if sc
                .serialize_settings()
                .validators()
                .requires_file_provenance_information()
            {
                let mut history = xmp.history();
                let mut saved = history.add_event();

                saved
                    .action(xmp_writer::ResourceEventAction::Saved)
                    .when(date);

                if !sc
                    .serialize_settings()
                    .validators()
                    .prohibits_instance_id_in_xmp_metadata()
                {
                    saved.instance_id(&format!("{instance_id}_source"));
                }

                saved.finish();

                let mut converted = history.add_event();

                converted
                    .action(xmp_writer::ResourceEventAction::Converted)
                    .when(date);

                if let Some(creator) = &self.creator {
                    converted.software_agent(creator);
                }

                if !sc
                    .serialize_settings()
                    .validators()
                    .prohibits_instance_id_in_xmp_metadata()
                {
                    converted.instance_id(&format!("{instance_id}_source"));
                }
            }
        } else {
            sc.register_validation_error(ValidationError::MissingDocumentDate);
        }

        // PDF/X: write `pdf:Trapped` in XMP metadata. Mirrors the Info-dict
        // path so the two streams agree. PDF/X forbids the `Unknown`
        // state, so when the caller supplies `Unknown` under a PDF/X
        // validator we downgrade to `NotTrapped`.
        let validators = sc.serialize_settings().validators();
        if validators.requires_trapping_metadata() || self.trapped.is_some() {
            match resolve_trapping(self.trapped, validators) {
                Trapping::Trapped => {
                    xmp.trapped(true);
                }
                Trapping::NotTrapped => {
                    xmp.trapped(false);
                }
                Trapping::Unknown => {
                    xmp.element("Trapped", Namespace::AdobePdf).value("Unknown");
                }
            }
        }
    }

    pub(crate) fn serialize_document_info(
        &self,
        ref_: &mut Ref,
        pdf: &mut Pdf,
        config: Configuration,
    ) {
        if config.validators().prohibits_info_dict() {
            return;
        }

        // PDF/X (ISO 15930-*) mandates `/GTS_PDFXVersion` and `/Trapped` in
        // the Info dictionary even on PDF 2.0 (ISO 15930-9 still requires
        // them despite the general PDF 2.0 Info-dict deprecation). Force
        // the dict to be written under any PDF/X validator. A caller-set
        // `/Trapped` value also forces emission outside PDF/X so the Info
        // and XMP streams stay in agreement.
        let requires_pdfx_info = config.validators().requires_pdfx_identification()
            || config.validators().requires_trapping_metadata()
            || self.trapped.is_some();

        // Under PDF 2.0 the per-field string entries (title, producer,
        // etc.) are deprecated and not written — only `creation_date`
        // and the PDF/X-mandated entries remain. Guard the ref bump
        // with a version-aware will-write predicate so that a caller
        // who sets `producer` (or any other deprecated field) on a
        // PDF 2.0 document does not consume a ref without emitting a
        // corresponding Info dict object.
        let will_write = if config.version() < PdfVersion::Pdf20 {
            self.has_document_info() || requires_pdfx_info
        } else {
            self.creation_date.is_some() || requires_pdfx_info
        };

        if will_write {
            let ref_ = ref_.bump();
            let mut document_info = LazyCell::new(|| pdf.document_info(ref_));

            // ALl of those are deprecated in PDF 2.0 and will only be written
            // to the XMP metadata.
            if config.version() < PdfVersion::Pdf20 {
                if let Some(title) = &self.title {
                    document_info.title(TextStr(title));
                }

                if let Some(description) = &self.description {
                    document_info.subject(TextStr(description));
                }

                if let Some(keywords) = &self.keywords {
                    let joined = keywords.join(", ");
                    document_info.keywords(TextStr(&joined));
                }

                if let Some(authors) = &self.authors {
                    let joined = authors.join(", ");
                    document_info.author(TextStr(&joined));
                }

                if let Some(creator) = &self.creator {
                    document_info.creator(TextStr(creator));
                }

                if let Some(producer) = &self.producer {
                    document_info.producer(TextStr(producer));
                }
            }

            if let Some(date_time) = self.creation_date {
                document_info.modified_date(pdf_date(date_time));
                document_info.creation_date(pdf_date(date_time));
            }

            // PDF/X identification: `/GTS_PDFXVersion (PDF/X-...)` per
            // ISO 32000-2 Annex K.2.
            if let Some(version) = config.validators().gts_pdfx_version_string() {
                document_info
                    .deref_mut()
                    .pair(Name(b"GTS_PDFXVersion"), TextStr(version));
            }

            // `/Trapped` is mandated by every PDF/X revision; callers may
            // also set it explicitly outside PDF/X. `resolve_trapping`
            // downgrades `Unknown` to `NotTrapped` under PDF/X (which
            // forbids `Unknown`) and defaults to `NotTrapped` when a
            // PDF/X validator is active without an explicit caller value.
            if config.validators().requires_trapping_metadata() || self.trapped.is_some() {
                let status = resolve_trapping(self.trapped, config.validators()).to_pdf_status();
                let name = match status {
                    TrappingStatus::Trapped => Name(b"True"),
                    TrappingStatus::NotTrapped => Name(b"False"),
                    TrappingStatus::Unknown => Name(b"Unknown"),
                };
                document_info.deref_mut().pair(Name(b"Trapped"), name);
            }
        }
    }
}

/// A datetime. Invalid values will be clamped.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct DateTime {
    /// The year (0-9999).
    pub(crate) year: u16,
    /// The month (0-11).
    pub(crate) month: Option<u8>,
    /// The day (0-31).
    pub(crate) day: Option<u8>,
    /// The hour (0-23).
    pub(crate) hour: Option<u8>,
    /// The minute (0-59).
    pub(crate) minute: Option<u8>,
    /// The second (0-59).
    pub(crate) second: Option<u8>,
    /// The hour offset from UTC (-23 through 23).
    pub(crate) utc_offset_hour: Option<i8>,
    /// The minute offset from UTC (0-59). Will carry over the sign from
    /// `utc_offset_hour`.
    pub(crate) utc_offset_minute: u8,
}

impl DateTime {
    /// Create a new, minimal date. The year will be clamped within the range
    /// 0-9999.
    #[inline]
    pub fn new(year: u16) -> Self {
        Self {
            year: year.min(9999),
            month: None,
            day: None,
            hour: None,
            minute: None,
            second: None,
            utc_offset_hour: None,
            utc_offset_minute: 0,
        }
    }

    /// Add the month field. It will be clamped within the range 1-12.
    #[inline]
    pub fn month(mut self, month: u8) -> Self {
        self.month = Some(month.clamp(1, 12));
        self
    }

    /// Add the day field. It will be clamped within the range 1-31.
    #[inline]
    pub fn day(mut self, day: u8) -> Self {
        self.day = Some(day.clamp(1, 31));
        self
    }

    /// Add the hour field. It will be clamped within the range 0-23.
    #[inline]
    pub fn hour(mut self, hour: u8) -> Self {
        self.hour = Some(hour.min(23));
        self
    }

    /// Add the minute field. It will be clamped within the range 0-59.
    #[inline]
    pub fn minute(mut self, minute: u8) -> Self {
        self.minute = Some(minute.min(59));
        self
    }

    /// Add the second field. It will be clamped within the range 0-59.
    #[inline]
    pub fn second(mut self, second: u8) -> Self {
        self.second = Some(second.min(59));
        self
    }

    /// Add the offset from UTC in hours. If not specified, the time will be
    /// assumed to be local to the viewer's time zone. It will be clamped within
    /// the range -23-23.
    #[inline]
    pub fn utc_offset_hour(mut self, hour: i8) -> Self {
        self.utc_offset_hour = Some(hour.clamp(-23, 23));
        self
    }

    /// Add the offset from UTC in minutes. This will have the same sign as set in
    /// [`Self::utc_offset_hour`]. It will be clamped within the range 0-59.
    #[inline]
    pub fn utc_offset_minute(mut self, minute: u8) -> Self {
        self.utc_offset_minute = minute.min(59);
        self
    }
}

/// Converts a datetime to a pdf-writer date.
pub(crate) fn pdf_date(date_time: DateTime) -> pdf_writer::Date {
    // We always assume a full date with all fields because for some reason
    // Acrobat doesn't like PDF/A-1 files without everything set.
    pdf_writer::Date::new(date_time.year)
        .month(date_time.month.unwrap_or(1))
        .day(date_time.day.unwrap_or(1))
        .hour(date_time.hour.unwrap_or(0))
        .minute(date_time.minute.unwrap_or(0))
        .second(date_time.second.unwrap_or(0))
        .utc_offset_hour(date_time.utc_offset_hour.unwrap_or(0))
        .utc_offset_minute(date_time.utc_offset_minute)
}

/// Converts a datetime to an xmp-writer datetime.
fn xmp_date(datetime: DateTime) -> xmp_writer::DateTime {
    let timezone = match (datetime.utc_offset_hour, datetime.utc_offset_minute) {
        (Some(h), m) => Some(Timezone::Local {
            hour: h,
            minute: m as i8,
        }),
        _ => Some(Timezone::Utc),
    };

    // We always assume a full date with all fields because for some reason
    // Acrobat doesn't like PDF/A-1 files without everything set.
    xmp_writer::DateTime {
        year: datetime.year,
        month: Some(datetime.month.unwrap_or(1)),
        day: Some(datetime.day.unwrap_or(1)),
        hour: Some(datetime.hour.unwrap_or(0)),
        minute: Some(datetime.minute.unwrap_or(0)),
        second: Some(datetime.second.unwrap_or(0)),
        timezone,
    }
}

/// `/OpenAction` document action (ISO 32000-2 §12.6.4.3).
///
/// Currently only the direct-link `[<page-ref> <destination>]`
/// form is exposed. Krilla serialises this as a single-line
/// array entry on the catalogue dictionary; the destination
/// flavour comes from [`OpenZoom`].
#[derive(Copy, Clone, Debug)]
pub struct OpenAction {
    /// 0-indexed page the viewer should open on. Resolved against
    /// the document's `PageInfo` table at serialise time; an
    /// out-of-range index panics in the same way `XyzDestination`
    /// does.
    pub(crate) page_index: usize,
    /// Destination flavour.
    pub(crate) zoom: OpenZoom,
}

impl OpenAction {
    /// Build an `/OpenAction` direct-link entry that opens
    /// `page_index` (0-indexed) at the given destination flavour.
    pub fn go_to_page_with_zoom(page_index: usize, zoom: OpenZoom) -> Self {
        Self { page_index, zoom }
    }
}

/// Destination flavour for `/OpenAction` (ISO 32000-2 §12.3.2).
///
/// Each variant maps onto one of the eight destination arrays the
/// PDF spec accepts in this slot:
/// `Xyz(zoom)` → `[<page> /XYZ null null <zoom>]`;
/// `FitPage` → `[<page> /Fit]`;
/// `FitHorizontalToWidth` → `[<page> /FitH null]`;
/// `FitVerticalToHeight` → `[<page> /FitV null]`;
/// `FitBoundingBox` → `[<page> /FitB]`;
/// `FitBoundingBoxHorizontal` → `[<page> /FitBH null]`;
/// `FitBoundingBoxVertical` → `[<page> /FitBV null]`.
#[derive(Copy, Clone, Debug)]
pub enum OpenZoom {
    /// `/XYZ null null <zoom>` — open at an explicit zoom factor.
    /// `1.0` = 100 %. Negative or zero values are silently treated
    /// as "viewer default" by conforming readers, per the spec.
    Xyz(f32),
    /// `/Fit` — fit the entire page into the viewer window.
    FitPage,
    /// `/FitH null` — fit the page's width to the window; vertical
    /// position chosen by the viewer.
    FitHorizontalToWidth,
    /// `/FitV null` — fit the page's height to the window;
    /// horizontal position chosen by the viewer.
    FitVerticalToHeight,
    /// `/FitB` — fit the page's bounding box into the window.
    FitBoundingBox,
    /// `/FitBH null` — fit the bounding-box width to the window.
    FitBoundingBoxHorizontal,
    /// `/FitBV null` — fit the bounding-box height to the window.
    FitBoundingBoxVertical,
}

/// The main text direction of the document.
#[allow(missing_docs)]
#[derive(Copy, Clone, Debug)]
pub enum TextDirection {
    LeftToRight,
    RightToLeft,
}

impl TextDirection {
    pub(crate) fn to_pdf(self) -> pdf_writer::types::Direction {
        match self {
            TextDirection::LeftToRight => pdf_writer::types::Direction::L2R,
            TextDirection::RightToLeft => pdf_writer::types::Direction::R2L,
        }
    }
}

/// How the viewer should lay out the pages.
#[derive(Copy, Clone, Debug)]
pub enum PageLayout {
    /// Only a single page at a time.
    SinglePage,
    /// A single, continuously scrolling column of pages.
    OneColumn,
    /// Two continuously scrolling columns of pages, laid out with odd-numbered
    /// pages on the left.
    TwoColumnLeft,
    /// Two continuously scrolling columns of pages, laid out with odd-numbered
    /// pages on the right (like in a left-bound book).
    TwoColumnRight,
    /// Only two pages are visible at a time, laid out with odd-numbered pages
    /// on the left. PDF 1.5+.
    TwoPageLeft,
    /// Only two pages are visible at a time, laid out with odd-numbered pages
    /// on the right (like in a left-bound book). PDF 1.5+.
    TwoPageRight,
}

impl PageLayout {
    pub(crate) fn to_pdf(self) -> pdf_writer::types::PageLayout {
        match self {
            PageLayout::SinglePage => pdf_writer::types::PageLayout::SinglePage,
            PageLayout::OneColumn => pdf_writer::types::PageLayout::OneColumn,
            PageLayout::TwoColumnLeft => pdf_writer::types::PageLayout::TwoColumnLeft,
            PageLayout::TwoColumnRight => pdf_writer::types::PageLayout::TwoColumnRight,
            PageLayout::TwoPageLeft => pdf_writer::types::PageLayout::TwoPageLeft,
            PageLayout::TwoPageRight => pdf_writer::types::PageLayout::TwoPageRight,
        }
    }
}

/// Which document chrome the viewer should display when the document
/// is first opened (ISO 32000-2 §7.7.3.4, `/PageMode`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PageMode {
    /// Neither the document outline panel nor a panel with page preview
    /// images are visible.
    UseNone,
    /// The document outline panel is visible.
    UseOutlines,
    /// A panel with page preview images is visible.
    UseThumbs,
    /// Show the document page in full screen mode, with no chrome.
    FullScreen,
    /// Show the optional content group panel. PDF 1.5+.
    UseOC,
    /// Show the attachments panel. PDF 1.6+.
    UseAttachments,
}

impl PageMode {
    pub(crate) fn to_pdf(self) -> pdf_writer::types::PageMode {
        match self {
            PageMode::UseNone => pdf_writer::types::PageMode::UseNone,
            PageMode::UseOutlines => pdf_writer::types::PageMode::UseOutlines,
            PageMode::UseThumbs => pdf_writer::types::PageMode::UseThumbs,
            PageMode::FullScreen => pdf_writer::types::PageMode::FullScreen,
            PageMode::UseOC => pdf_writer::types::PageMode::UseOC,
            PageMode::UseAttachments => pdf_writer::types::PageMode::UseAttachments,
        }
    }
}

/// Page mode shown when the viewer is NOT in full-screen mode
/// (`/ViewerPreferences /NonFullScreenPageMode`). Strict subset of
/// [`PageMode`]; ISO 32000-2 §12.4.4 forbids `FullScreen` here.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(missing_docs)]
pub enum NonFullScreenPageMode {
    UseNone,
    UseOutlines,
    UseThumbs,
    UseOC,
}

impl NonFullScreenPageMode {
    pub(crate) fn to_pdf(self) -> pdf_writer::types::PageMode {
        match self {
            NonFullScreenPageMode::UseNone => pdf_writer::types::PageMode::UseNone,
            NonFullScreenPageMode::UseOutlines => pdf_writer::types::PageMode::UseOutlines,
            NonFullScreenPageMode::UseThumbs => pdf_writer::types::PageMode::UseThumbs,
            NonFullScreenPageMode::UseOC => pdf_writer::types::PageMode::UseOC,
        }
    }
}

/// Print-dialog page-scaling preference
/// (`/ViewerPreferences /PrintScaling`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(missing_docs)]
pub enum PrintScaling {
    /// No page scaling — print at 100%.
    None,
    /// Application default (the viewer's standard fit-to-page behaviour).
    AppDefault,
}

impl PrintScaling {
    pub(crate) fn to_pdf_name(self) -> Name<'static> {
        match self {
            PrintScaling::None => Name(b"None"),
            PrintScaling::AppDefault => Name(b"AppDefault"),
        }
    }
}

/// Duplex / simplex preference for the print dialog
/// (`/ViewerPreferences /Duplex`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(missing_docs)]
pub enum Duplex {
    Simplex,
    DuplexFlipShortEdge,
    DuplexFlipLongEdge,
}

impl Duplex {
    pub(crate) fn to_pdf_name(self) -> Name<'static> {
        match self {
            Duplex::Simplex => Name(b"Simplex"),
            Duplex::DuplexFlipShortEdge => Name(b"DuplexFlipShortEdge"),
            Duplex::DuplexFlipLongEdge => Name(b"DuplexFlipLongEdge"),
        }
    }
}

/// `/ViewerPreferences` dictionary entries (ISO 32000-2 §12.4.4).
///
/// Every field is optional. Setters on this struct return `Self` for
/// builder-style chaining; absent fields are not emitted in the PDF.
/// `direction` is set independently via [`Metadata::text_direction`];
/// supplying it here as part of a `ViewerPreferences` value
/// overrides the writing-mode-derived hint.
#[derive(Default, Clone, Debug)]
pub struct ViewerPreferences {
    pub(crate) hide_toolbar: Option<bool>,
    pub(crate) hide_menubar: Option<bool>,
    pub(crate) hide_window_ui: Option<bool>,
    pub(crate) fit_window: Option<bool>,
    pub(crate) center_window: Option<bool>,
    pub(crate) display_doc_title: Option<bool>,
    pub(crate) non_fullscreen_page_mode: Option<NonFullScreenPageMode>,
    pub(crate) print_scaling: Option<PrintScaling>,
    pub(crate) duplex: Option<Duplex>,
    pub(crate) pick_tray_by_pdf_size: Option<bool>,
}

impl ViewerPreferences {
    /// Create an empty viewer-preferences dictionary.
    pub fn new() -> Self {
        Self::default()
    }

    /// `/HideToolbar` — hide the viewer's toolbar while the document is open.
    pub fn hide_toolbar(mut self, hide: bool) -> Self {
        self.hide_toolbar = Some(hide);
        self
    }

    /// `/HideMenubar` — hide the viewer's menu bar while the document is open.
    pub fn hide_menubar(mut self, hide: bool) -> Self {
        self.hide_menubar = Some(hide);
        self
    }

    /// `/HideWindowUI` — hide the viewer's window-management controls.
    pub fn hide_window_ui(mut self, hide: bool) -> Self {
        self.hide_window_ui = Some(hide);
        self
    }

    /// `/FitWindow` — resize the viewer window to the size of the first page.
    pub fn fit_window(mut self, fit: bool) -> Self {
        self.fit_window = Some(fit);
        self
    }

    /// `/CenterWindow` — centre the viewer window on the screen.
    pub fn center_window(mut self, center: bool) -> Self {
        self.center_window = Some(center);
        self
    }

    /// `/DisplayDocTitle` — display the document's `/Title` rather than
    /// the file name in the viewer's title bar. Required `true` under
    /// PDF/UA-1.
    pub fn display_doc_title(mut self, display: bool) -> Self {
        self.display_doc_title = Some(display);
        self
    }

    /// `/NonFullScreenPageMode` — which chrome the viewer shows when
    /// the document is requesting full-screen but not currently
    /// in full-screen mode.
    pub fn non_fullscreen_page_mode(mut self, mode: NonFullScreenPageMode) -> Self {
        self.non_fullscreen_page_mode = Some(mode);
        self
    }

    /// `/PrintScaling` — default page-scaling preference for the print dialog.
    pub fn print_scaling(mut self, scaling: PrintScaling) -> Self {
        self.print_scaling = Some(scaling);
        self
    }

    /// `/Duplex` — duplex / simplex preference for the print dialog.
    pub fn duplex(mut self, duplex: Duplex) -> Self {
        self.duplex = Some(duplex);
        self
    }

    /// `/PickTrayByPDFSize` — automatically choose the paper tray by
    /// matching the page size.
    pub fn pick_tray_by_pdf_size(mut self, enabled: bool) -> Self {
        self.pick_tray_by_pdf_size = Some(enabled);
        self
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.hide_toolbar.is_none()
            && self.hide_menubar.is_none()
            && self.hide_window_ui.is_none()
            && self.fit_window.is_none()
            && self.center_window.is_none()
            && self.display_doc_title.is_none()
            && self.non_fullscreen_page_mode.is_none()
            && self.print_scaling.is_none()
            && self.duplex.is_none()
            && self.pick_tray_by_pdf_size.is_none()
    }
}
