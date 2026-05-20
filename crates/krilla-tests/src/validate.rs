use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::{rgb, separation};
use krilla::configure::validate::VersionedFeature;
use krilla::configure::ValidationError;
use krilla::embed::EmbedError;
use krilla::error::KrillaError;
use krilla::geom::{Point, Rect, Size};
use krilla::metadata::{DateTime, Metadata};
use krilla::num::NormalizedF32;
use krilla::outline::Outline;
use krilla::page::Page;
use krilla::paint::{Fill, FillRule, LinearGradient, SpreadMethod};
use krilla::tagging::{Artifact, ArtifactType, ContentTag, SpanTag, TagGroup, TagKind, TagTree};
use krilla::tagging::{ListNumbering, TableHeaderScope, Tag};
use krilla::text::{Font, TextDirection};
use krilla::text::{GlyphId, KrillaGlyph};
use krilla_macros::snapshot;

use crate::embed::{embedded_file_impl, file_1};
use crate::{
    blue_fill, cmyk_fill, dummy_text_with_spans, green_fill, load_jpg_image, load_png_image, loc,
    metadata_1, metadata_2, rect_to_path, red_fill, settings_13, settings_15, settings_17,
    settings_19, settings_20, settings_23, settings_24, settings_32, settings_33, settings_34,
    settings_35, settings_7, settings_8, settings_9, stops_with_2_solid_1, validation_errors,
    youtube_link, NOTO_SANS,
};
use crate::{Document, SerializeSettings};

fn pdfa_document() -> Document {
    Document::new_with(settings_7())
}

fn q_nesting_impl(settings: SerializeSettings) -> Document {
    let mut document = Document::new_with(settings);
    let mut page = document.start_page();
    let mut surface = page.surface();

    for _ in 0..29 {
        surface.push_clip_path(&rect_to_path(0.0, 0.0, 100.0, 100.0), &FillRule::NonZero);
    }

    for _ in 0..29 {
        surface.pop();
    }

    surface.finish();
    page.finish();

    document
}

#[snapshot(document, settings_7)]
pub fn validate_pdf_a_q_nesting_28(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    for _ in 0..28 {
        surface.push_clip_path(&rect_to_path(0.0, 0.0, 100.0, 100.0), &FillRule::NonZero);
    }

    for _ in 0..28 {
        surface.pop();
    }
}

#[test]
pub fn validate_pdf_a_q_nesting_28() {
    let document = q_nesting_impl(settings_7());
    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::TooHighQNestingLevel]
    );
}

#[test]
pub fn validate_pdf_a_string_length() {
    let mut document = pdfa_document();
    let metadata = Metadata::new()
        .creator("A".repeat(32768))
        .creation_date(DateTime::new(2021));
    document.set_metadata(metadata);
    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::TooLongString]
    );
}

#[snapshot(settings_7)]
fn validate_pdf_a_annotation(page: &mut Page) {
    page.add_annotation(
        LinkAnnotation::new(
            Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
            Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
        )
        .into(),
    );
}

#[test]
fn validate_pdf_a_postscript() {
    let mut document = pdfa_document();
    let mut page = document.start_page();

    let gradient = LinearGradient {
        x1: 50.0,
        y1: 0.0,
        x2: 150.0,
        y2: 0.0,
        transform: Default::default(),
        spread_method: SpreadMethod::Repeat,
        stops: stops_with_2_solid_1(),
        anti_alias: false,
    };

    let fill = Fill {
        paint: gradient.into(),
        ..Default::default()
    };

    let mut surface = page.surface();

    surface.set_fill(Some(fill));
    surface.draw_path(&rect_to_path(0.0, 0.0, 100.0, 100.0));

    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::ContainsPostScript(None)]
    )
}

#[test]
pub fn validate_disabled_q_nesting_28() {
    let document = q_nesting_impl(SerializeSettings::default());
    assert!(document.finish().is_ok());
}

fn cmyk_document_impl(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
    let fill = cmyk_fill(1.0);
    surface.set_fill(Some(fill));
    surface.draw_path(&path);

    surface.finish();
    page.finish();
}

#[test]
fn validate_pdf_a_missing_cmyk() {
    let mut document = pdfa_document();
    cmyk_document_impl(&mut document);

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::MissingCMYKProfile]
    )
}

#[test]
fn validate_pdf_a_existing_cmyk() {
    let mut document = Document::new_with(settings_8());
    cmyk_document_impl(&mut document);

    assert!(document.finish().is_ok())
}

#[test]
fn validate_pdf_a_notdef_glyph() {
    let mut document = pdfa_document();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font.clone(),
        20.0,
        "你",
        false,
        TextDirection::Auto,
    );
    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::ContainsNotDefGlyph(
            font,
            None,
            "你".to_string()
        )]
    )
}

#[test]
fn validate_pdfa2u_text_with_location() {
    let mut document = Document::new_with(settings_9());
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();
    let (text, glyphs) = dummy_text_with_spans();

    surface.set_location(loc(2));
    surface.set_fill(Some(red_fill(0.1)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 10.0, 10.0));

    surface.draw_glyphs(
        Point::from_xy(0.0, 100.0),
        &glyphs,
        font.clone(),
        &text,
        20.0,
        false,
    );
    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::ContainsNotDefGlyph(
            font,
            Some(loc(4)),
            "i".to_string()
        )]
    )
}

