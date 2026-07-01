use std::num::{NonZeroU16, NonZeroU32};

use krilla::action::{Action, LinkAction};
use krilla::annotation::{LinkAnnotation, Target};
use krilla::error::KrillaError;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::metadata::Metadata;
use krilla::outline::Outline;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::surface::Surface;
use krilla::tagging::{
    Artifact, ArtifactType, BBox, ColumnDimensions, ContentTag, NaiveRgbColor, Node, Sides,
    SpanTag, TagGroup, TagTree,
};
use krilla::tagging::{
    ListNumbering, Placement, StructRole, TableHeaderScope, Tag, TagId, TagNamespace,
    WritingMode,
};
use krilla::text::{Font, TextDirection};
use krilla::{Document, SerializeSettings};
use krilla_macros::snapshot;
use krilla_svg::{SurfaceExt, SvgSettings};

use crate::{
    green_fill, load_png_image, loc, rect_to_path, red_stroke, settings_1, settings_25,
    NOTO_SANS, SVGS_PATH,
};

pub trait SurfaceTaggingExt {
    fn fill_text_(&mut self, y: f32, content: &str);
    fn outline_text_(&mut self, y: f32, content: &str);
}

impl SurfaceTaggingExt for Surface<'_> {
    fn fill_text_(&mut self, y: f32, content: &str) {
        let font_data = NOTO_SANS.clone();
        let font = Font::new(font_data, 0).unwrap();

        self.draw_text(
            Point::from_xy(0.0, y),
            font,
            20.0,
            content,
            false,
            TextDirection::Auto,
        );
    }

    fn outline_text_(&mut self, y: f32, content: &str) {
        let font_data = NOTO_SANS.clone();
        let font = Font::new(font_data, 0).unwrap();

        self.draw_text(
            Point::from_xy(0.0, y),
            font,
            20.0,
            content,
            true,
            TextDirection::Auto,
        );
    }
}

#[snapshot(document)]
fn tagging_empty(document: &mut Document) {
    let tag_root = TagTree::new();
    document.set_tag_tree(tag_root);
}

fn tagging_simple_impl(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut par = TagGroup::new(Tag::P);

    let mut page = document.start_page();
    let mut surface = page.surface();
    let id = surface.start_tagged(ContentTag::Span(SpanTag {
        lang: Some("en"),
        alt_text: Some("an alt text"),
        expanded: Some("expanded"),
        actual_text: Some("actual text"),
    }));
    surface.fill_text_(25.0, "a paragraph");
    surface.end_tagged();

    surface.finish();
    page.finish();

    par.push(id);
    tag_tree.push(par);

    document.set_tag_tree(tag_tree);
}

