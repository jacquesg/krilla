use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::{cmyk, luma, rgb, separation};
use krilla::configure::ValidationError;
use krilla::embed::EmbedError;
use krilla::error::KrillaError;
use krilla::geom::{Point, Rect, Size};
use krilla::metadata::{DateTime, Metadata};
use krilla::num::NormalizedF32;
use krilla::outline::Outline;
use krilla::paint::{Fill, FillRule, LinearGradient, SpreadMethod, Stop};
use krilla::tagging::{ArtifactType, ContentTag, SpanTag, TagGroup, TagKind, TagTree};
use krilla::tagging::{ListNumbering, TableHeaderScope, Tag};
use krilla::text::{Font, TextDirection};
use krilla::text::{GlyphId, KrillaGlyph};
use krilla_macros::snapshot;

use crate::embed::{embedded_file_impl, file_1};
use crate::{
    blue_fill, cmyk_fill, dummy_text_with_spans, green_fill, load_jpg_image, load_png_image, loc,
    metadata_1, pdfx_external_output_profile, rect_to_path, red_fill, settings_13, settings_15,
    settings_19, settings_20, settings_23, settings_24, settings_31, settings_32, settings_33,
    settings_34, settings_35, settings_36, settings_37, settings_38, settings_39, settings_40,
    settings_41, settings_7, settings_8, settings_9, stops_with_2_solid_1, youtube_link,
    NOTO_SANS,
};
use crate::{Document, SerializeSettings};

fn pdfa_document() -> Document {
    let mut document = Document::new_with(settings_7());
    document.set_metadata(metadata_1());
    document
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
    document.set_metadata(metadata_1());

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

    surface.finish();
    page.finish();
    document.set_metadata(metadata_1());
}

#[test]
pub fn validate_pdf_a_q_nesting_28() {
    let document = q_nesting_impl(settings_7());
    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::TooHighQNestingLevel
        ]))
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
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::TooLongString
        ]))
    );
}

#[snapshot(document, settings_7)]
fn validate_pdf_a_annotation(document: &mut Document) {
    let page_settings = krilla::page::PageSettings::from_wh(200.0, 200.0).unwrap();
    let mut page = document.start_page_with(page_settings);
    page.add_annotation(
        LinkAnnotation::new(
            Rect::from_xywh(50.0, 50.0, 100.0, 100.0).unwrap(),
            Target::Action(LinkAction::new("https://www.youtube.com".to_string()).into()),
        )
        .into(),
    );
    page.finish();
    document.set_metadata(metadata_1());
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
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::ContainsPostScript(None)
        ]))
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
    document.set_metadata(metadata_1());
}

#[test]
fn validate_pdf_a_missing_cmyk() {
    let mut document = pdfa_document();
    cmyk_document_impl(&mut document);

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::MissingCMYKProfile
        ]))
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
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::ContainsNotDefGlyph(font, None, "你".to_string())
        ]))
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
    document.set_metadata(metadata_1());

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::ContainsNotDefGlyph(font, Some(loc(4)), "i".to_string())
        ]))
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
    document.set_metadata(metadata_1());

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::Transparency(Some(loc(4))),
            ValidationError::Transparency(Some(loc(6))),
            // Note that we don't have 7 here, even though we should in theory. The reason is
            // that since we cache graphics states, only the first time we serialize it will
            // it trigger the validation error. Not optimal, but changing that would be a pain.
        ]))
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

    let id2 = surface.start_tagged(ContentTag::Artifact(ArtifactType::Header));
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
    document.set_metadata(metadata_1());
}

#[test]
fn validate_pdfu_invalid_codepoint() {
    let mut document = Document::new_with(settings_9());
    let font_data = NOTO_SANS.clone();
    let font = Font::new(font_data, 0).unwrap();
    invalid_codepoint_impl(&mut document, font.clone(), "A\u{FEFF}B");

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::InvalidCodepointMapping(font, GlyphId::new(2), '\u{FEFF}', None)
        ]))
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
    document.set_metadata(metadata_1());

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::NoCodepointMapping(
                font,
                GlyphId::new(1),
                None
            )));
        }
        _ => panic!("Expected validation error"),
    }
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
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::UnicodePrivateArea(font, GlyphId::new(2), '\u{E022}', None)
        ]))
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

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::MissingAnnotationAltText(Some(annot_loc))));
        }
        _ => panic!("Expected validation error"),
    }
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

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::MissingAltText(Some(formula_loc))));
        }
        _ => panic!("Expected validation error"),
    }
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
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::MissingDocumentOutline,
            ValidationError::MissingAnnotationAltText(Some(annot_loc)),
            ValidationError::MissingAltText(Some(formula_loc)),
            ValidationError::NoDocumentTitle
        ]))
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
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::Transparency(None)
        ]))
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
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::Transparency(None)
        ]))
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
    document.set_metadata(metadata_1());

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::TooLargeFloat,
            ValidationError::TooLongArray,
        ]))
    )
}

