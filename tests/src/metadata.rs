use krilla::metadata::{DateTime, Metadata, PageLayout, TextDirection};
use krilla::Document;
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
    let settings = SerializeSettings {
        xmp_metadata: true,
        ..Default::default()
    };
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
        b"<?xpacket begin=\"\xef\xbb\xbf\" id=\"krilla-test-id\"?>krilla-raw-xmp-token<?xpacket end=\"w\"?>";
    // Force xmp_metadata off — raw_xmp must still emit the /Metadata
    // stream because it is an explicit opt-in.
    let settings = SerializeSettings {
        xmp_metadata: false,
        ..Default::default()
    };
    let mut document = Document::new_with(settings);
    document.set_metadata(metadata_impl_metadata().raw_xmp(payload.to_vec()));
    minimal_page(&mut document);
    let pdf = document.finish().expect("finish should succeed");

    assert!(
        memmem(&pdf, b"krilla-raw-xmp-token"),
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

/// [`Metadata::custom_property`] must write the author-supplied entry
/// into the `/Info` dictionary (ISO 32000-2 §14.3.3): the name as a
/// PDF name token and the value as a literal string.
#[test]
fn custom_property_written_to_info_dict() {
    let mut document = Document::new();
    document.set_metadata(Metadata::new().custom_property("CustomKey", "custom-value"));
    minimal_page(&mut document);
    let pdf = document.finish().expect("finish should succeed");

    assert!(
        memmem(&pdf, b"/CustomKey"),
        "custom Info-dict key should be written as a PDF name token",
    );
    assert!(
        memmem(&pdf, b"custom-value"),
        "custom Info-dict value should be written verbatim",
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