fn tagging_simple_with_link_impl(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut par = TagGroup::new(Tag::P);
    let mut link = TagGroup::new(Tag::Link);

    let mut page = document.start_page();
    let mut surface = page.surface();
    let id = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(25.0, "a paragraph");
    surface.end_tagged();

    surface.finish();

    let link_id = page.add_tagged_annotation(
        LinkAnnotation::new(
            Rect::from_xywh(0.0, 0.0, 100.0, 25.0).unwrap(),
            Target::Action(Action::Link(LinkAction::new("www.youtube.com".to_string()))),
        )
        .into(),
    );

    page.finish();

    link.push(link_id);
    link.push(id);
    par.push(link);
    tag_tree.push(par);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_simple(document: &mut Document) {
    tagging_simple_impl(document);
}

#[snapshot(document)]
fn tagging_simple_with_link(document: &mut Document) {
    tagging_simple_with_link_impl(document);
}

#[snapshot(document, settings_12)]
fn tagging_disabled(document: &mut Document) {
    tagging_simple_impl(document);
}

#[snapshot(document, settings_12)]
fn tagging_disabled_2(document: &mut Document) {
    tagging_simple_with_link_impl(document);
}

pub(crate) fn sample_svg() -> usvg::Tree {
    let data = std::fs::read(SVGS_PATH.join("resvg_shapes_rect_simple_case.svg")).unwrap();
    usvg::Tree::from_data(&data, &usvg::Options::default()).unwrap()
}

#[snapshot(document)]
fn tagging_image_with_alt(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut image_group =
        TagGroup::new(Tag::Figure(Some("This is the alternate text.".to_string())));

    let mut page = document.start_page();
    let mut surface = page.surface();

    let id = surface.start_tagged(ContentTag::Other);
    let tree = sample_svg();
    surface.draw_svg(
        &tree,
        Size::from_wh(tree.size().width(), tree.size().height()).unwrap(),
        SvgSettings::default(),
    );
    surface.end_tagged();

    surface.finish();
    page.finish();

    image_group.push(id);
    tag_tree.push(image_group);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_multiple_content_tags(document: &mut Document) {
    let mut tag_tree = TagTree::new();

    let mut page = document.start_page();
    let mut surface = page.surface();
    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(25.0, "a span");
    surface.end_tagged();
    let id2 = surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(
        ArtifactType::Header,
    )));
    surface.fill_text_(50.0, "a header artifact");
    surface.end_tagged();
    let id3 = surface.start_tagged(ContentTag::Other);
    surface.draw_path(&rect_to_path(50.0, 50.0, 100.0, 100.0));
    surface.end_tagged();

    let id4 = surface.start_tagged(ContentTag::Other);
    let tree = sample_svg();
    surface.push_transform(&Transform::from_translate(100.0, 100.0));
    surface.draw_svg(
        &tree,
        Size::from_wh(tree.size().width(), tree.size().height()).unwrap(),
        SvgSettings::default(),
    );
    surface.pop();
    surface.end_tagged();

    let id5 = surface.start_tagged(ContentTag::Other);
    let image = load_png_image("rgb8.png");
    let image_size = Size::from_wh(image.size().0 as f32, image.size().1 as f32).unwrap();
    surface.push_transform(&Transform::from_translate(100.0, 300.0));
    surface.draw_image(image, image_size);
    surface.pop();
    surface.end_tagged();

    let id6 = surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(
        ArtifactType::Other,
    )));
    surface.fill_text_(75.0, "a different type of artifact");
    surface.end_tagged();

    surface.finish();
    page.finish();

    tag_tree.push(id1);
    tag_tree.push(id2);
    tag_tree.push(id3);
    tag_tree.push(id4);
    tag_tree.push(id5);
    tag_tree.push(id6);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_multiple_pages(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut par_1 = TagGroup::new(Tag::P);
    let mut par_2 = TagGroup::new(Tag::P);
    let mut heading_1 = TagGroup::new(Tag::Hn(
        NonZeroU16::new(1).unwrap(),
        Some("first heading".into()),
    ));
    let mut heading_2 = TagGroup::new(Tag::Hn(
        NonZeroU16::new(1).unwrap(),
        Some("second heading".into()),
    ));

    let mut page = document.start_page();
    let mut surface = page.surface();
    let h1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(25.0, "a heading");
    surface.end_tagged();
    let p1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(50.0, "a paragraph");
    surface.end_tagged();
    surface.finish();
    page.finish();

    let mut page = document.start_page();
    let mut surface = page.surface();
    let p2 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(75.0, "a second paragraph");
    surface.end_tagged();
    surface.finish();
    page.finish();

    let mut page = document.start_page();
    let mut surface = page.surface();
    let h2 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(25.0, "another heading");
    surface.end_tagged();
    let p3 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(50.0, "another paragraph");
    surface.end_tagged();
    surface.finish();
    page.finish();

    heading_1.push(h1);
    par_1.push(p1);
    par_1.push(p2);

    heading_2.push(h2);
    par_2.push(p3);

    let mut sect1 = TagGroup::new(Tag::Section);
    sect1.push(heading_1);
    sect1.push(par_1);
    let mut sect2 = TagGroup::new(Tag::Section);
    sect2.push(heading_2);
    sect2.push(par_2);

    tag_tree.push(sect1);
    tag_tree.push(sect2);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_heading_level_7_and_8_pdf_17(document: &mut Document) {
    tagging_heading_level_7_and_8_impl(document);
}

#[snapshot(document, settings_25)]
fn tagging_heading_level_7_and_8_pdf_20(document: &mut Document) {
    tagging_heading_level_7_and_8_impl(document);
}

fn tagging_heading_level_7_and_8_impl(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut page = document.start_page();
    let mut surface = page.surface();
    let mut offset = 25.0;

    let mut new_heading = |level, name| {
        let hn = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
        surface.fill_text_(offset, name);
        offset += 25.0;
        surface.end_tagged();

        let level = NonZeroU16::new(level).unwrap();
        let mut heading = TagGroup::new(Tag::Hn(level, Some(name.into())));
        heading.push(hn);

        let mut sect = TagGroup::new(Tag::Section);
        sect.push(heading);

        sect
    };

    let mut sect_1 = new_heading(1, "first heading");
    let mut sect_2 = new_heading(2, "second heading");
    let mut sect_3 = new_heading(3, "third heading");
    let mut sect_4 = new_heading(4, "fourth heading");
    let mut sect_5 = new_heading(5, "fifth heading");
    let mut sect_6 = new_heading(6, "sixth heading");
    let mut sect_7 = new_heading(7, "senventh heading");
    let sect_8 = new_heading(8, "eigth heading");

    surface.finish();
    page.finish();

    sect_7.push(sect_8);
    sect_6.push(sect_7);
    sect_5.push(sect_6);
    sect_4.push(sect_5);
    sect_3.push(sect_4);
    sect_2.push(sect_3);
    sect_1.push(sect_2);

    tag_tree.push(sect_1);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_two_footnotes(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut fn_group_1 = TagGroup::new(Tag::Note);
    let mut fn_group_2 = TagGroup::new(Tag::Note);

    let mut page = document.start_page();
    let mut surface = page.surface();

    let id1 = surface.start_tagged(ContentTag::Other);
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&rect_to_path(50.0, 50.0, 100.0, 100.0));
    surface.end_tagged();

    let id2 = surface.start_tagged(ContentTag::Other);
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&rect_to_path(100.0, 100.0, 150.0, 150.0));
    surface.end_tagged();

    surface.finish();
    page.finish();

    fn_group_1.push(id1);
    fn_group_2.push(id2);
    tag_tree.push(fn_group_1);
    tag_tree.push(fn_group_2);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_table_header_and_footer(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let header_id = |x: usize| TagId::from(format!("Header {x}").into_bytes());
    let cell_text = |surface: &mut Surface, x: usize, y: usize, content: &str| {
        let font_data = NOTO_SANS.clone();
        let font = Font::new(font_data, 0).unwrap();

        surface.draw_text(
            Point::from_xy(x as f32 * 200.0, y as f32 * 100.0 + 50.0),
            font,
            20.0,
            content,
            false,
            TextDirection::Auto,
        );
    };

    let header = {
        let mut row = TagGroup::new(Tag::TR);
        for x in 0..3 {
            let text = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
            cell_text(&mut surface, x, 0, &format!("heading {}", x + 1));
            surface.end_tagged();

            let tag = Tag::TH(TableHeaderScope::Column).with_id(Some(header_id(x)));
            row.push(TagGroup::with_children(tag, vec![Node::Leaf(text)]));
        }
        TagGroup::with_children(Tag::THead, vec![Node::Group(row)])
    };

    let mut body = TagGroup::new(Tag::TBody);
    for y in 1..4 {
        let mut row = TagGroup::new(Tag::TR);
        for x in 0..3 {
            let text = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
            cell_text(&mut surface, x, y, &format!("body {} {}", x + 1, y + 1));
            surface.end_tagged();

            let tag = Tag::TD.with_headers(Some([header_id(x)]));
            row.push(TagGroup::with_children(tag, vec![Node::Leaf(text)]));
        }
        body.push(row);
    }

    let footer = {
        let text = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
        cell_text(&mut surface, 1, 4, "footer");
        surface.end_tagged();

        let cell = Tag::TD
            .with_row_span(Some(NonZeroU32::new(2).unwrap()))
            .with_col_span(Some(NonZeroU32::new(3).unwrap()))
            .with_headers(Some((0..3).map(header_id)));
        let cell = TagGroup::with_children(cell, vec![Node::Leaf(text)]);

        let row = TagGroup::with_children(Tag::TR, vec![Node::Group(cell)]);
        // Empty row to ensure proper table structure because of the rowspan.
        let empty_row = TagGroup::new(Tag::TR);
        TagGroup::with_children(Tag::TFoot, vec![row.into(), empty_row.into()])
    };

    surface.finish();
    page.finish();

    let mut table = TagGroup::new(Tag::Table.with_summary(Some("table summary".into())));
    table.push(header);
    table.push(body);
    table.push(footer);

    tag_tree.push(table);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_empty_table_cell_headers(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let mut row1 = TagGroup::new(Tag::TR);
    let mut th = TagGroup::new(Tag::TH(TableHeaderScope::Column));

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(0.0, "header");
    surface.end_tagged();

    th.push(id1);
    row1.push(th);

    let mut row2 = TagGroup::new(Tag::TR);
    let mut td = TagGroup::new(Tag::TD.with_headers(Some([])));

    let id2 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(25.0, "header");
    surface.end_tagged();

    td.push(id2);
    row2.push(td);

    surface.finish();
    page.finish();

    let mut table = TagGroup::new(Tag::Table);
    table.push(row1);
    table.push(row2);

    tag_tree.push(table);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_tag_attributes(document: &mut Document) {
    let mut tag_tree = TagTree::new();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let logo = surface.start_tagged(ContentTag::Artifact(Artifact::default()));
    surface.outline_text_(100.0, "NASA");
    surface.end_tagged();

    surface.finish();
    page.finish();

    let figure = Tag::Figure(Some("The NASA logo".into()))
        .with_actual_text(Some("NASA".into()))
        .with_expanded(Some("National Aeronautics and Space Administration".into()))
        .with_lang(Some("en".into()));

    tag_tree.push(TagGroup::with_children(figure, vec![Node::Leaf(logo)]));

    document.set_tag_tree(tag_tree);
}

#[snapshot(document)]
fn tagging_artifact_subtypes(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();

    surface.start_tagged(ContentTag::Artifact(Artifact::new(
        ArtifactType::Watermark,
        Some(Rect::from_xywh(1.0, 88.0, 21.0, 10.0).unwrap()),
    )));
    surface.outline_text_(100.0, "++");
    surface.end_tagged();

    surface.finish();
    page.finish();
}

#[snapshot(document, settings_15)]
fn tagging_figure_bounds(document: &mut Document) {
    document.set_metadata(Metadata::new().title("Figure".into()).language("en".into()));
    document.set_outline(Outline::new());

    let mut tag_tree = TagTree::new();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let id1 = surface.start_tagged(ContentTag::Other);
    let image = load_png_image("rgb8.png");
    let image_size = Size::from_wh(image.size().0 as f32, image.size().1 as f32).unwrap();
    surface.push_transform(&Transform::from_translate(100.0, 300.0));
    surface.draw_image(image, image_size);
    surface.pop();
    surface.end_tagged();

    surface.finish();
    page.finish();

    let rect = Rect::from_xywh(100.0, 300.0, image_size.width(), image_size.height()).unwrap();
    let figure_tag = Tag::Figure(Some("a gradient".into()))
        // removing this bbox will cause PAC 2024 to complain
        .with_bbox(Some(BBox::new(0, rect)))
        .with_width(Some(image_size.width()))
        .with_height(Some(image_size.height()));
    let figure = TagGroup::with_children(figure_tag, vec![id1.into()]);

    let par = TagGroup::with_children(Tag::P, vec![figure.into()]);

    tag_tree.push(par);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document, settings_15)]
fn tagging_layout_placement_and_writing_mode(document: &mut Document) {
    document.set_metadata(Metadata::new().title("Layout".into()).language("en".into()));
    document.set_outline(Outline::new());

    let mut tag_tree = TagTree::new();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let text = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(100.0, "\"Some quoted text\"");
    surface.end_tagged();

    surface.finish();
    page.finish();

    let mut quote = TagGroup::new(Tag::InlineQuote.with_placement(Some(Placement::Inline)));
    quote.push(text);

    let mut par = TagGroup::new(
        Tag::P
            .with_writing_mode(Some(WritingMode::LrTb))
            .with_placement(Some(Placement::Block)),
    );
    par.push(quote);

    tag_tree.push(par);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document, settings_15)]
fn tagging_div_and_border_color(document: &mut Document) {
    document.set_metadata(
        Metadata::new()
            .title("Tagged Borders".into())
            .language("en".into()),
    );
    document.set_outline(Outline::new());

    let mut tag_tree = TagTree::new();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let text = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(100.0, "\"Some quoted text\"");
    surface.end_tagged();

    surface.finish();
    page.finish();

    let red = NaiveRgbColor::new(0xFF, 0x00, 0x00);
    let blue = NaiveRgbColor::new(0x00, 0x00, 0xFF);
    let mut div = TagGroup::new(
        Tag::Div
            .with_border_color(Some(Sides::new(red, blue, red, blue)))
            .with_column_count(Some(NonZeroU32::new(2).unwrap()))
            .with_column_widths(Some(ColumnDimensions::all(50.0))),
    );
    let mut p = TagGroup::new(Tag::P);

    p.push(text);
    div.push(p);

    tag_tree.push(div);

    document.set_tag_tree(tag_tree);
}

#[snapshot(document, settings_15)]
fn tagging_strong_and_em_pdf_17(document: &mut Document) {
    tagging_strong_and_em_impl(document);
}

#[snapshot(document, settings_25)]
fn tagging_strong_and_em_pdf_20(document: &mut Document) {
    tagging_strong_and_em_impl(document);
}

#[snapshot(document, settings_25)]
fn tagging_custom_namespace_pdf_20(document: &mut Document) {
    let mut page = document.start_page();
    let mut surface = page.surface();
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&rect_to_path(20.0, 20.0, 80.0, 80.0));
    surface.finish();
    page.finish();

    let mut datetime = TagGroup::new(Tag::Datetime);
    datetime.push(TagGroup::new(Tag::Span));

    let mut tag_tree = TagTree::new();
    tag_tree.push(datetime);
    document.set_tag_tree(tag_tree);
}