#[test]
fn validate_pdf_a3_a_no_tag_tree() {
    let mut document = Document::new_with(settings_24());
    document.set_metadata(metadata_1());

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::MissingTagging
        ]))
    )
}

#[test]
fn validate_pdf_a3_missing_fields() {
    let mut d = Document::new_with(settings_23());
    let mut f1 = file_1();
    f1.description = None;
    f1.modification_date = None;
    d.embed_file(f1);
    d.set_metadata(metadata_1());

    assert_eq!(
        d.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::EmbeddedFile(EmbedError::MissingDate, None),
            ValidationError::EmbeddedFile(EmbedError::MissingDescription, None)
        ]))
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
    document.set_metadata(metadata_1());

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::MissingCMYKProfile
        ]))
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

#[snapshot(document, settings_41)]
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

#[snapshot(document, settings_41)]
fn validate_pdf_ua2_only_annotation(document: &mut Document) {
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

#[test]
fn validate_pdf_ua2_missing_requirements() {
    let mut document = Document::new_with(settings_41());
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
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::MissingDocumentOutline,
            ValidationError::MissingAnnotationAltText(Some(annot_loc)),
            ValidationError::MissingAltText(Some(formula_loc)),
            ValidationError::NoDocumentTitle,
            ValidationError::NoDocumentLanguage,
        ]))
    )
}

#[test]
fn validate_pdf_ua2_empty_alt() {
    let mut document = Document::new_with(settings_41());
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

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::MissingAltText(Some(formula_loc))));
        }
        _ => panic!("Expected validation error"),
    }
}

#[test]
fn validate_pdf_ua2_empty_annotation_alt() {
    let mut document = Document::new_with(settings_41());
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

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::MissingAnnotationAltText(Some(annot_loc))));
        }
        _ => panic!("Expected validation error"),
    }
}

#[test]
fn validate_pdf_ua2_missing_tagging() {
    let mut document = Document::new_with(settings_41());
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .title("a nice title".to_string()),
    );
    let outline = Outline::new();
    document.set_outline(outline);

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::MissingTagging,
        ]))
    )
}

// ---- WTPDF 1.0 tests ----

#[snapshot(document, settings_40)]
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
    let mut document = Document::new_with(settings_40());

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::MissingTagging,
        ]))
    )
}

#[test]
fn validate_wtpdf_allows_minimal_metadata() {
    // WTPDF, unlike UA-2, does not mandate a document title, language,
    // alt text on figures, an outline, or DisplayDocTitle. A minimally
    // tagged document with no metadata at all must therefore succeed.
    let mut document = Document::new_with(settings_40());
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
    let mut document = Document::new_with(settings_40());
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

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::ContainsNotDefGlyph(font, None, "你".to_string()),
        ]))
    )
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
    document.set_metadata(metadata_1());

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::Transparency(None),
            ValidationError::Transparency(Some(loc(2)))
        ]))
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
    document.set_metadata(metadata_1());

    assert_eq!(
        document.finish(),
        Err(KrillaError::Validation(vec![
            ValidationError::InconsistentSeparationFallback(
                separation::SeparationColorant::Custom("PANTONE 185 C".to_string())
            )
        ]))
    );
}

// ---- PDF/X tests ----

use krilla::page::PageSettings;

fn pdfx_page_settings() -> PageSettings {
    let ps = PageSettings::from_wh(200.0, 200.0).unwrap();
    let trim = Rect::from_xywh(0.0, 0.0, 200.0, 200.0).unwrap();
    ps.with_trim_box(Some(trim))
}