#[test]
fn validate_pdfa1b_transparency_with_location() {
    let mut document = Document::new_with(settings_19());
    let mut page = document.start_page();
    let mut surface = page.surface();

    surface.set_location(loc(2));
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 10.0, 10.0));
    surface.set_location(loc(3));
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 10.0, 10.0));
    surface.set_location(loc(4));
    surface.set_fill(Some(green_fill(0.9)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 10.0, 10.0));
    surface.set_location(loc(5));
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 10.0, 10.0));
    surface.set_location(loc(6));
    surface.set_fill(Some(blue_fill(0.8)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 10.0, 10.0));
    surface.set_location(loc(7));
    surface.set_fill(Some(blue_fill(0.9)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 10.0, 10.0));

    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![
            ValidationError::Transparency(Some(loc(4))),
            ValidationError::Transparency(Some(loc(6))),
            // Note that we don't have 7 here, even though we should in theory. The reason is
            // that since we cache graphics states, only the first time we serialize it will
            // it trigger the validation error. Not optimal, but changing that would be a pain.
        ]
    )
}

fn validate_pdf_full_example(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "This is some text",
        false,
        TextDirection::Auto,
    );

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(30.0, 30.0, 70.0, 70.0));

    surface.finish();
    page.finish();

    let metadata = metadata_1();
    document.set_metadata(metadata);
}

pub(crate) fn validate_pdf_tagged_full_example(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag {
        lang: None,
        alt_text: Some("Alt"),
        expanded: Some("Expanded"),
        actual_text: Some("ActualText"),
    }));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "This is some text",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    let id2 = surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(
        ArtifactType::Header,
    )));
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(30.0, 30.0, 70.0, 70.0));
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(id1);
    tag_tree.push(id2);
    document.set_tag_tree(tag_tree);

    let metadata = metadata_1();
    document.set_metadata(metadata);
}

fn invalid_codepoint_impl(document: &mut Document, font: Font, text: &str) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let glyphs = vec![
        KrillaGlyph::new(GlyphId::new(3), 2048.0, 0.0, 0.0, 0.0, 0..1, None),
        KrillaGlyph::new(GlyphId::new(2), 2048.0, 0.0, 0.0, 0.0, 1..4, None),
    ];

    surface.draw_glyphs(
        Point::from_xy(0.0, 100.0),
        &glyphs,
        font.clone(),
        text,
        20.0,
        false,
    );
    surface.finish();
    page.finish();
}

#[test]
fn validate_pdfu_invalid_codepoint() {
    let mut document = Document::new_with(settings_9());
    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();
    invalid_codepoint_impl(&mut document, font.clone(), "A\u{FEFF}B");

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::InvalidCodepointMapping(
            font,
            GlyphId::new(2),
            '\u{FEFF}',
            None
        )]
    )
}

#[test]
fn validate_pdfa_no_codepoint() {
    let mut document = Document::new_with(settings_20());
    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let glyphs = [KrillaGlyph::new(
        GlyphId::new(3),
        2048.0,
        0.0,
        0.0,
        0.0,
        0..0,
        None,
    )];

    surface.draw_glyphs(
        Point::from_xy(0.0, 100.0),
        &glyphs,
        font.clone(),
        "",
        20.0,
        false,
    );
    surface.finish();
    page.finish();

    assert!(
        validation_errors(document.finish()).contains(&ValidationError::NoCodepointMapping(
            font,
            GlyphId::new(1),
            None
        ))
    );
}

#[test]
fn validate_pdfa_private_unicode_codepoint() {
    let mut document = Document::new_with(settings_13());
    let metadata = metadata_1();
    document.set_metadata(metadata);
    document.set_tag_tree(TagTree::new());
    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();
    invalid_codepoint_impl(&mut document, font.clone(), "A\u{E022}B");

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::UnicodePrivateArea(
            font,
            GlyphId::new(2),
            '\u{E022}',
            None
        )]
    )
}

#[snapshot(document, settings_20)]
fn validate_pdf_a1_a_full_example(document: &mut Document) {
    validate_pdf_tagged_full_example(document);
}