fn tagging_strong_and_em_impl(document: &mut Document) {
    document.set_metadata(
        Metadata::new()
            .title("Strong and Em".into())
            .language("en".into()),
    );
    document.set_outline(Outline::new());

    let mut tag_tree = TagTree::new();
    let mut page = document.start_page();
    let mut surface = page.surface();

    let id1 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(100.0, "STRONG TEXT");
    surface.end_tagged();
    let mut strong = TagGroup::new(Tag::Strong);
    strong.push(id1);

    let id2 = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
    surface.fill_text_(100.0, "emphasized text");
    surface.end_tagged();
    let mut em = TagGroup::new(Tag::Em);
    em.push(id2);

    surface.finish();
    page.finish();

    let mut p = TagGroup::new(Tag::P);
    p.push(strong);
    p.push(em);

    tag_tree.push(p);

    document.set_tag_tree(tag_tree);
}

#[test]
#[should_panic]
fn tagging_page_identifer_appears_twice() {
    let mut document = Document::new();
    let mut tag_tree = TagTree::new();
    let mut fn_group_1 = TagGroup::new(Tag::P);
    let mut fn_group_2 = TagGroup::new(Tag::P);

    let mut page = document.start_page();
    let mut surface = page.surface();

    let id1 = surface.start_tagged(ContentTag::Other);
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&rect_to_path(50.0, 50.0, 100.0, 100.0));
    surface.end_tagged();

    surface.finish();
    page.finish();

    fn_group_1.push(id1);
    fn_group_2.push(id1);
    tag_tree.push(fn_group_1);
    tag_tree.push(fn_group_2);

    document.set_tag_tree(tag_tree);

    let _ = document.finish();
}

