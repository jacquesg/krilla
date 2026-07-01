//! End-to-end digital-signature tests exercising the real
//! emit -> patch path (ISO 32000-2 §12.8), not a hand-built buffer.
//!
//! Each test drives a full [`Document::finish`], which first serialises
//! the PDF with placeholder `/ByteRange [0 1000000000 1000000000
//! 1000000000]` and `/Contents (0000…)` entries, then runs the
//! post-finish patcher over the produced buffer: the three byte-range
//! placeholder slots are rewritten to the real `[0 a b c]` offsets and
//! the literal-string `/Contents` placeholder is swapped to a hex
//! string (`(` -> `<`, `)` -> `>`) holding the signer's bytes. The
//! signer here is an echo closure returning a fixed blob so the emitted
//! hex is predictable.

use krilla::annotation::{
    Annotation, SignatureField, SignatureLock, WidgetAnnotation, WidgetField,
};
use krilla::geom::Rect;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::signature::DigitalSignature;
use krilla::Document;

/// Fixed signature blob the echo signer returns; its lowercase hex
/// (`deadbeef`) must appear at the start of the patched `/Contents`
/// hex string, proving the signer ran and its bytes were encoded.
const SIG_BYTES: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A [`DigitalSignature`] whose signer echoes [`SIG_BYTES`], so the
/// test can assert the exact hex written into `/Contents`. The signer
/// receives the covered byte range and returns raw signature bytes;
/// krilla owns the hex encoding and byte-range arithmetic.
fn echo_signature() -> DigitalSignature {
    DigitalSignature::new(Box::new(|_to_sign: &[u8]| Ok(SIG_BYTES.to_vec())))
}

/// Assert the buffer carries a *patched* signature: the byte-range
/// array keeps its `[0 …` prefix but the `1000000000` placeholder is
/// gone, and `/Contents` is now a hex string (`<…>`) rather than the
/// literal `(…)` placeholder, holding the echoed signature bytes.
fn assert_patched(pdf: &[u8]) {
    assert!(
        contains(pdf, b"/ByteRange [0 "),
        "/ByteRange array missing from the signed PDF",
    );
    assert!(
        !contains(pdf, b"1000000000"),
        "/ByteRange still holds the 1000000000 placeholder — patcher did not run",
    );
    assert!(
        contains(pdf, b"/Contents <"),
        "/Contents was not rewritten to a hex string (delimiters not swapped)",
    );
    assert!(
        contains(pdf, b"deadbeef"),
        "echoed signature bytes missing from the /Contents hex string",
    );
}

#[test]
fn widget_signature_field_is_patched_end_to_end() {
    // WIDGET path: an explicit `/FT /Sig` widget annotation is attached
    // through the AcroForm path, so `finish` wires the widget's `/V` to
    // the `/Sig` dict rather than synthesising a standalone field.
    let mut doc = Document::new_with(crate::settings_1());
    doc.set_digital_signature(echo_signature());
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    surface.set_fill(Some(Fill::default()));
    surface.finish();
    let widget = WidgetAnnotation::new(
        Rect::from_xywh(10.0, 10.0, 52.0, 20.0).unwrap(),
        "Signature1",
        WidgetField::Signature(SignatureField {
            lock: SignatureLock::None,
        }),
    );
    page.add_annotation(Annotation::new_widget(widget, None));
    page.finish();

    let pdf = doc
        .finish()
        .expect("signing a document with a /Sig widget must succeed");
    assert_patched(&pdf);
}

#[test]
fn standalone_invisible_signature_is_patched_end_to_end() {
    // STANDALONE path: signing is enabled but no `SignatureField` widget
    // exists, so `finish` synthesises a document-level invisible
    // `/FT /Sig` field. The `/ByteRange` and `/Contents` placeholders
    // are emitted and patched identically to the widget path.
    let mut doc = Document::new_with(crate::settings_1());
    doc.set_digital_signature(echo_signature());
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    surface.set_fill(Some(Fill::default()));
    surface.finish();
    page.finish();

    let pdf = doc
        .finish()
        .expect("signing a document with no /Sig widget must still succeed");
    assert_patched(&pdf);
}