#[snapshot(document, settings_19)]
fn validate_pdf_a1_b_full_example(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[snapshot(document, settings_13)]
fn validate_pdf_a2_a_full_example(document: &mut Document) {
    validate_pdf_tagged_full_example(document);
}

#[snapshot(document, settings_7)]
fn validate_pdf_a2_b_full_example(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[snapshot(document, settings_9)]
fn validate_pdf_a2_u_full_example(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[snapshot(document, settings_14)]
fn validate_pdf_a3_a_full_example(document: &mut Document) {
    validate_pdf_tagged_full_example(document);
}

#[snapshot(document, settings_10)]
fn validate_pdf_a3_b_full_example(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[snapshot(document, settings_11)]
fn validate_pdf_a3_u_full_example(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[snapshot(document, settings_26)]
fn validate_pdf_a4_full_example(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[snapshot(document, settings_27)]
fn validate_pdf_a4f_full_example(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[snapshot(document, settings_28)]
fn validate_pdf_a4e_full_example(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[test]
fn validate_pdf_ua1_empty_annotation_alt() {
    let mut document = Document::new_with(settings_15());
    let mut page = document.start_page();

    let annot_loc = loc(1);
    let annot = page.add_tagged_annotation(
        Annotation::new_link(
            LinkAnnotation::new(
                Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
                Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
            ),
            Some(String::new()),
        )
        .with_location(Some(annot_loc)),
    );

    page.finish();

    let div_loc = loc(2);
    let mut tag_group = TagGroup::new(Tag::Div.with_location(Some(div_loc)));
    tag_group.push(annot);

    let mut tag_tree = TagTree::new();
    tag_tree.push(tag_group);
    document.set_tag_tree(tag_tree);

    assert!(validation_errors(document.finish())
        .contains(&ValidationError::MissingAnnotationAltText(Some(annot_loc))));
}

#[test]
fn validate_pdf_ua1_empty_alt() {
    let mut document = Document::new_with(settings_15());
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "Hi",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();

    page.finish();

    let formula_loc = loc(1);
    let mut tag_group =
        TagGroup::new(Tag::Formula(Some(String::new())).with_location(Some(formula_loc)));
    tag_group.push(id1);

    let mut tag_tree = TagTree::new();
    tag_tree.push(tag_group);
    document.set_tag_tree(tag_tree);

    assert!(validation_errors(document.finish())
        .contains(&ValidationError::MissingAltText(Some(formula_loc))));
}

#[snapshot(document, settings_15)]
fn validate_pdf_ua1_full_example(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "This is some text",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();

    let annotation = page.add_tagged_annotation(Annotation::new_link(
        LinkAnnotation::new(
            Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
            Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
        ),
        Some("A link to youtube".to_string()),
    ));

    let mut link_group = TagGroup::new(Tag::Link);
    link_group.push(annotation);

    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(id1);
    tag_tree.push(link_group);
    document.set_tag_tree(tag_tree);

    let metadata = Metadata::new()
        .language("en".to_string())
        .title("a nice title".to_string());
    document.set_metadata(metadata);

    let outline = Outline::new();
    document.set_outline(outline);
}

#[test]
fn validate_pdf_ua1_missing_requirements() {
    let mut document = Document::new_with(settings_15());
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "Hi",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();

    let annot_loc = loc(1);
    let annot = page.add_tagged_annotation(
        Annotation::new_link(
            LinkAnnotation::new(
                Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
                Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
            ),
            None,
        )
        .with_location(Some(annot_loc)),
    );

    page.finish();

    let formula_loc = loc(2);
    let mut tag_group = TagGroup::new(Tag::Formula(None).with_location(Some(formula_loc)));
    tag_group.push(id1);
    tag_group.push(annot);

    let mut tag_tree = TagTree::new();
    tag_tree.push(tag_group);
    document.set_tag_tree(tag_tree);

    assert_eq!(
        validation_errors(document.finish()),
        vec![
            ValidationError::MissingDocumentOutline,
            ValidationError::MissingAnnotationAltText(Some(annot_loc)),
            ValidationError::MissingAltText(Some(formula_loc)),
            ValidationError::NoDocumentTitle
        ]
    )
}

#[snapshot(document, settings_15)]
fn validate_pdf_ua1_attributes(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 100.0, 100.0));
    surface.end_tagged();

    let id2 = surface.start_tagged(ContentTag::Other);
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 100.0, 100.0));
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut tag_tree = TagTree::new();

    let mut group1 = TagGroup::new(Tag::L(ListNumbering::Circle));
    group1.push(id1);

    let mut group2 = TagGroup::new(Tag::TH(TableHeaderScope::Row));
    let mut group3 = TagGroup::new(Tag::TR);
    let mut group4 = TagGroup::new(Tag::Table);
    group2.push(id2);
    group3.push(group2);
    group4.push(group3);

    tag_tree.push(group1);
    tag_tree.push(group4);
    document.set_tag_tree(tag_tree);

    let metadata = Metadata::new()
        .language("en".to_string())
        .title("a nice title".to_string());
    document.set_metadata(metadata);

    let outline = Outline::new();
    document.set_outline(outline);
}

#[snapshot(document, settings_16)]
fn pdf_version_14_tagged(document: &mut Document) {
    validate_pdf_tagged_full_example(document);
}

#[test]
fn validate_pdf_a1_no_transparency() {
    let mut document = Document::new_with(settings_19());
    let metadata = metadata_1();
    document.set_metadata(metadata);
    let mut page = document.start_page();
    let mut surface = page.surface();
    surface.set_fill(Some(red_fill(0.5)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 100.0, 100.0));
    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::Transparency(None)]
    )
}

#[test]
fn validate_pdf_a1_no_image_transparency() {
    let mut document = Document::new_with(settings_19());
    let metadata = metadata_1();
    document.set_metadata(metadata);

    let image = load_png_image("rgba8.png");
    let size = Size::from_wh(image.size().0 as f32, image.size().1 as f32).unwrap();

    let mut page = document.start_page();
    let mut surface = page.surface();
    surface.draw_image(image, size);
    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::Transparency(None)]
    )
}

#[snapshot(document, settings_22)]
fn validate_other_version(document: &mut Document) {
    validate_pdf_full_example(document);
}

#[test]
fn validate_pdf_a1_limits() {
    let mut document = Document::new_with(settings_19());
    let mut page = document.start_page();

    // An array can only have 8191 elements, so it must not be possible to have that many.
    for _ in 0..8193 {
        page.add_annotation(youtube_link(100.0, 100.0, 100.0, 100.0));
    }

    page.add_annotation(youtube_link(66000.1, 66000.1, 100.0, 100.0));
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![
            ValidationError::TooLargeFloat,
            ValidationError::TooLongArray,
        ]
    )
}

#[test]
fn validate_pdf_a3_a_no_tag_tree() {
    let mut document = Document::new_with(settings_24());
    document.set_metadata(metadata_1());

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::MissingTagging]
    )
}