#[test]
fn tagging_id_appears_twice() {
    let mut document = Document::new();
    let mut tag_tree = TagTree::new();

    let id = TagId::from(*b"one");
    let loc_1 = loc(1);
    let loc_2 = loc(2);
    let group_1 = TagGroup::new(Tag::P.with_id(Some(id.clone())).with_location(Some(loc_1)));
    let group_2 = TagGroup::new(Tag::P.with_id(Some(id.clone())).with_location(Some(loc_2)));

    tag_tree.push(group_1);
    tag_tree.push(group_2);

    document.set_tag_tree(tag_tree);

    assert_eq!(
        document.finish(),
        Err(KrillaError::DuplicateTagId(id, Some(loc_2)))
    );
}

#[test]
fn tagging_unknown_header_tag_id() {
    let mut document = Document::new();
    let mut tag_tree = TagTree::new();

    let id = TagId::from(*b"one");
    let loc_1 = loc(1);
    let group_1 = TagGroup::new(
        Tag::TD
            .with_headers(Some([id.clone()]))
            .with_location(Some(loc_1)),
    );

    tag_tree.push(group_1);

    document.set_tag_tree(tag_tree);

    assert_eq!(
        document.finish(),
        Err(KrillaError::UnknownTagId(id, Some(loc_1)))
    );
}

