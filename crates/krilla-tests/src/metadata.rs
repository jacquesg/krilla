use krilla::configure::{ConfigurationBuilder, Pdfx};
use krilla::metadata::{DateTime, Metadata, PageLayout, TextDirection, Trapping};
use krilla::Document;
use krilla::SerializeSettings;
use krilla_macros::snapshot;

fn datetime() -> DateTime {
    DateTime::new(2024)
        .month(11)
        .day(8)
        .hour(22)
        .minute(23)
        .second(18)
        .utc_offset_hour(1)
        .utc_offset_minute(12)
}

pub(crate) fn metadata_impl(document: &mut Document) {
    let date = datetime();
    let metadata = Metadata::new()
        .creation_date(date)
        .description("A very interesting subject".to_string())
        .creator("krilla".to_string())
        .producer("krilla".to_string())
        .language("en".to_string())
        .keywords(vec![
            "keyword1".to_string(),
            "keyword2".to_string(),
            "keyword3".to_string(),
        ])
        .title("An awesome title".to_string())
        .authors(vec!["John Doe".to_string(), "Max Mustermann".to_string()])
        .text_direction(TextDirection::LeftToRight)
        .page_layout(PageLayout::TwoColumnRight);
    document.set_metadata(metadata);
}

#[snapshot(document)]
fn metadata_empty(document: &mut Document) {
    let metadata = Metadata::new();
    document.set_metadata(metadata);
}

#[snapshot(document)]
fn metadata_full(document: &mut Document) {
    metadata_impl(document);
}

#[snapshot(document, settings_5)]
fn metadata_full_with_xmp(document: &mut Document) {
    metadata_impl(document);
}

#[snapshot(document, settings_30)]
fn metadata_pdf_20_author(document: &mut Document) {
    let metadata = Metadata::new()
        .authors(vec!["John Doe".to_string(), "Max Mustermann".to_string()])
        .creation_date(datetime());
    document.set_metadata(metadata);
}

// A page is required for a catalogue (and therefore a /Metadata entry)
// to be emitted; produce one with no content drawn.
fn minimal_page(document: &mut Document) {
    use krilla::page::PageSettings;
    let mut page = document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
    let surface = page.surface();
    surface.finish();
    page.finish();
}

/// Without [`Metadata::raw_xmp`], the catalogue must continue to emit
/// either krilla's generated XMP packet (when
/// [`SerializeSettings::xmp_metadata`] is set) or no `/Metadata` entry
/// at all. The marker the krilla writer always embeds is `W5M0MpCehiHzreSzNTczkc9d`
/// (the XMP packet UUID). Generated XMP always carries it.
#[test]
fn raw_xmp_default_unchanged() {
    use krilla::SerializeSettings;
    let mut settings = SerializeSettings::default();
    settings.xmp_metadata = true;
    let mut document = Document::new_with(settings);
    document.set_metadata(metadata_impl_metadata());
    minimal_page(&mut document);
    let pdf = document.finish().expect("finish should succeed");

    // Generated XMP contains the standard XMP-packet marker.
    assert!(
        memmem(&pdf, b"W5M0MpCehiHzreSzNTczkc9d"),
        "generated XMP packet marker should be present without raw_xmp override",
    );
}

/// With [`Metadata::raw_xmp`] set, the catalogue's `/Metadata` stream
/// must contain the author's bytes verbatim, and the generated XMP
/// packet marker must be absent.
#[test]
fn raw_xmp_overrides_stream() {
    use krilla::SerializeSettings;
    let payload =
        b"<?xpacket begin=\"\xef\xbb\xbf\" id=\"krilla-test-id\"?>moegoe-raw-xmp-token<?xpacket end=\"w\"?>";
    let mut settings = SerializeSettings::default();
    // Force xmp_metadata off — raw_xmp must still emit the /Metadata
    // stream because it is an explicit opt-in.
    settings.xmp_metadata = false;
    let mut document = Document::new_with(settings);
    document.set_metadata(metadata_impl_metadata().raw_xmp(payload.to_vec()));
    minimal_page(&mut document);
    let pdf = document.finish().expect("finish should succeed");

    assert!(
        memmem(&pdf, b"moegoe-raw-xmp-token"),
        "raw XMP payload should be present in the PDF",
    );
    assert!(
        !memmem(&pdf, b"W5M0MpCehiHzreSzNTczkc9d"),
        "generated XMP packet marker must not appear when raw_xmp is set",
    );
    assert!(
        memmem(&pdf, b"/Type /Metadata") || memmem(&pdf, b"/Type/Metadata"),
        "/Metadata stream dictionary must be emitted when raw_xmp is set",
    );
}