/// Helper that creates a valid PDF/X document with text and a shape.
/// Uses CMYK fill for X-1a compatibility.
fn validate_pdf_x_full_example_cmyk(document: &mut Document) {
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
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

    surface.set_fill(Some(cmyk_fill(1.0)));
    surface.draw_path(&rect_to_path(30.0, 30.0, 70.0, 70.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X Document".to_string());
    document.set_metadata(metadata);
}

/// Helper that creates a valid PDF/X document using RGB (for X-3, X-4).
fn validate_pdf_x_full_example_rgb(document: &mut Document) {
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
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

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X Document".to_string());
    document.set_metadata(metadata);
}

fn pdfx_validation_document(settings: SerializeSettings) -> Document {
    let mut document = Document::new_with(settings);
    let mut page = document.start_page_with(pdfx_page_settings());
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    document
}

// ---- PDF/X snapshot tests ----

#[snapshot(document, settings_31)]
fn validate_pdf_x4_full_example(document: &mut Document) {
    validate_pdf_x_full_example_rgb(document);
}

#[snapshot(document, settings_32)]
fn validate_pdf_x3_full_example(document: &mut Document) {
    validate_pdf_x_full_example_rgb(document);
}

#[snapshot(document, settings_33)]
fn validate_pdf_x1a_full_example(document: &mut Document) {
    validate_pdf_x_full_example_cmyk(document);
}

#[snapshot(document, settings_34)]
fn validate_pdf_x4p_full_example(document: &mut Document) {
    validate_pdf_x_full_example_rgb(document);
}

#[snapshot(document, settings_35)]
fn validate_pdf_x6_full_example(document: &mut Document) {
    validate_pdf_x_full_example_rgb(document);
}

#[snapshot(document, settings_39)]
fn validate_pdf_x6p_full_example(document: &mut Document) {
    validate_pdf_x_full_example_rgb(document);
}

#[snapshot(document, settings_37)]
fn validate_pdf_a2b_x4_full_example(document: &mut Document) {
    validate_pdf_x_full_example_rgb(document);
}

#[snapshot(document, settings_38)]
fn validate_pdf_a3b_x4_full_example(document: &mut Document) {
    validate_pdf_x_full_example_rgb(document);
}

#[snapshot(document, settings_36)]
fn validate_pdf_a1b_x1a_full_example(document: &mut Document) {
    validate_pdf_x_full_example_cmyk(document);
}

// ---- PDF/X unit tests ----

#[test]
fn validate_pdf_x1a_no_rgb() {
    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::ContainsRgb(_))));
        }
        other => panic!("expected ContainsRgb error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x1a_no_rgb_image() {
    use krilla::image::Image;

    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    let image_data = std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/images/rgb8.png"),
    )
    .unwrap();
    let image = Image::from_png(image_data.into(), false).unwrap();
    surface.draw_image(image, Size::from_wh(50.0, 50.0).unwrap());

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::ContainsRgb(_))));
        }
        other => panic!("expected ContainsRgb error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x1a_luma_ok() {
    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    let fill = Fill {
        paint: luma::Color::new(128).into(),
        opacity: NormalizedF32::ONE,
        rule: FillRule::default(),
    };
    surface.set_fill(Some(fill));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_pdf_x1a_no_annotations() {
    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);

    page.add_annotation(Annotation::new_link(
        LinkAnnotation::new(
            Rect::from_xywh(0.0, 0.0, 100.0, 20.0).unwrap(),
            Target::Action(LinkAction::new("https://example.com".to_string()).into()),
        ),
        None,
    ));

    let mut surface = page.surface();
    surface.set_fill(Some(cmyk_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));
    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::ContainsAnnotation(None)));
        }
        other => panic!("expected ContainsAnnotation error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x1a_no_transparency() {
    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(cmyk_fill(0.5)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::Transparency(None)));
        }
        other => panic!("expected Transparency error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x3_no_transparency() {
    let mut document = Document::new_with(settings_32());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(0.5)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-3".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::Transparency(None)));
        }
        other => panic!("expected Transparency error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x4_transparency_ok() {
    let mut document = Document::new_with(settings_31());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(0.5)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_pdf_x_missing_trim_art_box() {
    let mut document = Document::new_with(settings_31());
    let mut page = document.start_page();
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::MissingTrimOrArtBox(0, _))));
        }
        other => panic!("expected MissingTrimOrArtBox error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x_with_trim_box() {
    let mut document = Document::new_with(settings_31());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_pdf_x_with_art_box() {
    let mut document = Document::new_with(settings_31());
    let ps = PageSettings::from_wh(200.0, 200.0).unwrap();
    let art = Rect::from_xywh(0.0, 0.0, 200.0, 200.0).unwrap();
    let page_settings = ps.with_art_box(Some(art));
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_pdf_x1a_cmyk_fill_ok() {
    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(cmyk_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_pdf_x_missing_date() {
    let mut document = Document::new_with(settings_31());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new().language("en".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::MissingDocumentDate));
        }
        other => panic!("expected MissingDocumentDate error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x1a_no_title() {
    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(cmyk_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::NoDocumentTitle));
        }
        other => panic!("expected NoDocumentTitle error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x3_no_title() {
    let mut document = Document::new_with(settings_32());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::NoDocumentTitle));
        }
        other => panic!("expected NoDocumentTitle error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x4_no_title_ok() {
    let mut document = Document::new_with(settings_31());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    // PDF/X-4 does NOT require a title.
    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_pdf_x1a_separation_cmyk_fallback_ok() {
    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    let space = separation::SeparationSpace::new(
        separation::SeparationColorant::Custom("PANTONE 185 C".to_string()),
        cmyk::Color::new(0, 255, 255, 0).into(),
    );
    let color: krilla::color::Color = separation::Color::new(255, space).into();
    let fill = Fill {
        paint: color.into(),
        opacity: NormalizedF32::ONE,
        rule: FillRule::default(),
    };
    surface.set_fill(Some(fill));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_pdf_x1a_separation_rgb_fallback() {
    let mut document = Document::new_with(settings_33());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    let space = separation::SeparationSpace::new(
        separation::SeparationColorant::Custom("PANTONE 185 C".to_string()),
        rgb::Color::new(255, 0, 0).into(),
    );
    let color: krilla::color::Color = separation::Color::new(255, space).into();
    let fill = Fill {
        paint: color.into(),
        opacity: NormalizedF32::ONE,
        rule: FillRule::default(),
    };
    surface.set_fill(Some(fill));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::ContainsRgb(_))));
        }
        other => panic!("expected ContainsRgb error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x1a_missing_cmyk_profile() {
    use krilla::configure::{Configuration, Validator};

    // X1A without a CMYK profile should trigger MissingCMYKProfile.
    let settings = SerializeSettings {
        configuration: Configuration::new_with_validator(Validator::X1A),
        // No cmyk_profile provided.
        ..crate::settings_1()
    };
    let mut document = Document::new_with(settings);
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(cmyk_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/X-1a".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::MissingCMYKProfile));
        }
        other => panic!("expected MissingCMYKProfile error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_a1b_x1a_combined_rejects_rgb() {
    // The combined A1B_X1A validator should reject RGB (from X1A)
    // AND transparency (from both A1B and X1A).
    let mut document = Document::new_with(settings_36());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    // RGB fill: forbidden by X1A constituent.
    surface.set_fill(Some(red_fill(0.5))); // Also transparent: forbidden by A1B.
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001))
        .title("PDF/A-1b + PDF/X-1a".to_string());
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ValidationError::ContainsRgb(_))),
                "expected ContainsRgb, got {errors:?}"
            );
            assert!(
                errors.contains(&ValidationError::Transparency(None)),
                "expected Transparency, got {errors:?}"
            );
        }
        other => panic!("expected validation errors, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x_variants_require_document_date_without_metadata_object() {
    for (name, settings) in [
        ("X4", settings_31()),
        ("X4P", settings_34()),
        ("X6", settings_35()),
        ("X6P", settings_39()),
        ("A2B_X4", settings_37()),
        ("A3B_X4", settings_38()),
    ] {
        let document = pdfx_validation_document(settings);

        match document.finish() {
            Err(KrillaError::Validation(errors)) => {
                assert!(
                    errors.contains(&ValidationError::MissingDocumentDate),
                    "{name}: expected MissingDocumentDate, got {errors:?}"
                );
            }
            other => panic!("{name}: expected MissingDocumentDate error, got {other:?}"),
        }
    }
}