#[test]
fn validate_pdf_a3_missing_fields() {
    let mut d = Document::new_with(settings_23());
    let mut f1 = file_1();
    f1.description = None;
    f1.modification_date = None;
    d.embed_file(f1);

    assert_eq!(
        validation_errors(d.finish()),
        vec![
            ValidationError::EmbeddedFile(EmbedError::MissingDate, None),
            ValidationError::EmbeddedFile(EmbedError::MissingDescription, None)
        ]
    )
}

#[snapshot(document, settings_23)]
fn validate_pdf_a3_with_embedded_file(d: &mut Document) {
    embedded_file_impl(d)
}

#[snapshot(document, settings_27)]
fn validate_pdf_a4_f_with_embedded_file(d: &mut Document) {
    embedded_file_impl(d)
}

// See https://github.com/LaurenzV/krilla/issues/162
// Can't include this test because it would requires us to embed the font in the snapshot.
#[cfg(target_os = "macos")]
#[ignore]
fn validate_pdf_a1_b_ttc(d: &mut Document) {
    let font_data: crate::Data = std::fs::read("/System/Library/Fonts/Supplemental/Songti.ttc")
        .unwrap()
        .into();
    let font = Font::new(font_data.clone(), 3).unwrap();

    let mut page = d.start_page();
    let mut surface = page.surface();

    surface.draw_text(
        Point::from_xy(0.0, 75.0),
        font.clone(),
        20.0,
        "文",
        false,
        TextDirection::Auto,
    );
}

#[test]
fn validate_pdf_a1_b_cmyk_image_without_icc_profile() {
    let mut document = Document::new_with(settings_19());
    let mut page = document.start_page();
    let mut surface = page.surface();
    let image = load_jpg_image("cmyk.jpg");
    let size = image.size();
    surface.draw_image(
        image.clone(),
        Size::from_wh(size.0 as f32, size.1 as f32).unwrap(),
    );

    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::MissingCMYKProfile]
    );
}

#[snapshot(document, settings_15)]
fn validate_pdf_ua1_only_annotation(document: &mut Document) {
    let mut page = document.start_page();

    let annotation = page.add_tagged_annotation(Annotation::new_link(
        LinkAnnotation::new(
            Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
            Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
        ),
        Some("A link to youtube".to_string()),
    ));

    let mut link_group = TagGroup::new(Tag::Link);
    link_group.push(annotation);

    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(link_group);
    document.set_tag_tree(tag_tree);

    let metadata = Metadata::new()
        .language("en".to_string())
        .title("a nice title".to_string());
    document.set_metadata(metadata);

    let outline = Outline::new();
    document.set_outline(outline);
}

// ---- PDF/UA-2 tests ----

#[snapshot(document, settings_34)]
fn validate_pdf_ua2_full_example(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "This is some text",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();

    let annotation = page.add_tagged_annotation(Annotation::new_link(
        LinkAnnotation::new(
            Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
            Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
        ),
        Some("A link to youtube".to_string()),
    ));

    let mut link_group = TagGroup::new(Tag::Link);
    link_group.push(annotation);

    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(id1);
    tag_tree.push(link_group);
    document.set_tag_tree(tag_tree);

    let metadata = Metadata::new()
        .language("en".to_string())
        .title("a nice title".to_string());
    document.set_metadata(metadata);

    let outline = Outline::new();
    document.set_outline(outline);
}

#[test]
fn validate_pdf_ua2_missing_requirements() {
    let mut document = Document::new_with(settings_34());
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "Hi",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();

    let annot_loc = loc(1);
    let annot = page.add_tagged_annotation(
        Annotation::new_link(
            LinkAnnotation::new(
                Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
                Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
            ),
            None,
        )
        .with_location(Some(annot_loc)),
    );

    page.finish();

    let formula_loc = loc(2);
    let mut tag_group = TagGroup::new(Tag::Formula(None).with_location(Some(formula_loc)));
    tag_group.push(id1);
    tag_group.push(annot);

    let mut tag_tree = TagTree::new();
    tag_tree.push(tag_group);
    document.set_tag_tree(tag_tree);

    let errs = validation_errors(document.finish());
    assert!(errs.contains(&ValidationError::MissingDocumentOutline));
    assert!(errs.contains(&ValidationError::MissingAnnotationAltText(Some(annot_loc))));
    assert!(errs.contains(&ValidationError::MissingAltText(Some(formula_loc))));
    assert!(errs.contains(&ValidationError::NoDocumentTitle));
    assert!(errs.contains(&ValidationError::NoDocumentLanguage));
}

#[test]
fn validate_pdf_ua2_missing_tagging() {
    let mut document = Document::new_with(settings_34());
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .title("a nice title".to_string()),
    );
    document.set_outline(Outline::new());

    let errs = validation_errors(document.finish());
    assert!(errs.contains(&ValidationError::MissingTagging));
}

// ---- WTPDF 1.0 tests ----

#[snapshot(document, settings_35)]
fn validate_wtpdf_full_example(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "This is some text",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(id1);
    document.set_tag_tree(tag_tree);
}

#[test]
fn validate_wtpdf_missing_tagging() {
    let mut document = Document::new_with(settings_35());
    let errs = validation_errors(document.finish());
    assert!(errs.contains(&ValidationError::MissingTagging));
}