/// Construct a fresh [`Metadata`] equivalent to [`metadata_impl`] but
/// without taking a `&mut Document` — needed because builder chaining
/// consumes the receiver.
fn metadata_impl_metadata() -> Metadata {
    Metadata::new()
        .creation_date(datetime())
        .description("A very interesting subject".to_string())
        .creator("krilla".to_string())
        .producer("krilla".to_string())
        .language("en".to_string())
        .title("An awesome title".to_string())
        .authors(vec!["John Doe".to_string()])
}

/// Tiny needle-in-haystack scan; `slice::contains_slice` is unstable.
fn memmem(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Build a minimal X-4 document with the given trapping state and return
/// its bytes. PDF/X-4 requires a CMYK ICC profile and a TrimBox or
/// ArtBox on every page; this helper supplies both so the trapping
/// assertions are the only variable under test.
fn pdfx_doc_with_trapped(trapped: Option<Trapping>) -> Vec<u8> {
    use krilla::icc::ICCProfile;
    use krilla::page::PageSettings;
    let cmyk_bytes = std::fs::read(crate::ASSETS_PATH.join("icc/eciCMYK_v2.icc"))
        .expect("CMYK profile asset must exist");
    let cmyk = ICCProfile::<4>::new(&cmyk_bytes).expect("CMYK profile must parse");
    let config = ConfigurationBuilder::new()
        .with_pdfx_validator(Pdfx::X4)
        .finish()
        .expect("X4 configuration");
    let settings = SerializeSettings {
        configuration: config,
        cmyk_profile: Some(cmyk),
        ..crate::settings_1()
    };
    let mut document = Document::new_with(settings);
    let mut meta = Metadata::new()
        .creation_date(datetime())
        .title("trap test".to_string());
    if let Some(t) = trapped {
        meta = meta.trapped(t);
    }
    document.set_metadata(meta);
    let page_settings = PageSettings::default().with_trim_box(Some(
        krilla::geom::Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap(),
    ));
    let mut page = document.start_page_with(page_settings);
    page.surface().finish();
    page.finish();
    document.finish().expect("finish should succeed")
}

/// `/Trapped /True` lands in the Info dict and `pdf:Trapped` lands in
/// the XMP packet when the caller sets `Trapping::Trapped`.
#[test]
fn trapped_true_emits_both_streams() {
    let pdf = pdfx_doc_with_trapped(Some(Trapping::Trapped));
    assert!(
        memmem(&pdf, b"/Trapped /True") || memmem(&pdf, b"/Trapped/True"),
        "Info dict must contain `/Trapped /True`",
    );
    assert!(
        memmem(&pdf, b"pdf:Trapped=\"True\"") || memmem(&pdf, b"<pdf:Trapped>True</pdf:Trapped>"),
        "XMP packet must record `pdf:Trapped` as True",
    );
}

/// `Trapping::NotTrapped` is the explicit `/False` path.
#[test]
fn trapped_false_emits_both_streams() {
    let pdf = pdfx_doc_with_trapped(Some(Trapping::NotTrapped));
    assert!(
        memmem(&pdf, b"/Trapped /False") || memmem(&pdf, b"/Trapped/False"),
        "Info dict must contain `/Trapped /False`",
    );
    assert!(
        memmem(&pdf, b"pdf:Trapped=\"False\"")
            || memmem(&pdf, b"<pdf:Trapped>False</pdf:Trapped>"),
        "XMP packet must record `pdf:Trapped` as False",
    );
}

/// PDF/X forbids `/Trapped /Unknown`; krilla must downgrade an explicit
/// `Trapping::Unknown` to `NotTrapped` under a PDF/X validator.
#[test]
fn pdfx_downgrades_unknown_to_false() {
    let pdf = pdfx_doc_with_trapped(Some(Trapping::Unknown));
    assert!(
        memmem(&pdf, b"/Trapped /False") || memmem(&pdf, b"/Trapped/False"),
        "Info dict must downgrade Unknown to `/False` under PDF/X",
    );
    assert!(
        !memmem(&pdf, b"/Trapped /Unknown") && !memmem(&pdf, b"/Trapped/Unknown"),
        "Info dict must not emit `/Trapped /Unknown` under PDF/X",
    );
}

/// PDF/X with no caller-supplied trapping still produces `/Trapped
/// /False` because the validator requires the entry.
#[test]
fn pdfx_defaults_to_false_when_unset() {
    let pdf = pdfx_doc_with_trapped(None);
    assert!(
        memmem(&pdf, b"/Trapped /False") || memmem(&pdf, b"/Trapped/False"),
        "Info dict must default to `/Trapped /False` under PDF/X with no caller value",
    );
}
