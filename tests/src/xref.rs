//! Cross-reference layout selection (classic `xref` table vs
//! `/Type /XRef` stream).

use krilla::page::PageSettings;
use krilla::{Document, SerializeSettings};

use crate::settings_1;

fn build_one_page_pdf(settings: SerializeSettings) -> Vec<u8> {
    let mut doc = Document::new_with(settings);
    doc.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
    doc.finish().unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn xref_table_emitted_when_xref_streams_flag_unset() {
    let pdf = build_one_page_pdf(settings_1());

    // Classic layout: a literal `xref` keyword on its own line and a
    // `trailer` dictionary marker. Neither appears in the xref-stream
    // variant.
    assert!(
        contains(&pdf, b"\nxref\n"),
        "classic /xref table keyword missing from default layout"
    );
    assert!(
        contains(&pdf, b"trailer\n"),
        "classic trailer keyword missing from default layout"
    );
    assert!(
        !contains(&pdf, b"/Type /XRef"),
        "/Type /XRef stream emitted when xref_streams flag was false"
    );
}

#[test]
fn xref_stream_emitted_when_xref_streams_flag_set() {
    let mut settings = settings_1();
    settings.xref_streams = true;
    let pdf = build_one_page_pdf(settings);

    // Stream layout: the xref information lives in an indirect
    // `/Type /XRef` object — there is no `xref` / `trailer` keyword
    // pair.
    assert!(
        contains(&pdf, b"/Type /XRef"),
        "/Type /XRef stream missing when xref_streams flag was true"
    );
    assert!(
        !contains(&pdf, b"\nxref\n"),
        "classic /xref keyword still emitted when xref_streams flag was true"
    );
    assert!(
        !contains(&pdf, b"\ntrailer\n"),
        "classic trailer keyword still emitted when xref_streams flag was true"
    );
}