#[test]
fn validate_wtpdf_allows_minimal_metadata() {
    // WTPDF, unlike UA-2, does not mandate a document title, language,
    // alt text on figures, an outline, or DisplayDocTitle. A minimally
    // tagged document with no metadata at all must therefore succeed.
    let mut document = Document::new_with(settings_35());
    let mut page = document.start_page();
    let mut surface = page.surface();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(id1);
    document.set_tag_tree(tag_tree);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_wtpdf_requires_codepoint_mappings() {
    let mut document = Document::new_with(settings_35());
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font.clone(),
        20.0,
        "你",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(id1);
    document.set_tag_tree(tag_tree);

    let errs = validation_errors(document.finish());
    assert!(errs
        .iter()
        .any(|e| matches!(e, ValidationError::ContainsNotDefGlyph(_, _, _))));
}

#[test]
fn validate_deduplicate_errors() {
    let mut document = Document::new_with(settings_19());
    let mut page = document.start_page();
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(0.5)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 20.0, 20.0));
    surface.set_location(loc(2));
    surface.set_fill(Some(red_fill(0.4)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 20.0, 20.0));
    surface.reset_location();
    surface.set_fill(Some(red_fill(0.3)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 20.0, 20.0));
    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![
            ValidationError::Transparency(None),
            ValidationError::Transparency(Some(loc(2)))
        ]
    );
}

#[test]
fn validate_inconsistent_separation_fallback() {
    let mut document = Document::new_with(settings_7());
    let mut page = document.start_page();
    let mut surface = page.surface();

    // First usage of "PANTONE 185 C" with red fallback
    let space1 = separation::SeparationSpace::new(
        separation::SeparationColorant::Custom("PANTONE 185 C".to_string()),
        rgb::Color::new(255, 0, 0).into(),
    );
    let color1: krilla::color::Color = separation::Color::new(255, space1).into();
    let fill1 = Fill {
        paint: color1.into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    };
    surface.set_fill(Some(fill1));
    surface.draw_path(&rect_to_path(0.0, 0.0, 20.0, 20.0));

    // Second usage of "PANTONE 185 C" with DIFFERENT blue fallback
    // This should trigger a validation error
    let space2 = separation::SeparationSpace::new(
        separation::SeparationColorant::Custom("PANTONE 185 C".to_string()),
        rgb::Color::new(0, 0, 255).into(),
    );
    let color2: krilla::color::Color = separation::Color::new(255, space2).into();
    let fill2 = Fill {
        paint: color2.into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    };
    surface.set_fill(Some(fill2));
    surface.draw_path(&rect_to_path(30.0, 0.0, 50.0, 20.0));

    surface.finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::InconsistentSeparationFallback(
            separation::SeparationColorant::Custom("PANTONE 185 C".to_string())
        )]
    );
}

fn validate_pdf14_ua1_header_footer_artifact_subtypes() {
    let mut document = Document::new_with(settings_33());
    let mut page = document.start_page();
    let mut surface = page.surface();

    let id = surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(
        ArtifactType::Header,
    )));
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(30.0, 30.0, 70.0, 70.0));
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(id);
    document.set_tag_tree(tag_tree);

    document.set_metadata(metadata_2());

    let outline = Outline::new();
    document.set_outline(outline);

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::RequiresNewerPdfVersion(
            VersionedFeature::HeaderFooterArtifactSubtypes,
            None
        )]
    );
}

#[snapshot(document)]
fn no_validators_embedded_file_no_af(d: &mut Document) {
    // Embedded files are written but no AF (associated files) entry is
    // produced, because an empty validator set means `allows_associated_files`
    // is vacuously false.
    embedded_file_impl(d);
}

// A-3b + UA-1: Even though neither PDF 1.7 nor PDF/UA-1 specify associated
// files, A-3b adds them, so the AF entry should be written.
#[snapshot(document, settings_32)]
fn validate_multi_validator_embedded_file_af(d: &mut Document) {
    let metadata = Metadata::new()
        .language("en".to_string())
        .title("a nice title".to_string())
        .creation_date(DateTime::new(2001));
    d.set_metadata(metadata);
    d.set_tag_tree(TagTree::new());
    d.set_outline(Outline::new());

    d.embed_file(file_1());
}

// A-3b + UA-1: UA-1 requires an outline; A-3b does not.
#[test]
fn validate_multi_validator_ua1_prohibits_missing_outline() {
    let mut document = Document::new_with(settings_32());
    let metadata = Metadata::new()
        .language("en".to_string())
        .title("title".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);
    document.set_tag_tree(TagTree::new());

    let mut page = document.start_page();
    page.surface().finish();
    page.finish();

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::MissingDocumentOutline]
    );
}

#[snapshot(document, settings_32)]
fn validate_multi_validator_pdf_a3b_pdf_ua1_full_example(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "Hello, PDF/A-3b + PDF/UA-1",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(id1);
    document.set_tag_tree(tag_tree);

    let metadata = Metadata::new()
        .language("en".to_string())
        .title("a nice title".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    document.set_outline(Outline::new());
}

#[test]
fn validate_pdf14_ua1_structure_order_tabbing() {
    let mut document = Document::new_with(settings_33());
    let mut page = document.start_page();

    let annot = page.add_tagged_annotation(Annotation::new_link(
        LinkAnnotation::new(
            Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
            Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
        ),
        Some("Link to YouTube".to_string()),
    ));

    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(annot);
    document.set_tag_tree(tag_tree);

    document.set_metadata(metadata_2());

    let outline = Outline::new();
    document.set_outline(outline);

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::RequiresNewerPdfVersion(
            VersionedFeature::StructureOrderTabbing,
            None
        )]
    );
}