#[test]
fn validate_pdf_x_embedded_output_intent_variants_require_cmyk_profile() {
    use krilla::configure::{Configuration, Validator};

    for (name, validator) in [
        ("X3", Validator::X3),
        ("X4", Validator::X4),
        ("X6", Validator::X6),
        ("A2B_X4", Validator::A2B_X4),
        ("A3B_X4", Validator::A3B_X4),
    ] {
        let settings = SerializeSettings {
            configuration: Configuration::new_with_validator(validator),
            ..crate::settings_1()
        };
        let mut document = Document::new_with(settings);
        let mut page = document.start_page_with(pdfx_page_settings());
        let mut surface = page.surface();

        surface.set_fill(Some(red_fill(1.0)));
        surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));
        surface.finish();
        page.finish();

        document.set_metadata(
            Metadata::new()
                .language("en".to_string())
                .creation_date(DateTime::new(2001))
                .title(name.to_string()),
        );

        match document.finish() {
            Err(KrillaError::Validation(errors)) => {
                assert!(
                    errors.contains(&ValidationError::MissingCMYKProfile),
                    "{name}: expected MissingCMYKProfile, got {errors:?}"
                );
            }
            other => panic!("{name}: expected MissingCMYKProfile error, got {other:?}"),
        }
    }
}

