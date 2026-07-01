use krilla::configure::ValidationError;
use krilla::embed::{AssociationKind, EmbedError, EmbeddedFile, MimeType};
use krilla::error::KrillaError;
use krilla::metadata::{DateTime, Metadata};
use krilla::tagging::TagTree;
use krilla_macros::snapshot;

use crate::{metadata_1, settings_10, validation_errors, Document};
use crate::{settings_13, settings_23, settings_27, ASSETS_PATH};

pub(crate) fn file_1() -> EmbeddedFile {
    let data = std::fs::read(ASSETS_PATH.join("emojis.txt")).unwrap();
    EmbeddedFile {
        path: "emojis.txt".to_string(),
        mime_type: Some(MimeType::new("text/txt").unwrap()),
        description: Some("The description of the file.".to_string()),
        association_kind: AssociationKind::Supplement,
        data: data.into(),
        modification_date: Some(DateTime::new(2001)),
        compress: Some(false),
        location: None,
    }
}

fn file_2() -> EmbeddedFile {
    let data = std::fs::read(ASSETS_PATH.join("svgs/resvg_structure_svg_nested_svg_with_rect.svg"))
        .unwrap();
    EmbeddedFile {
        path: "image.svg".to_string(),
        mime_type: Some(MimeType::new("image/svg+xml").unwrap()),
        description: Some("A nice SVG image!".to_string()),
        association_kind: AssociationKind::Supplement,
        modification_date: Some(DateTime::new(2001)),
        data: data.into(),
        compress: Some(false),
        location: None,
    }
}

fn file_3() -> EmbeddedFile {
    let data = std::fs::read(ASSETS_PATH.join("images/rgb8.png")).unwrap();

    EmbeddedFile {
        path: "rgb8.png".to_string(),
        mime_type: Some(MimeType::new("image/png").unwrap()),
        description: Some("A nice picture.".to_string()),
        association_kind: AssociationKind::Unspecified,
        data: data.into(),
        modification_date: Some(DateTime::new(2001)),
        compress: Some(false),
        location: None,
    }
}

fn file_4() -> EmbeddedFile {
    let data = std::fs::read(ASSETS_PATH.join("images/rgb8.gif")).unwrap();

    EmbeddedFile {
        path: "rgb8.gif".to_string(),
        mime_type: Some(MimeType::new("image/gif").unwrap()),
        description: Some("A nice gif.".to_string()),
        association_kind: AssociationKind::Unspecified,
        modification_date: Some(DateTime::new(2001)),
        data: data.into(),
        compress: Some(false),
        location: None,
    }
}

pub(crate) fn empty_file() -> EmbeddedFile {
    EmbeddedFile {
        path: "empty_file".to_string(),
        mime_type: Some(MimeType::new("text/plain").unwrap()),
        description: Some("A zero-byte file.".to_string()),
        association_kind: AssociationKind::Supplement,
        data: vec![].into(),
        modification_date: Some(DateTime::new(2001)),
        compress: None,
        location: None,
    }
}

#[snapshot(document)]
fn embedded_file(d: &mut Document) {
    let file = file_1();
    d.embed_file(file);
}

#[snapshot(document)]
fn embedded_file_with_compression(d: &mut Document) {
    let mut file = file_1();
    file.compress = Some(true);

    d.embed_file(file);
}

#[snapshot(document)]
fn embedded_file_with_auto_compression_success(d: &mut Document) {
    let mut file = file_1();
    file.compress = None;

    d.embed_file(file);
}

#[snapshot(document)]
fn embedded_file_with_auto_compression_fail(d: &mut Document) {
    let mut file = file_4();
    file.compress = None;

    d.embed_file(file);
}

#[snapshot(document)]
fn embedded_file_multiple(d: &mut Document) {
    let f1 = file_1();
    let f2 = file_2();
    let f3 = file_3();

    d.embed_file(f1);
    d.embed_file(f2);
    d.embed_file(f3);
}

#[snapshot(document)]
fn embedded_zero_byte_file(d: &mut Document) {
    let file = empty_file();
    d.embed_file(file);
}

pub(crate) fn embedded_file_impl(d: &mut Document) {
    let metadata = metadata_1();
    d.set_metadata(metadata);
    let f1 = file_1();
    d.embed_file(f1);
}

#[snapshot(document, settings_25)]
fn embedded_file_pdf_20(d: &mut Document) {
    // PDF 2.0 supports associated files, so we expect them to appear.
    embedded_file_impl(d)
}

#[test]
fn embedded_file_duplicate() {
    let mut d = Document::new();
    let f1 = file_1();
    let mut f2 = file_2();
    f2.path = f1.path.clone();

    assert!(d.embed_file(f1).is_some());
    assert!(d.embed_file(f2).is_none());
}

#[test]
fn embedded_file_pdf_a2() {
    let mut d = Document::new_with(settings_13());
    let metadata = metadata_1();
    d.set_metadata(metadata);
    d.set_tag_tree(TagTree::new());

    let mut f1 = file_1();
    f1.description = None;
    d.embed_file(f1);

    assert_eq!(
        validation_errors(d.finish()),
        vec![ValidationError::EmbeddedFile(EmbedError::Existence, None),]
    )
}

// See <https://github.com/typst/typst/issues/6758>
#[test]
fn embedded_file_before_metadata() {
    let mut d = Document::new_with(settings_10());
    d.set_tag_tree(TagTree::new());

    let f1 = file_1();
    d.embed_file(f1);

    let metadata = metadata_1();
    d.set_metadata(metadata);

    assert!(d.finish().is_ok())
}