#[test]
fn validate_pdf14_ua1_table_header_scope() {
    let mut document = Document::new_with(settings_33());
    let mut page = document.start_page();
    let mut surface = page.surface();

    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();

    let text_id = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "header",
        false,
        TextDirection::Auto,
    );
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut row = TagGroup::new(Tag::TR);
    let mut th = TagGroup::new(Tag::TH(TableHeaderScope::Row));
    th.push(text_id);
    row.push(th);

    let mut table = TagGroup::new(Tag::Table);
    table.push(row);

    let mut tag_tree = TagTree::new();
    tag_tree.push(table);
    document.set_tag_tree(tag_tree);

    document.set_metadata(metadata_2());

    let outline = Outline::new();
    document.set_outline(outline);

    assert_eq!(
        validation_errors(document.finish()),
        vec![ValidationError::RequiresNewerPdfVersion(
            VersionedFeature::TableHeaderScope,
            None
        )]
    );
}

#[test]
fn validate_pdf14_tagged_annotation_no_ua() {
    // Ensure tagging + annotation + PDF 1.4 without UA validator doesn't
    // fail (see https://github.com/LaurenzV/krilla/pull/278#discussion_r3213542007).
    let mut document = Document::new_with(settings_17());
    let mut page = document.start_page();

    let annot = page.add_tagged_annotation(Annotation::new_link(
        LinkAnnotation::new(
            Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
            Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
        ),
        None,
    ));

    page.finish();

    let mut tag_tree = TagTree::new();
    tag_tree.push(annot);
    document.set_tag_tree(tag_tree);

    assert!(document.finish().is_ok());
}

#[test]
fn custom_output_intent_rejects_invalid_input() {
    use krilla::icc::ICCProfile;
    use krilla::{
        CustomOutputIntent, CustomOutputIntentError, CustomOutputIntentSubtype,
        OutputIntentProfile,
    };

    let profile_bytes =
        std::fs::read(crate::WORKSPACE_PATH.join("crates/krilla/icc/sRGB-v4.icc")).unwrap();
    let profile = ICCProfile::<3>::new(&profile_bytes).unwrap();

    assert_eq!(
        CustomOutputIntent::new(
            CustomOutputIntentSubtype::PdfA,
            OutputIntentProfile::Rgb(profile.clone()),
            "   ".to_string(),
            "info".to_string(),
        )
        .err(),
        Some(CustomOutputIntentError::EmptyIdentifier)
    );

    assert_eq!(
        CustomOutputIntent::new(
            CustomOutputIntentSubtype::PdfA,
            OutputIntentProfile::Rgb(profile.clone()),
            "Custom".to_string(),
            "  ".to_string(),
        )
        .err(),
        Some(CustomOutputIntentError::EmptyInfo)
    );

    assert_eq!(
        CustomOutputIntent::new(
            CustomOutputIntentSubtype::Custom(String::new()),
            OutputIntentProfile::Rgb(profile),
            "Custom".to_string(),
            "info".to_string(),
        )
        .err(),
        Some(CustomOutputIntentError::EmptyCustomSubtype)
    );
}

#[test]
fn custom_output_intent_emits_catalogue_entry() {
    use krilla::icc::ICCProfile;
    use krilla::{
        CustomOutputIntent, CustomOutputIntentSubtype, Document, OutputIntentProfile,
        SerializeSettings,
    };

    let profile_bytes =
        std::fs::read(crate::WORKSPACE_PATH.join("crates/krilla/icc/sRGB-v4.icc")).unwrap();
    let profile = ICCProfile::<3>::new(&profile_bytes).unwrap();
    let intent = CustomOutputIntent::new(
        CustomOutputIntentSubtype::PdfA,
        OutputIntentProfile::Rgb(profile),
        "sRGB IEC61966-2.1".to_string(),
        "sRGB v4 destination profile".to_string(),
    )
    .expect("intent fields are non-empty")
    .with_output_condition("sRGB".to_string())
    .with_registry_name("http://www.color.org".to_string());

    let settings = SerializeSettings {
        output_intents: vec![intent],
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );
    let mut page = document.start_page();
    page.surface().finish();
    page.finish();
    let bytes = document.finish().expect("document finishes without errors");

    let pdf = String::from_utf8_lossy(&bytes);
    assert!(
        pdf.contains("/OutputIntents"),
        "PDF should contain /OutputIntents catalogue entry"
    );
    assert!(
        pdf.contains("/Type /OutputIntent"),
        "PDF should contain /Type /OutputIntent dictionary entry"
    );
    assert!(
        pdf.contains("/S /GTS_PDFA1"),
        "PDF should contain /S /GTS_PDFA1 for PdfA subtype"
    );
    assert!(
        pdf.contains("(sRGB IEC61966-2.1)"),
        "PDF should contain the output condition identifier"
    );
    assert!(
        pdf.contains("(sRGB v4 destination profile)"),
        "PDF should contain the info string"
    );
    assert!(
        pdf.contains("(sRGB)"),
        "PDF should contain the optional output condition"
    );
    assert!(
        pdf.contains("(http://www.color.org)"),
        "PDF should contain the optional registry name"
    );
}

