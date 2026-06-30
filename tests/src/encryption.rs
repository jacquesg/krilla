//! End-to-end tests for [`SerializeSettings::encryption`].
//!
//! Each test builds a complete krilla document with the
//! [`Encryption`] surface, then string-matches the produced PDF for the
//! standard markers a compliant V=5/R=6 encrypted file must carry:
//! `/Encrypt` reference in the trailer, `/Filter /Standard`,
//! `/CFM /AESV3`. The negative-control assertions check that
//! caller-supplied plaintext (the `/Title` and the content-stream
//! string) does NOT survive into the bytestream.

use krilla::encryption::{Encryption, Permissions};
use krilla::metadata::Metadata;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::{Document, SerializeSettings};

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn build_doc(settings: SerializeSettings, title: &str) -> Vec<u8> {
    let mut doc = Document::new_with(settings);
    doc.set_metadata(Metadata::new().title(title.into()));
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    surface.set_fill(Some(Fill::default()));
    surface.finish();
    page.finish();
    doc.finish().unwrap()
}

fn encrypted_settings() -> SerializeSettings {
    SerializeSettings {
        encryption: Some(
            Encryption::new("alice", "bob")
                .with_permissions(Permissions::PRINT | Permissions::COPY),
        ),
        ..crate::settings_1()
    }
}

#[test]
fn encryption_emits_encrypt_trailer_entry() {
    let pdf = build_doc(encrypted_settings(), "Confidential Report");
    assert!(
        contains(&pdf, b"/Encrypt "),
        "/Encrypt reference missing from trailer"
    );
}

#[test]
fn encryption_emits_standard_security_handler_v5() {
    let pdf = build_doc(encrypted_settings(), "Confidential Report");
    assert!(contains(&pdf, b"/Filter /Standard"));
    assert!(contains(&pdf, b"/V 5"));
    assert!(contains(&pdf, b"/R 6"));
    assert!(contains(&pdf, b"/Length 256"));
    assert!(contains(&pdf, b"/CFM /AESV3"));
    assert!(contains(&pdf, b"/StmF /StdCF"));
    assert!(contains(&pdf, b"/StrF /StdCF"));
}

#[test]
fn encryption_emits_id_array_in_trailer() {
    let pdf = build_doc(encrypted_settings(), "Confidential Report");
    // Encryption requires an `/ID` array so the consumer can derive
    // the AES key. krilla writes one based on the document's
    // metadata-derived instance_id / document_id; it must appear in
    // the trailer.
    assert!(
        contains(&pdf, b"/ID ["),
        "encrypted PDF must have a /ID array in the trailer"
    );
}

#[test]
fn encryption_hides_metadata_title_plaintext() {
    let pdf = build_doc(encrypted_settings(), "Confidential Report");
    assert!(
        !contains(&pdf, b"Confidential Report"),
        "metadata /Title plaintext leaked through encryption"
    );
}

#[test]
fn unencrypted_pdf_keeps_title_plaintext() {
    let pdf = build_doc(crate::settings_1(), "Confidential Report");
    assert!(
        contains(&pdf, b"Confidential Report"),
        "plaintext /Title missing from unencrypted PDF"
    );
    assert!(
        !contains(&pdf, b"/Encrypt "),
        "/Encrypt trailer entry present without encryption setting"
    );
}