#[test]
fn validate_pdf_x4_writes_required_xmp_fields() {
    let mut document = Document::new_with(settings_31());
    let mut page = document.start_page_with(pdfx_page_settings());
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(0.5)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));
    surface.finish();
    page.finish();

    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001))
            .title("PDF/X-4".to_string()),
    );

    let pdf = document.finish().unwrap();
    let pdf_text = String::from_utf8_lossy(&pdf);

    assert!(pdf_text.starts_with("%PDF-1.6"));
    assert!(pdf_text.contains("<xmp:MetadataDate>2001-01-01T00:00:00Z</xmp:MetadataDate>"));
    assert!(pdf_text.contains("<xmpMM:VersionID>1</xmpMM:VersionID>"));
}

#[test]
fn validate_pdf_x1a_gradient_checks_every_stop() {
    let mut document = Document::new_with(settings_33());
    let mut page = document.start_page_with(pdfx_page_settings());
    let mut surface = page.surface();

    let gradient = LinearGradient {
        x1: 0.0,
        y1: 0.0,
        x2: 100.0,
        y2: 0.0,
        transform: Default::default(),
        spread_method: SpreadMethod::Pad,
        stops: vec![
            Stop {
                offset: NormalizedF32::ZERO,
                color: cmyk::Color::new(255, 0, 0, 0).into(),
                opacity: NormalizedF32::ONE,
            },
            Stop {
                offset: NormalizedF32::ONE,
                color: rgb::Color::new(255, 0, 0).into(),
                opacity: NormalizedF32::ONE,
            },
        ],
        anti_alias: false,
    };

    surface.set_fill(Some(Fill {
        paint: gradient.into(),
        opacity: NormalizedF32::ONE,
        rule: FillRule::default(),
    }));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001))
            .title("PDF/X-1a".to_string()),
    );

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ValidationError::ContainsRgb(_))),
                "expected ContainsRgb, got {errors:?}"
            );
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ValidationError::MixedGradientColorSpaces(_))),
                "expected MixedGradientColorSpaces, got {errors:?}"
            );
        }
        other => panic!("expected gradient validation errors, got {other:?}"),
    }
}

#[test]
fn validate_mixed_gradient_stop_spaces_fail_cleanly_without_a_validator() {
    let mut document = pdfx_validation_document(crate::settings_1());
    let mut page = document.start_page_with(pdfx_page_settings());
    let mut surface = page.surface();

    let gradient = LinearGradient {
        x1: 0.0,
        y1: 0.0,
        x2: 100.0,
        y2: 0.0,
        transform: Default::default(),
        spread_method: SpreadMethod::Pad,
        stops: vec![
            Stop {
                offset: NormalizedF32::ZERO,
                color: cmyk::Color::new(255, 0, 0, 0).into(),
                opacity: NormalizedF32::ONE,
            },
            Stop {
                offset: NormalizedF32::ONE,
                color: luma::Color::new(0).into(),
                opacity: NormalizedF32::ONE,
            },
        ],
        anti_alias: false,
    };

    surface.set_fill(Some(Fill {
        paint: gradient.into(),
        opacity: NormalizedF32::ONE,
        rule: FillRule::default(),
    }));
    surface.draw_path(&rect_to_path(60.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ValidationError::MixedGradientColorSpaces(_))),
                "expected MixedGradientColorSpaces, got {errors:?}"
            );
        }
        other => panic!("expected MixedGradientColorSpaces error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x4p_requires_external_output_profile() {
    use krilla::configure::{Configuration, Validator};

    let settings = SerializeSettings {
        configuration: Configuration::new_with_validator(Validator::X4P),
        ..crate::settings_1()
    };
    let mut document = pdfx_validation_document(settings);

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::MissingExternalOutputProfile));
        }
        other => panic!("expected MissingExternalOutputProfile error, got {other:?}"),
    }
}