#[test]
#[should_panic]
fn tagging_annotation_identifer_appears_twice() {
    let mut document = Document::new();
    let mut tag_tree = TagTree::new();
    let mut fn_group_1 = TagGroup::new(Tag::P);
    let mut fn_group_2 = TagGroup::new(Tag::P);

    let mut page = document.start_page();
    let link_id = page.add_tagged_annotation(
        LinkAnnotation::new(
            Rect::from_xywh(0.0, 0.0, 100.0, 25.0).unwrap(),
            Target::Action(Action::Link(LinkAction::new("www.youtube.com".to_string()))),
        )
        .into(),
    );
    page.finish();

    fn_group_1.push(link_id);
    fn_group_2.push(link_id);
    tag_tree.push(fn_group_1);
    tag_tree.push(fn_group_2);

    document.set_tag_tree(tag_tree);

    let _ = document.finish();
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn pretty(settings: SerializeSettings) -> SerializeSettings {
    SerializeSettings {
        pretty: true,
        ..settings
    }
}

fn document_with(settings: SerializeSettings, tag_tree: TagTree) -> Vec<u8> {
    let mut document = Document::new_with(settings);
    document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
    document.set_tag_tree(tag_tree);
    document.finish().unwrap()
}

#[test]
fn role_map_user_entry_added_to_pdf_17_dict() {
    let tag_tree = TagTree::new().with_role_map([("MyCustom", StructRole::P)]);
    let pdf = document_with(pretty(settings_1()), tag_tree);

    assert!(
        contains(&pdf, b"/RoleMap"),
        "/RoleMap dict missing from PDF 1.7 output"
    );
    assert!(
        contains(&pdf, b"/MyCustom /P"),
        "user-supplied role mapping missing from /RoleMap"
    );
}

#[test]
fn role_map_user_override_replaces_builtin_in_place() {
    // Built-in: Strong -> Span. User overrides Strong -> H1.
    let tag_tree = TagTree::new().with_role_map([("Strong", StructRole::H1)]);
    let pdf = document_with(pretty(settings_1()), tag_tree);

    assert!(
        contains(&pdf, b"/Strong /H1"),
        "user override (Strong -> H1) missing"
    );
    assert!(
        !contains(&pdf, b"/Strong /Span"),
        "built-in Strong -> Span survived user override"
    );
}

#[test]
fn namespace_override_changes_pdf_20_bytes() {
    // Same document, same tag — only difference is the per-tag
    // namespace override. The two produced PDFs must differ
    // somewhere: the struct element for Tag::P binds to the SSN by
    // default, and to the krilla namespace under the override.
    let default_doc = {
        let mut document = Document::new_with(pretty(settings_25()));
        document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
        let mut tag_tree = TagTree::new();
        tag_tree.push(TagGroup::new(Tag::P));
        document.set_tag_tree(tag_tree);
        document.finish().unwrap()
    };
    let overridden_doc = {
        let mut document = Document::new_with(pretty(settings_25()));
        document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
        let mut tag_tree = TagTree::new();
        tag_tree.push(TagGroup::new(Tag::P.with_namespace(Some(TagNamespace::Krilla))));
        document.set_tag_tree(tag_tree);
        document.finish().unwrap()
    };

    assert_ne!(
        default_doc, overridden_doc,
        "namespace override on Tag::P produced identical bytes — override did not take effect"
    );
    // Both files declare the krilla namespace URL at the document
    // level; the override doesn't change that. What changes is
    // which dict the struct element's /NS pair points at.
    let url = b"https://github.com/LaurenzV/krilla";
    assert!(
        contains(&default_doc, url),
        "krilla namespace URL missing from default PDF 2.0 output"
    );
    assert!(
        contains(&overridden_doc, url),
        "krilla namespace URL missing from overridden PDF 2.0 output"
    );
}

#[test]
fn namespace_override_ignored_on_pdf_17() {
    // Under PDF 1.7 the namespace model does not exist; the
    // override must be silently dropped so the produced bytes
    // match the no-override baseline byte-for-byte.
    let default_doc = {
        let mut document = Document::new_with(pretty(settings_1()));
        document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
        let mut tag_tree = TagTree::new();
        tag_tree.push(TagGroup::new(Tag::P));
        document.set_tag_tree(tag_tree);
        document.finish().unwrap()
    };
    let overridden_doc = {
        let mut document = Document::new_with(pretty(settings_1()));
        document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
        let mut tag_tree = TagTree::new();
        tag_tree.push(TagGroup::new(Tag::P.with_namespace(Some(TagNamespace::Krilla))));
        document.set_tag_tree(tag_tree);
        document.finish().unwrap()
    };

    assert_eq!(
        default_doc, overridden_doc,
        "namespace override leaked into PDF 1.7 output — should be silently ignored below PDF 2.0"
    );
}

// --- custom external namespaces (MathML, HTML 4, …) -----------------

const MATHML_URI: &str = "http://www.w3.org/1998/Math/MathML";
const HTML4_URI: &str = "http://www.w3.org/TR/REC-html40";

#[test]
fn register_namespace_emits_namespace_dict_under_pdf_20() {
    let mut document = Document::new_with(pretty(settings_25()));
    let _ = document.register_namespace(MATHML_URI);
    document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
    let mut tag_tree = TagTree::new();
    tag_tree.push(TagGroup::new(Tag::P));
    document.set_tag_tree(tag_tree);
    let pdf = document.finish().unwrap();

    // The MathML URI must appear verbatim inside a Namespace
    // dict, and the dict must be referenced from the catalogue's
    // /Namespaces array. The URL is a UTF-16BE text string in
    // PDF 2.0 tagged output; search for the ASCII bytes — they
    // appear in clear because the URL is all ASCII.
    assert!(
        contains(&pdf, MATHML_URI.as_bytes()),
        "MathML namespace URI missing from output",
    );
    assert!(
        contains(&pdf, b"/Namespaces"),
        "/Namespaces array missing from catalogue",
    );
}

#[test]
fn register_namespace_is_idempotent() {
    let mut document = Document::new_with(pretty(settings_25()));
    let h1 = document.register_namespace(MATHML_URI);
    let h2 = document.register_namespace(MATHML_URI);
    let h3 = document.register_namespace(HTML4_URI);
    assert_eq!(h1, h2, "registering the same URI twice must return the same handle");
    assert_ne!(h1, h3, "distinct URIs must return distinct handles");

    document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
    let mut tag_tree = TagTree::new();
    tag_tree.push(TagGroup::new(Tag::P));
    document.set_tag_tree(tag_tree);
    let pdf = document.finish().unwrap();

    // Exactly one MathML Namespace dict — the second registration
    // must not produce a duplicate.
    let count = pdf
        .windows(MATHML_URI.len())
        .filter(|w| *w == MATHML_URI.as_bytes())
        .count();
    assert_eq!(
        count, 1,
        "MathML URI appears {count} times; expected exactly 1",
    );
}

#[test]
fn tag_with_custom_namespace_changes_bytes() {
    let default_doc = {
        let mut document = Document::new_with(pretty(settings_25()));
        document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
        let mut tag_tree = TagTree::new();
        tag_tree.push(TagGroup::new(Tag::Formula(None)));
        document.set_tag_tree(tag_tree);
        document.finish().unwrap()
    };
    let bound_doc = {
        let mut document = Document::new_with(pretty(settings_25()));
        let mathml = document.register_namespace(MATHML_URI);
        document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
        let mut tag_tree = TagTree::new();
        // Bind the Formula tag to MathML rather than the SSN.
        tag_tree.push(TagGroup::new(
            Tag::Formula(None).with_namespace(Some(TagNamespace::Custom(mathml))),
        ));
        document.set_tag_tree(tag_tree);
        document.finish().unwrap()
    };

    assert_ne!(
        default_doc, bound_doc,
        "Tag::with_namespace(Custom(mathml)) must change the emitted bytes",
    );
    assert!(
        contains(&bound_doc, MATHML_URI.as_bytes()),
        "MathML URI missing from the bound document",
    );
    assert!(
        !contains(&default_doc, MATHML_URI.as_bytes()),
        "MathML URI leaked into the default document (no registration)",
    );
}

#[test]
fn register_namespace_silently_dropped_on_pdf_17() {
    // PDF 1.7 has no namespace model; the registration must not
    // affect the produced bytes.
    let no_ns_doc = {
        let mut document = Document::new_with(pretty(settings_1()));
        document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
        document.finish().unwrap()
    };
    let with_ns_doc = {
        let mut document = Document::new_with(pretty(settings_1()));
        let _ = document.register_namespace(MATHML_URI);
        document.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
        document.finish().unwrap()
    };
    assert_eq!(
        no_ns_doc, with_ns_doc,
        "register_namespace must not affect the bytes of a PDF 1.7 document",
    );
}

#[test]
fn role_map_omitted_under_pdf_20_namespaces() {
    // PDF 2.0 uses /Namespaces + /RoleMapNS (a namespace-keyed
    // mapping inside each namespace dict) instead of the flat
    // /RoleMap dict; krilla intentionally ignores the user
    // role_map on that path because the mapping is not directly
    // expressible in the namespace model without also telling
    // krilla which namespace to bind the custom name to.
    let tag_tree = TagTree::new().with_role_map([("MyCustom", StructRole::P)]);
    let pdf = document_with(pretty(settings_25()), tag_tree);

    assert!(
        contains(&pdf, b"/Namespaces"),
        "/Namespaces array missing from PDF 2.0 output"
    );
    assert!(
        !contains(&pdf, b"/MyCustom /P"),
        "user role mapping leaked into PDF 2.0 output"
    );
}

#[test]
#[should_panic]
fn tagging_missing_identifier_in_tree() {
    let mut document = Document::new();
    let tag_tree = TagTree::new();

    let mut page = document.start_page();
    let mut surface = page.surface();

    let _ = surface.start_tagged(ContentTag::Other);
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&rect_to_path(50.0, 50.0, 100.0, 100.0));
    surface.end_tagged();

    surface.finish();
    page.finish();

    document.set_tag_tree(tag_tree);

    let _ = document.finish();
}