#[test]
fn custom_output_intent_custom_subtype_emits_verbatim_name() {
    use krilla::icc::ICCProfile;
    use krilla::{
        CustomOutputIntent, CustomOutputIntentSubtype, Document, OutputIntentProfile,
        SerializeSettings,
    };

    let profile_bytes =
        std::fs::read(crate::WORKSPACE_PATH.join("crates/krilla/icc/sRGB-v4.icc")).unwrap();
    let profile = ICCProfile::<3>::new(&profile_bytes).unwrap();
    let intent = CustomOutputIntent::new(
        CustomOutputIntentSubtype::Custom("ISO_PDFE1".to_string()),
        OutputIntentProfile::Rgb(profile),
        "Custom".to_string(),
        "PDF/E custom intent".to_string(),
    )
    .expect("intent fields are non-empty");

    let settings = SerializeSettings {
        output_intents: vec![intent],
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );
    let mut page = document.start_page();
    page.surface().finish();
    page.finish();
    let bytes = document.finish().expect("document finishes without errors");

    let pdf = String::from_utf8_lossy(&bytes);
    assert!(
        pdf.contains("/S /ISO_PDFE1"),
        "PDF should contain /S /ISO_PDFE1 verbatim for Custom subtype"
    );
}

#[test]
fn custom_output_intent_no_validator_emits_only_custom() {
    use krilla::icc::ICCProfile;
    use krilla::{
        CustomOutputIntent, CustomOutputIntentSubtype, Document, OutputIntentProfile,
        SerializeSettings,
    };

    let profile_bytes =
        std::fs::read(crate::WORKSPACE_PATH.join("crates/krilla/icc/sRGB-v4.icc")).unwrap();
    let profile = ICCProfile::<3>::new(&profile_bytes).unwrap();
    let intent = CustomOutputIntent::new(
        CustomOutputIntentSubtype::PdfE,
        OutputIntentProfile::Rgb(profile),
        "Custom".to_string(),
        "PDF/E intent".to_string(),
    )
    .expect("intent fields are non-empty");

    let settings = SerializeSettings {
        output_intents: vec![intent],
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );
    let mut page = document.start_page();
    page.surface().finish();
    page.finish();
    let bytes = document.finish().expect("document finishes without errors");
    let pdf = String::from_utf8_lossy(&bytes);

    assert!(pdf.contains("/S /ISO_PDFE1"));
    // No validator → no PDFA intent should be auto-generated.
    assert!(!pdf.contains("/S /GTS_PDFA1"));
}

#[test]
fn fallback_cmyk_profile_emits_default_output_intent() {
    use krilla::icc::ICCProfile;
    use krilla::{Document, SerializeSettings};

    let profile_bytes = std::fs::read(crate::ASSETS_PATH.join("icc/eciCMYK_v2.icc")).unwrap();
    let profile = ICCProfile::<4>::new(&profile_bytes).unwrap();

    let settings = SerializeSettings {
        fallback_cmyk_profile: Some(profile),
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );
    let mut page = document.start_page();
    page.surface().finish();
    page.finish();
    let bytes = document.finish().expect("document finishes without errors");

    let pdf = String::from_utf8_lossy(&bytes);
    assert!(
        pdf.contains("/OutputIntents"),
        "PDF should contain /OutputIntents catalogue entry"
    );
    assert!(
        pdf.contains("/Type /OutputIntent"),
        "PDF should contain /Type /OutputIntent dictionary entry"
    );
    assert!(
        pdf.contains("/S /GTS_PDFX"),
        "fallback intent should use /S /GTS_PDFX subtype"
    );
    assert!(
        pdf.contains("/DestOutputProfile"),
        "fallback intent should reference the supplied profile via /DestOutputProfile"
    );
    assert!(
        pdf.contains("(CMYK)"),
        "fallback intent should declare CMYK output condition"
    );
}

#[test]
fn fallback_cmyk_profile_yields_to_explicit_custom_output_intent() {
    use krilla::icc::ICCProfile;
    use krilla::{
        CustomOutputIntent, CustomOutputIntentSubtype, Document, OutputIntentProfile,
        SerializeSettings,
    };

    let cmyk_bytes = std::fs::read(crate::ASSETS_PATH.join("icc/eciCMYK_v2.icc")).unwrap();
    let cmyk_profile = ICCProfile::<4>::new(&cmyk_bytes).unwrap();
    let rgb_bytes =
        std::fs::read(crate::WORKSPACE_PATH.join("crates/krilla/icc/sRGB-v4.icc")).unwrap();
    let rgb_profile = ICCProfile::<3>::new(&rgb_bytes).unwrap();
    let explicit_intent = CustomOutputIntent::new(
        CustomOutputIntentSubtype::Custom("ISO_PDFE1".to_string()),
        OutputIntentProfile::Rgb(rgb_profile),
        "sRGB explicit".to_string(),
        "Explicit caller intent".to_string(),
    )
    .expect("intent fields are non-empty");

    let settings = SerializeSettings {
        output_intents: vec![explicit_intent],
        fallback_cmyk_profile: Some(cmyk_profile),
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );
    let mut page = document.start_page();
    page.surface().finish();
    page.finish();
    let bytes = document.finish().expect("document finishes without errors");

    let pdf = String::from_utf8_lossy(&bytes);
    assert!(
        pdf.contains("/S /ISO_PDFE1"),
        "explicit caller intent should be emitted verbatim"
    );
    assert!(
        !pdf.contains("/S /GTS_PDFX"),
        "fallback CMYK intent must not fire when a caller intent is present"
    );
}