#[test]
fn external_output_profile_rejects_invalid_input() {
    use krilla::icc::ICCProfile;
    use krilla::{ExternalOutputProfile, ExternalOutputProfileError};

    let profile_bytes =
        std::fs::read(crate::WORKSPACE_PATH.join("crates/krilla/icc/sRGB-v4.icc")).unwrap();
    let profile = ICCProfile::<3>::new(&profile_bytes).unwrap();

    assert_eq!(
        ExternalOutputProfile::rgb(
            profile.clone(),
            vec![],
            "Custom".to_string(),
            "info".to_string(),
        )
        .err(),
        Some(ExternalOutputProfileError::EmptyUrls)
    );

    assert_eq!(
        ExternalOutputProfile::rgb(
            profile.clone(),
            vec!["   ".to_string()],
            "Custom".to_string(),
            "info".to_string(),
        )
        .err(),
        Some(ExternalOutputProfileError::EmptyUrls)
    );

    assert_eq!(
        ExternalOutputProfile::rgb(
            profile.clone(),
            vec!["https://example.com/profile.icc".to_string()],
            "   ".to_string(),
            "info".to_string(),
        )
        .err(),
        Some(ExternalOutputProfileError::EmptyIdentifier)
    );

    assert_eq!(
        ExternalOutputProfile::rgb(
            profile,
            vec!["https://example.com/profile.icc".to_string()],
            "Custom".to_string(),
            "   ".to_string(),
        )
        .err(),
        Some(ExternalOutputProfileError::EmptyInfo)
    );
}

#[test]
fn validate_x4_rejects_external_output_profile() {
    use krilla::configure::{Configuration, Validator};

    let settings = SerializeSettings {
        configuration: Configuration::new_with_validator(Validator::X4),
        external_output_profile: Some(pdfx_external_output_profile()),
        ..crate::settings_1()
    };
    let mut document = pdfx_validation_document(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::ExternalOutputProfileUnsupportedByValidator));
        }
        other => {
            panic!("expected ExternalOutputProfileUnsupportedByValidator error, got {other:?}")
        }
    }
}

#[test]
fn validate_pdf_x4p_with_external_profile_reference() {
    use krilla::configure::{Configuration, Validator};

    let settings = SerializeSettings {
        configuration: Configuration::new_with_validator(Validator::X4P),
        external_output_profile: Some(pdfx_external_output_profile()),
        ..crate::settings_1()
    };
    let mut document = pdfx_validation_document(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );

    let pdf = document.finish().unwrap();
    let pdf_text = String::from_utf8_lossy(&pdf);

    assert!(pdf_text.contains("/DestOutputProfileRef <<"));
    assert!(pdf_text.contains("/URLs ["));
    assert!(pdf_text.contains("/FS /URL"));
    assert!(pdf_text.contains("/F (https://example.com/profiles/sRGB-v4.icc)"));
    assert!(pdf_text.contains("/OutputConditionIdentifier (Custom)"));
    assert!(pdf_text.contains("/OutputCondition (sRGB)"));
    assert!(pdf_text.contains("/Info (sRGB v4 ICC profile)"));
    assert!(pdf_text.contains("/CheckSum <"));
    assert!(pdf_text.contains("/ICCVersion ("));
    assert!(pdf_text.contains("/ProfileCS ("));
    assert!(!pdf_text.contains("/DestOutputProfile "));
}

fn output_profile_refs(pdf_text: &str) -> Vec<&str> {
    let mut refs = Vec::new();
    let mut remainder = pdf_text;

    while let Some(start) = remainder.find("/DestOutputProfile ") {
        let tail = &remainder[start + "/DestOutputProfile ".len()..];
        let end = tail.find(" R").unwrap();
        refs.push(&tail[..end]);
        remainder = &tail[end + 2..];
    }

    refs
}