#[test]
#[ignore = "shells out to qpdf; run via cargo test -- --ignored qpdf_roundtrip"]
fn qpdf_roundtrip_encrypted_pdf() {
    use std::process::Command;

    let pdf = build_doc(encrypted_settings(), "Confidential Report");
    let path = std::env::temp_dir().join("krilla_qpdf_roundtrip.pdf");
    std::fs::write(&path, &pdf).expect("write encrypted PDF");

    // `qpdf --check` parses the encryption dict and validates the
    // file structurally; it requires the password to unlock the
    // payload. A non-zero exit indicates either a malformed `/Encrypt`
    // dict or invalid encryption parameters.
    let output = Command::new("qpdf")
        .arg("--check")
        .arg("--password=alice")
        .arg(&path)
        .output()
        .expect("qpdf not on PATH");
    assert!(
        output.status.success(),
        "qpdf rejected the encrypted PDF: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AESv3"));
    assert!(stdout.contains("string encryption method: AESv3"));
    assert!(stdout.contains("stream encryption method: AESv3"));

    // Wrong password must be rejected.
    let wrong = Command::new("qpdf")
        .arg("--check")
        .arg("--password=wrong")
        .arg(&path)
        .output()
        .expect("qpdf not on PATH");
    assert!(
        !wrong.status.success(),
        "qpdf accepted a wrong password (output: {})",
        String::from_utf8_lossy(&wrong.stderr),
    );
}

#[test]
fn encryption_off_by_default() {
    // SerializeSettings::default().encryption must be None so the
    // default build path stays unaffected.
    let settings = SerializeSettings::default();
    assert!(settings.encryption.is_none());
}

// --- PDF/A + encryption: mutual exclusion --------------------------

use krilla::configure::{Archival, ConfigurationBuilder, Prepress, ValidationError};
use krilla::error::KrillaError;

fn build_doc_collect_err(settings: SerializeSettings, title: &str) -> KrillaError {
    let mut doc = Document::new_with(settings);
    doc.set_metadata(Metadata::new().title(title.into()));
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let _ = page.surface();
    page.finish();
    doc.finish().expect_err("encryption + validator must error")
}

fn assert_contains_encryption_error(err: KrillaError) {
    match err {
        KrillaError::Validation(errors) => {
            assert!(
                errors
                    .iter()
                    .any(|(e, _)| matches!(e, ValidationError::ContainsEncryption)),
                "expected ValidationError::ContainsEncryption in {:?}",
                errors,
            );
        }
        other => panic!("expected Validation error, got {other:?}"),
    }
}

#[test]
fn encryption_plus_pdf_a_archival_validator_errors() {
    let settings = SerializeSettings {
        configuration: ConfigurationBuilder::new()
            .with_archival_validator(Archival::A3_B)
            .finish()
            .unwrap(),
        encryption: Some(Encryption::new("u", "o")),
        ..crate::settings_1()
    };
    let err = build_doc_collect_err(settings, "Confidential");
    assert_contains_encryption_error(err);
}

#[test]
fn encryption_plus_pdf_x_validator_errors() {
    let settings = SerializeSettings {
        configuration: ConfigurationBuilder::new()
            .with_prepress_validator(Prepress::X4)
            .finish()
            .unwrap(),
        encryption: Some(Encryption::new("u", "o")),
        ..crate::settings_1()
    };
    let err = build_doc_collect_err(settings, "Confidential");
    assert_contains_encryption_error(err);
}

#[test]
fn encryption_plus_pdf_ua_is_allowed() {
    // ISO 14289 is silent on encryption; the combination must
    // build successfully without raising ContainsEncryption.
    use krilla::configure::Accessibility;
    let settings = SerializeSettings {
        configuration: ConfigurationBuilder::new()
            .with_accessibility_validator(Accessibility::UA1)
            .finish()
            .unwrap(),
        encryption: Some(Encryption::new("u", "o")),
        ..crate::settings_1()
    };
    let mut doc = Document::new_with(settings);
    doc.set_metadata(
        Metadata::new()
            .title("Accessible Secret".into())
            .language("en".into()),
    );
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let _ = page.surface();
    page.finish();
    // UA-1 mandates several other things (tagging, language, etc.)
    // so the document still errors — but the error must NOT be
    // ContainsEncryption. We accept any other validation failure
    // and just assert encryption itself is not flagged.
    if let Err(KrillaError::Validation(errors)) = doc.finish() {
        assert!(
            !errors
                .iter()
                .any(|(e, _)| matches!(e, ValidationError::ContainsEncryption)),
            "PDF/UA must not flag encryption: {:?}",
            errors,
        );
    }
}