#[test]
fn aside_emits_pdf_20_role_with_ssn_namespace() {
    // Under PDF 2.0 the `Aside` role lives in the standard structure
    // namespace (SSN) per ISO 32000-2 §14.8.4.3. The bytes must
    // contain `/S /Aside` and reference the SSN URL `iso.org/pdf2/ssn`.
    let mut tag_tree = TagTree::new();
    tag_tree.push(TagGroup::new(Tag::Aside));
    let pdf = document_with(pretty(settings_25()), tag_tree);

    assert!(
        contains(&pdf, b"/S /Aside"),
        "/S /Aside missing from PDF 2.0 output for Tag::Aside"
    );
    assert!(
        contains(&pdf, b"iso.org/pdf2/ssn"),
        "PDF 2.0 SSN namespace URL missing from output containing Tag::Aside"
    );
}

#[test]
fn aside_falls_back_to_div_on_pdf_17() {
    // PDF 1.7 has no `Aside` role; the compat path emits `/S /Div`
    // (matching `StructRole2::Aside.compatibility_1_7(default)`).
    let mut tag_tree = TagTree::new();
    tag_tree.push(TagGroup::new(Tag::Aside));
    let pdf = document_with(pretty(settings_1()), tag_tree);

    assert!(
        contains(&pdf, b"/S /Div"),
        "PDF 1.7 fallback for Tag::Aside should emit /S /Div"
    );
    assert!(
        !contains(&pdf, b"/S /Aside"),
        "Tag::Aside must not emit /S /Aside in PDF 1.7 output"
    );
}