#[test]
fn validate_combined_pdfa_pdfx_declares_pdfx_extension_schema() {
    for (settings, use_cmyk) in [
        (settings_36(), true),
        (settings_37(), false),
        (settings_38(), false),
    ] {
        let mut document = Document::new_with(settings);
        let mut page = document.start_page_with(pdfx_page_settings());
        let mut surface = page.surface();

        surface.set_fill(Some(if use_cmyk {
            cmyk_fill(1.0)
        } else {
            red_fill(1.0)
        }));
        surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));
        surface.finish();
        page.finish();

        document.set_metadata(
            Metadata::new()
                .language("en".to_string())
                .creation_date(DateTime::new(2001))
                .title("Combined".to_string()),
        );

        let pdf = document.finish().unwrap();
        let pdf_text = String::from_utf8_lossy(&pdf);

        assert!(
            pdf_text.contains("xmlns:pdfxid=\"http://www.npes.org/pdfx/ns/id/\""),
            "missing pdfxid namespace declaration"
        );
        assert!(
            pdf_text.contains("<pdfaSchema:namespaceURI>http://www.npes.org/pdfx/ns/id/</pdfaSchema:namespaceURI>"),
            "missing PDF/A extension schema for pdfxid"
        );
        assert!(
            pdf_text.contains("<pdfaProperty:name>GTS_PDFXVersion</pdfaProperty:name>"),
            "missing PDF/A extension property declaration for GTS_PDFXVersion"
        );
    }
}

#[test]
fn validate_a1b_x1a_uses_one_output_profile_for_both_intents() {
    let mut document = Document::new_with(settings_36());
    let mut page = document.start_page_with(pdfx_page_settings());
    let mut surface = page.surface();

    surface.set_fill(Some(cmyk_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001))
            .title("PDF/A-1b + PDF/X-1a".to_string()),
    );

    let pdf = document.finish().unwrap();
    let pdf_text = String::from_utf8_lossy(&pdf);
    let refs = output_profile_refs(&pdf_text);

    assert_eq!(refs.len(), 2, "expected two output intents");
    assert_eq!(
        refs[0], refs[1],
        "combined output intents must share one ICC profile"
    );
}

#[test]
fn validate_pdf_x1a_uses_device_cmyk_for_page_content() {
    let mut document = Document::new_with(settings_33());
    let mut page = document.start_page_with(pdfx_page_settings());
    let mut surface = page.surface();

    surface.set_fill(Some(cmyk_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001))
            .title("PDF/X-1a".to_string()),
    );

    let pdf = document.finish().unwrap();
    let pdf_text = String::from_utf8_lossy(&pdf);

    assert!(
        !pdf_text.contains("/ColorSpace <<"),
        "PDF/X-1a page resources must not declare ICCBased aliases for page content"
    );
    assert!(
        !pdf_text.contains(" scn\n"),
        "PDF/X-1a page content should use device operators instead of ICCBased scn painting"
    );
}

#[test]
fn validate_a1b_x1a_uses_device_cmyk_for_page_content() {
    let mut document = Document::new_with(settings_36());
    let mut page = document.start_page_with(pdfx_page_settings());
    let mut surface = page.surface();

    surface.set_fill(Some(cmyk_fill(1.0)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001))
            .title("PDF/A-1b + PDF/X-1a".to_string()),
    );

    let pdf = document.finish().unwrap();
    let pdf_text = String::from_utf8_lossy(&pdf);

    assert!(
        !pdf_text.contains("/ColorSpace <<"),
        "PDF/A-1b + PDF/X-1a page resources must not declare ICCBased aliases for page content"
    );
    assert!(
        !pdf_text.contains(" scn\n"),
        "PDF/A-1b + PDF/X-1a page content should use device operators instead of ICCBased scn painting"
    );
}

#[test]
fn validate_pdfx_downgrades_unknown_trapping_to_not_trapped() {
    use krilla::metadata::Trapping;
    let mut document = pdfx_validation_document(settings_35());
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001))
            .trapped(Trapping::Unknown),
    );

    let pdf = document.finish().unwrap();
    let pdf_text = String::from_utf8_lossy(&pdf);

    // PDF/X forbids the Unknown trapping state; krilla downgrades it to
    // NotTrapped in both the Info dict and the XMP metadata.
    assert!(
        pdf_text.contains("/Trapped /False"),
        "PDF/X-6 with Trapping::Unknown must still write /Trapped /False"
    );
    assert!(pdf_text.contains("<pdf:Trapped>False</pdf:Trapped>"));
    assert!(!pdf_text.contains("/Trapped /Unknown"));
}

#[test]
fn validate_pdf_x6_writes_trapped_in_info_dict() {
    // ISO 15930-9 (PDF/X-6) mandates /Trapped in the Document Info dictionary
    // despite PDF 2.0's general deprecation of Info-dict keys. Ensure we
    // always emit it for X-6, plus the XMP pdf:Trapped counterpart.
    let mut document = pdfx_validation_document(settings_35());
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );

    let pdf = document.finish().unwrap();
    let pdf_text = String::from_utf8_lossy(&pdf);

    assert!(
        pdf_text.contains("/Trapped /False"),
        "expected /Trapped in Info dict for PDF/X-6"
    );
    assert!(pdf_text.contains("<pdf:Trapped>False</pdf:Trapped>"));
}