#[test]
fn validate_pdf_x4_emits_gts_pdfx_version_and_trapped() {
    use krilla::configure::{ConfigurationBuilder, Pdfx};
    use krilla::icc::ICCProfile;
    use krilla::page::PageSettings;
    use krilla::{Document, SerializeSettings};

    let cmyk_bytes = std::fs::read(crate::ASSETS_PATH.join("icc/eciCMYK_v2.icc")).unwrap();
    let cmyk = ICCProfile::<4>::new(&cmyk_bytes).unwrap();
    let config = ConfigurationBuilder::new()
        .with_pdfx_validator(Pdfx::X4)
        .finish()
        .unwrap();

    let settings = SerializeSettings {
        configuration: config,
        cmyk_profile: Some(cmyk),
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .title("X-4 test".to_string())
            .creation_date(DateTime::new(2026)),
    );
    let page_settings = PageSettings::default().with_trim_box(Some(
        krilla::geom::Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap(),
    ));
    let mut page = document.start_page_with(page_settings);
    page.surface().finish();
    page.finish();
    let bytes = document.finish().expect("PDF/X-4 finishes cleanly");
    let pdf = String::from_utf8_lossy(&bytes);

    assert!(
        pdf.contains("/GTS_PDFXVersion (PDF/X-4)"),
        "/GTS_PDFXVersion (PDF/X-4) must be present in the Info dict"
    );
    assert!(
        pdf.contains("/Trapped /False"),
        "/Trapped /False must be present in the Info dict"
    );
    assert!(
        pdf.contains("/S /GTS_PDFX"),
        "/S /GTS_PDFX output-intent subtype must be present"
    );
}

#[test]
fn validate_pdf_x4_missing_trim_box_raises_error() {
    use krilla::configure::{ConfigurationBuilder, Pdfx};
    use krilla::icc::ICCProfile;
    use krilla::{Document, SerializeSettings};

    let cmyk_bytes = std::fs::read(crate::ASSETS_PATH.join("icc/eciCMYK_v2.icc")).unwrap();
    let cmyk = ICCProfile::<4>::new(&cmyk_bytes).unwrap();
    let config = ConfigurationBuilder::new()
        .with_pdfx_validator(Pdfx::X4)
        .finish()
        .unwrap();

    let settings = SerializeSettings {
        configuration: config,
        cmyk_profile: Some(cmyk),
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .title("X-4 missing-trim".to_string())
            .creation_date(DateTime::new(2026)),
    );
    // No trim or art box.
    let mut page = document.start_page();
    page.surface().finish();
    page.finish();

    let errs = validation_errors(document.finish());
    assert!(
        errs.iter()
            .any(|e| matches!(e, ValidationError::MissingTrimOrArtBox(0, _))),
        "expected MissingTrimOrArtBox; got {errs:?}"
    );
}

#[test]
fn validate_pdf_x1a_with_rgb_raises_contains_rgb() {
    use krilla::configure::{ConfigurationBuilder, Pdfx};
    use krilla::icc::ICCProfile;
    use krilla::page::PageSettings;
    use krilla::{Document, SerializeSettings};

    let cmyk_bytes = std::fs::read(crate::ASSETS_PATH.join("icc/eciCMYK_v2.icc")).unwrap();
    let cmyk = ICCProfile::<4>::new(&cmyk_bytes).unwrap();
    let config = ConfigurationBuilder::new()
        .with_pdfx_validator(Pdfx::X1A)
        .finish()
        .unwrap();

    let settings = SerializeSettings {
        configuration: config,
        cmyk_profile: Some(cmyk),
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .title("X-1a RGB".to_string())
            .creation_date(DateTime::new(2026)),
    );
    let page_settings = PageSettings::default().with_trim_box(Some(
        krilla::geom::Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap(),
    ));
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));
    surface.finish();
    page.finish();

    let errs = validation_errors(document.finish());
    assert!(
        errs.iter().any(|e| matches!(e, ValidationError::ContainsRgb(_))),
        "expected ContainsRgb under PDF/X-1a; got {errs:?}"
    );
}

#[test]
fn validate_pdf_x4p_requires_external_profile() {
    use krilla::configure::{ConfigurationBuilder, Pdfx};
    use krilla::icc::ICCProfile;
    use krilla::page::PageSettings;
    use krilla::{Document, SerializeSettings};

    let cmyk_bytes = std::fs::read(crate::ASSETS_PATH.join("icc/eciCMYK_v2.icc")).unwrap();
    let cmyk = ICCProfile::<4>::new(&cmyk_bytes).unwrap();
    let config = ConfigurationBuilder::new()
        .with_pdfx_validator(Pdfx::X4P)
        .finish()
        .unwrap();

    // No external_output_profile supplied → expect the configuration error.
    let settings = SerializeSettings {
        configuration: config,
        cmyk_profile: Some(cmyk),
        ..crate::settings_1()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .title("X-4p missing profile".to_string())
            .creation_date(DateTime::new(2026)),
    );
    let page_settings = PageSettings::default().with_trim_box(Some(
        krilla::geom::Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap(),
    ));
    let mut page = document.start_page_with(page_settings);
    page.surface().finish();
    page.finish();

    let errs = validation_errors(document.finish());
    assert!(
        errs.iter()
            .any(|e| matches!(e, ValidationError::ExternalOutputProfileRequiresX4P)),
        "expected ExternalOutputProfileRequiresX4P; got {errs:?}"
    );
}