#[test]
fn embedded_file_pdf_a3b_missing_date() {
    let mut d = Document::new_with(settings_10());
    d.set_tag_tree(TagTree::new());
    let metadata = metadata_1();
    d.set_metadata(metadata);

    let mut f1 = file_1();
    f1.modification_date = None;
    d.embed_file(f1);

    assert_eq!(
        validation_errors(d.finish()),
        vec![ValidationError::EmbeddedFile(EmbedError::MissingDate, None),]
    )
}

/// Build a PDF/A-3 document (PDF 1.7) embedding a single file with the
/// supplied `association_kind`, returning the serialised bytes.
fn embed_with_kind_pdf_a3(kind: AssociationKind) -> Vec<u8> {
    let mut d = Document::new_with(settings_10());
    d.set_tag_tree(TagTree::new());
    d.set_metadata(metadata_1());
    let mut f1 = file_1();
    f1.association_kind = kind;
    d.embed_file(f1);
    d.finish().expect("PDF/A-3 document should finish cleanly")
}

/// Build a PDF/A-4F document (PDF 2.0) embedding a single file with the
/// supplied `association_kind`, returning the serialised bytes.
fn embed_with_kind_pdf_a4f(kind: AssociationKind) -> Vec<u8> {
    let mut d = Document::new_with(settings_27());
    d.set_tag_tree(TagTree::new());
    d.set_metadata(metadata_1());
    let mut f1 = file_1();
    f1.association_kind = kind;
    d.embed_file(f1);
    d.finish().expect("PDF/A-4F document should finish cleanly")
}

/// Locate the `/AFRelationship /<Keyword>` entry written into the PDF
/// and return the matched keyword as a `String`, or `None` if absent.
fn af_relationship_keyword(pdf: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(pdf);
    let needle = "/AFRelationship /";
    let start = text.find(needle)?;
    let after = &text[start + needle.len()..];
    let end = after
        .find(|c: char| c.is_whitespace() || c == '>' || c == ']' || c == '/')
        .unwrap_or(after.len());
    Some(after[..end].to_string())
}

#[test]
fn association_kind_encrypted_payload_pdf_2_0_emits_keyword() {
    let pdf = embed_with_kind_pdf_a4f(AssociationKind::EncryptedPayload);
    assert_eq!(
        af_relationship_keyword(&pdf).as_deref(),
        Some("EncryptedPayload"),
        "PDF 2.0 target must serialise EncryptedPayload verbatim"
    );
}

#[test]
fn association_kind_form_data_pdf_2_0_emits_keyword() {
    let pdf = embed_with_kind_pdf_a4f(AssociationKind::FormData);
    assert_eq!(
        af_relationship_keyword(&pdf).as_deref(),
        Some("FormData"),
        "PDF 2.0 target must serialise FormData verbatim"
    );
}

#[test]
fn association_kind_schema_pdf_2_0_emits_keyword() {
    let pdf = embed_with_kind_pdf_a4f(AssociationKind::Schema);
    assert_eq!(
        af_relationship_keyword(&pdf).as_deref(),
        Some("Schema"),
        "PDF 2.0 target must serialise Schema verbatim"
    );
}

#[test]
fn association_kind_encrypted_payload_pdf_1_7_downgrades_to_unspecified() {
    let pdf = embed_with_kind_pdf_a3(AssociationKind::EncryptedPayload);
    assert_eq!(
        af_relationship_keyword(&pdf).as_deref(),
        Some("Unspecified"),
        "PDF 1.7 target must downgrade EncryptedPayload to Unspecified"
    );
}

#[test]
fn association_kind_form_data_pdf_1_7_downgrades_to_unspecified() {
    let pdf = embed_with_kind_pdf_a3(AssociationKind::FormData);
    assert_eq!(
        af_relationship_keyword(&pdf).as_deref(),
        Some("Unspecified"),
        "PDF 1.7 target must downgrade FormData to Unspecified"
    );
}

#[test]
fn association_kind_schema_pdf_1_7_downgrades_to_unspecified() {
    let pdf = embed_with_kind_pdf_a3(AssociationKind::Schema);
    assert_eq!(
        af_relationship_keyword(&pdf).as_deref(),
        Some("Unspecified"),
        "PDF 1.7 target must downgrade Schema to Unspecified"
    );
}

#[test]
fn association_kind_pre_2_0_keywords_round_trip_unchanged() {
    // Sanity: the four pre-2.0 keywords serialise as themselves under
    // both PDF 1.7 and PDF 2.0 targets.
    for kind in [
        AssociationKind::Source,
        AssociationKind::Data,
        AssociationKind::Alternative,
        AssociationKind::Supplement,
    ] {
        let pdf_17 = embed_with_kind_pdf_a3(kind);
        let pdf_20 = embed_with_kind_pdf_a4f(kind);
        let expected = format!("{kind:?}");
        assert_eq!(
            af_relationship_keyword(&pdf_17).as_deref(),
            Some(expected.as_str()),
            "{kind:?} should serialise verbatim under PDF 1.7"
        );
        assert_eq!(
            af_relationship_keyword(&pdf_20).as_deref(),
            Some(expected.as_str()),
            "{kind:?} should serialise verbatim under PDF 2.0"
        );
    }
}
