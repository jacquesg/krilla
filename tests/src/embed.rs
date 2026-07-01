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
        embed_location: krilla::embed::EmbedLocation::Before,
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
        embed_location: krilla::embed::EmbedLocation::Before,
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
        embed_location: krilla::embed::EmbedLocation::Before,
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
        embed_location: krilla::embed::EmbedLocation::Before,
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
        embed_location: krilla::embed::EmbedLocation::Before,
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

// --- EmbedLocation partitioning of /AF -----------------------------

fn build_doc_with_locations(
    settings: krilla::SerializeSettings,
    files: Vec<(EmbeddedFile, krilla::embed::EmbedLocation)>,
) -> Vec<u8> {
    let mut doc = Document::new_with(settings);
    doc.set_metadata(metadata_1());
    for (file, loc) in files {
        doc.embed_file(file.with_embed_location(loc));
    }
    doc.start_page().finish();
    doc.finish().unwrap()
}

/// Extracts the byte slice between `/AF [` and the matching `]`.
fn extract_af_array(pdf: &[u8]) -> &[u8] {
    let start_marker = b"/AF [";
    let start = pdf
        .windows(start_marker.len())
        .position(|w| w == start_marker)
        .expect("/AF array missing");
    let body_start = start + start_marker.len();
    let end = body_start
        + pdf[body_start..]
            .iter()
            .position(|&b| b == b']')
            .expect("/AF array unterminated");
    &pdf[body_start..end]
}

/// Resolve each "N" of the form "N 0 R" inside the /AF array to
/// the name of the FileSpec dict it points at (`/F (name)` value).
/// Returns the names in emission order.
fn af_order(pdf: &[u8]) -> Vec<String> {
    let af = extract_af_array(pdf);
    let af_text = std::str::from_utf8(af).unwrap();
    // Each `/AF` entry is "N 0 R". Parse "N" tokens.
    let ref_numbers: Vec<u32> = af_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .chunks(3)
        .filter_map(|chunk| chunk.first().and_then(|s| s.parse::<u32>().ok()))
        .collect();

    ref_numbers
        .into_iter()
        .map(|n| {
            // Find the indirect object `N 0 obj` and pull the
            // `/F (name)` literal out of its FileSpec dict.
            let obj_header = format!("{n} 0 obj");
            let obj_start = pdf
                .windows(obj_header.len())
                .position(|w| w == obj_header.as_bytes())
                .unwrap_or_else(|| panic!("indirect object {n} 0 not found"));
            let f_marker = b"/F (";
            let f_idx = obj_start
                + pdf[obj_start..]
                    .windows(f_marker.len())
                    .position(|w| w == f_marker)
                    .unwrap_or_else(|| panic!("/F (name) missing on ref {n}"));
            let name_start = f_idx + f_marker.len();
            let name_end = name_start
                + pdf[name_start..]
                    .iter()
                    .position(|&b| b == b')')
                    .expect("/F string unterminated");
            String::from_utf8_lossy(&pdf[name_start..name_end]).into_owned()
        })
        .collect()
}

#[test]
fn embed_location_before_then_after_partitions_af() {
    // Insert in interleaved order — first an `After`, then a
    // `Before`, then an `After`. The /AF array must list the
    // Before entry first; within each partition entries stay
    // alphabetised (BTreeMap iteration order).
    let pdf = build_doc_with_locations(
        settings_23(),
        vec![
            (file_1(), krilla::embed::EmbedLocation::After), // emojis.txt
            (file_2(), krilla::embed::EmbedLocation::Before), // image.svg
            (file_3(), krilla::embed::EmbedLocation::After), // rgb8.png
        ],
    );

    let order = af_order(&pdf);
    assert_eq!(
        order,
        vec![
            "image.svg".to_string(),  // Before partition (alone)
            "emojis.txt".to_string(), // After partition, alphabetical
            "rgb8.png".to_string(),
        ],
        "/AF entries not partitioned by EmbedLocation"
    );
}

#[test]
fn embed_location_default_before_keeps_alphabetical_order() {
    // No `After` entries — default Before keeps all attachments in
    // alphabetical order (BTreeMap iteration).
    let pdf = build_doc_with_locations(
        settings_23(),
        vec![
            (file_3(), krilla::embed::EmbedLocation::Before), // rgb8.png
            (file_1(), krilla::embed::EmbedLocation::Before), // emojis.txt
            (file_2(), krilla::embed::EmbedLocation::Before), // image.svg
        ],
    );

    let order = af_order(&pdf);
    assert_eq!(
        order,
        vec![
            "emojis.txt".to_string(),
            "image.svg".to_string(),
            "rgb8.png".to_string(),
        ],
        "/AF entries not in alphabetical order in the single-partition case"
    );
}

#[test]
fn embed_location_default_is_before() {
    // EmbedLocation::default() must be Before, the location assumed
    // for an embedded file whose location is left unspecified.
    assert_eq!(
        krilla::embed::EmbedLocation::default(),
        krilla::embed::EmbedLocation::Before,
    );
}

#[test]
fn embed_location_field_round_trips_through_builder() {
    let f = file_1().with_embed_location(krilla::embed::EmbedLocation::After);
    assert_eq!(f.embed_location(), krilla::embed::EmbedLocation::After,);
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