#[test]
fn validate_pdf_x6p_requires_external_output_profile() {
    use krilla::configure::{Configuration, Validator};

    let settings = SerializeSettings {
        configuration: Configuration::new_with_validator(Validator::X6P),
        ..crate::settings_1()
    };
    let mut document = pdfx_validation_document(settings);

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::MissingExternalOutputProfile));
        }
        other => panic!("expected MissingExternalOutputProfile error, got {other:?}"),
    }
}

#[test]
fn validate_pdf_x6p_transparency_ok() {
    let mut document = Document::new_with(settings_39());
    let page_settings = pdfx_page_settings();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();

    surface.set_fill(Some(red_fill(0.5)));
    surface.draw_path(&rect_to_path(0.0, 0.0, 50.0, 50.0));

    surface.finish();
    page.finish();

    let metadata = Metadata::new()
        .language("en".to_string())
        .creation_date(DateTime::new(2001));
    document.set_metadata(metadata);

    assert!(document.finish().is_ok());
}

#[test]
fn validate_pdf_x6p_with_external_profile_reference() {
    use krilla::configure::{Configuration, Validator};

    let settings = SerializeSettings {
        configuration: Configuration::new_with_validator(Validator::X6P),
        external_output_profile: Some(pdfx_external_output_profile()),
        ..crate::settings_1()
    };
    let mut document = pdfx_validation_document(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );

    let pdf = document.finish().unwrap();
    let pdf_text = String::from_utf8_lossy(&pdf);

    assert!(pdf_text.starts_with("%PDF-2.0"));
    assert!(pdf_text.contains("/DestOutputProfileRef <<"));
    assert!(pdf_text.contains("/S /GTS_PDFX"));
    assert!(!pdf_text.contains("/DestOutputProfile "));
    assert!(pdf_text.contains("GTS_PDFXVersion"));
    assert!(pdf_text.contains("PDF/X-6p"));
}

#[test]
fn validate_x6_rejects_external_output_profile() {
    use krilla::configure::{Configuration, Validator};

    // X6 (not X6P) should reject external output profile.
    let settings = SerializeSettings {
        configuration: Configuration::new_with_validator(Validator::X6),
        external_output_profile: Some(pdfx_external_output_profile()),
        ..crate::settings_1()
    };
    let mut document = pdfx_validation_document(settings);
    document.set_metadata(
        Metadata::new()
            .language("en".to_string())
            .creation_date(DateTime::new(2001)),
    );

    match document.finish() {
        Err(KrillaError::Validation(errors)) => {
            assert!(errors.contains(&ValidationError::ExternalOutputProfileUnsupportedByValidator));
        }
        other => {
            panic!("expected ExternalOutputProfileUnsupportedByValidator error, got {other:?}")
        }
    }
}

#[test]
fn custom_output_intent_rejects_invalid_input() {
    use krilla::icc::ICCProfile;
    use krilla::{
        CustomOutputIntent, CustomOutputIntentError, CustomOutputIntentSubtype, OutputIntentProfile,
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
    let mut surface = page.surface();
    surface.finish();
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
    let mut surface = page.surface();
    surface.finish();
    page.finish();
    let bytes = document.finish().expect("document finishes without errors");

    let pdf = String::from_utf8_lossy(&bytes);
    assert!(
        pdf.contains("/S /ISO_PDFE1"),
        "PDF should contain /S /ISO_PDFE1 verbatim for Custom subtype"
    );
}

#[test]
fn fallback_cmyk_profile_emits_default_output_intent() {
    use krilla::icc::ICCProfile;
    use krilla::{Document, SerializeSettings};

    let profile_bytes = std::fs::read(
        crate::ASSETS_PATH.join("icc/krilla-generic-cmyk-v2.icc"),
    )
    .unwrap();
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
    let mut surface = page.surface();
    surface.finish();
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

    let cmyk_bytes = std::fs::read(
        crate::ASSETS_PATH.join("icc/krilla-generic-cmyk-v2.icc"),
    )
    .unwrap();
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
    let mut surface = page.surface();
    surface.finish();
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