#[test]
fn sub_emits_pdf_20_role_with_ssn_namespace() {
    // Under PDF 2.0 the `Sub` role lives in the SSN per ISO 32000-2
    // §14.8.4.6. The bytes must contain `/S /Sub` and reference the
    // SSN URL.
    let mut tag_tree = TagTree::new();
    tag_tree.push(TagGroup::new(Tag::Sub));
    let pdf = document_with(pretty(settings_25()), tag_tree);

    assert!(
        contains(&pdf, b"/S /Sub"),
        "/S /Sub missing from PDF 2.0 output for Tag::Sub"
    );
    assert!(
        contains(&pdf, b"iso.org/pdf2/ssn"),
        "PDF 2.0 SSN namespace URL missing from output containing Tag::Sub"
    );
}

#[test]
fn sub_emits_custom_role_on_pdf_17() {
    // PDF 1.7 has no `Sub` standard role; krilla emits a custom
    // role `Sub` (custom_kind) and registers a `/Sub -> /Span`
    // mapping in the document `/RoleMap`. This mirrors how `Strong`
    // and `Em` are handled so the inline-level semantic survives.
    let mut tag_tree = TagTree::new();
    tag_tree.push(TagGroup::new(Tag::Sub));
    let pdf = document_with(pretty(settings_1()), tag_tree);

    assert!(
        contains(&pdf, b"/S /Sub"),
        "Tag::Sub must emit a custom /S /Sub element on PDF 1.7"
    );
    assert!(
        contains(&pdf, b"/Sub /Span"),
        "Tag::Sub must register /Sub -> /Span in the PDF 1.7 /RoleMap"
    );
}
