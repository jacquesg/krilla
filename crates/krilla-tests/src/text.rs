use krilla::geom::Point;
use krilla::page::Page;
use krilla::paint::{Fill, LinearGradient, Paint, SpreadMethod, Stroke};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph, Tag, TextDirection};
use krilla::{Data, Document};
use krilla_macros::{snapshot, visreg};

use crate::{
    blue_fill, blue_stroke, red_fill, red_stroke, stops_with_3_solid_1, CANTARELL_VAR,
    LATIN_MODERN_ROMAN, LIBERTINUS_SERIF, NOTO_COLOR_EMOJI_COLR, NOTO_SANS, NOTO_SANS_ARABIC,
    NOTO_SANS_CJK, NOTO_SANS_DEVANAGARI, NOTO_SANS_VAR, TWITTER_COLOR_EMOJI,
};

fn text_gradient(spread_method: SpreadMethod) -> LinearGradient {
    LinearGradient {
        x1: 50.0,
        y1: 0.0,
        x2: 150.0,
        y2: 0.0,
        transform: Default::default(),
        spread_method,
        stops: stops_with_3_solid_1(),
        anti_alias: false,
    }
}

fn text_with_fill_impl(surface: &mut Surface, outlined: bool) {
    let font = Font::new(NOTO_SANS.clone(), 0).unwrap();
    surface.set_fill(Some(red_fill(0.5)));
    surface.draw_text(
        Point::from_xy(0.0, 80.0),
        font.clone(),
        20.0,
        "red outlined text",
        outlined,
        TextDirection::Auto,
    );

    surface.set_fill(Some(blue_fill(0.8)));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font.clone(),
        20.0,
        "blue outlined text",
        outlined,
        TextDirection::Auto,
    );

    let grad_fill = Fill {
        paint: Paint::from(text_gradient(SpreadMethod::Pad)),
        ..Default::default()
    };

    surface.set_fill(Some(grad_fill));
    surface.draw_text(
        Point::from_xy(0.0, 120.0),
        font.clone(),
        20.0,
        "gradient text",
        outlined,
        TextDirection::Auto,
    );

    let noto_font = Font::new(NOTO_COLOR_EMOJI_COLR.clone(), 0).unwrap();

    surface.set_fill(Some(blue_fill(0.8)));
    surface.draw_text(
        Point::from_xy(0.0, 140.0),
        noto_font.clone(),
        20.0,
        "😄😁😆",
        outlined,
        TextDirection::Auto,
    );

    let grad_fill = Fill {
        paint: Paint::from(text_gradient(SpreadMethod::Reflect)),
        ..Default::default()
    };

    surface.set_fill(Some(grad_fill));
    surface.draw_text(
        Point::from_xy(0.0, 160.0),
        font,
        20.0,
        "longer gradient text with repeat",
        outlined,
        TextDirection::Auto,
    );
}

#[visreg]
fn text_outlined_with_fill(surface: &mut Surface) {
    text_with_fill_impl(surface, true)
}

fn text_with_stroke_impl(surface: &mut Surface, outlined: bool) {
    let font = Font::new(NOTO_SANS.clone(), 0).unwrap();
    surface.set_stroke(Some(red_stroke(0.5, 1.0)));
    surface.draw_text(
        Point::from_xy(0.0, 80.0),
        font.clone(),
        20.0,
        "red outlined text",
        outlined,
        TextDirection::Auto,
    );

    surface.set_stroke(Some(blue_stroke(0.8)));
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font.clone(),
        20.0,
        "blue outlined text",
        outlined,
        TextDirection::Auto,
    );

    let grad_stroke = Stroke {
        paint: Paint::from(text_gradient(SpreadMethod::Pad)),
        ..Default::default()
    };

    surface.set_stroke(Some(grad_stroke));
    surface.draw_text(
        Point::from_xy(0.0, 120.0),
        font,
        20.0,
        "gradient text",
        outlined,
        TextDirection::Auto,
    );

    let font = Font::new(NOTO_COLOR_EMOJI_COLR.clone(), 0).unwrap();

    surface.set_stroke(Some(blue_stroke(0.8)));
    surface.draw_text(
        Point::from_xy(0.0, 140.0),
        font,
        20.0,
        "😄😁😆",
        outlined,
        TextDirection::Auto,
    );
}

#[visreg]
fn text_outlined_with_stroke(surface: &mut Surface) {
    text_with_stroke_impl(surface, true);
}

#[visreg]
fn text_zalgo(surface: &mut Surface) {
    let font = Font::new(NOTO_SANS.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        32.0,
        "z͈̤̭͖̉͑́a̳ͫ́̇͑̽͒ͯlͨ͗̍̀̍̔̀ģ͔̫̫̄o̗̠͔̦͆̏̓͢",
        false,
        TextDirection::Auto,
    );
}

#[visreg]
fn text_direction_ltr(surface: &mut Surface) {
    let font = Font::new(NOTO_SANS_CJK.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "你好这是一段则是文字",
        false,
        TextDirection::LeftToRight,
    );
}

#[visreg]
fn text_direction_rtl(surface: &mut Surface) {
    let font = Font::new(NOTO_SANS_CJK.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        20.0,
        "你好这是一段则是文字",
        false,
        TextDirection::RightToLeft,
    );
}

#[visreg]
fn text_direction_ttb(surface: &mut Surface) {
    let font = Font::new(NOTO_SANS_CJK.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(100.0, 0.0),
        font,
        20.0,
        "你好这是一段则是文字",
        false,
        TextDirection::TopToBottom,
    );
}

#[visreg]
fn text_direction_btt(surface: &mut Surface) {
    let font = Font::new(NOTO_SANS_CJK.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(100.0, 0.0),
        font,
        20.0,
        "你好这是一段则是文字",
        false,
        TextDirection::BottomToTop,
    );
}

#[snapshot]
fn text_direction_auto(page: &mut Page) {
    let font = Font::new(NOTO_SANS_ARABIC.clone(), 0).unwrap();
    let mut surface = page.surface();
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        32.0,
        "مرحبا بالعالم",
        false,
        TextDirection::Auto,
    );
}

pub(crate) fn simple_text_impl(page: &mut Page, font_data: Data) {
    let mut surface = page.surface();
    surface.draw_text(
        Point::from_xy(0.0, 50.0),
        Font::new(font_data, 0).unwrap(),
        16.0,
        "A line of text.",
        false,
        TextDirection::Auto,
    );
}

#[snapshot]
fn text_simple_cff(page: &mut Page) {
    simple_text_impl(page, LATIN_MODERN_ROMAN.clone());
}

#[snapshot]
fn text_simple_ttf(page: &mut Page) {
    simple_text_impl(page, NOTO_SANS.clone());
}

#[snapshot]
fn text_complex(page: &mut Page) {
    let mut surface = page.surface();
    surface.draw_text(
        Point::from_xy(0.0, 50.0),
        Font::new(NOTO_SANS_DEVANAGARI.clone(), 0).unwrap(),
        16.0,
        "यह कुछ जटिल पाठ है.",
        false,
        TextDirection::Auto,
    );
}

#[snapshot]
fn text_complex_2(page: &mut Page) {
    let mut surface = page.surface();
    surface.draw_text(
        Point::from_xy(0.0, 50.0),
        Font::new(NOTO_SANS_DEVANAGARI.clone(), 0).unwrap(),
        16.0,
        "यु॒धा नर॑ ऋ॒ष्वा",
        false,
        TextDirection::Auto,
    );
}

#[snapshot]
fn text_complex_3(page: &mut Page) {
    let mut surface = page.surface();
    surface.draw_text(
        Point::from_xy(0.0, 50.0),
        Font::new(NOTO_SANS_DEVANAGARI.clone(), 0).unwrap(),
        12.0,
        "आ रु॒क्मैरा यु॒धा नर॑ ऋ॒ष्वा ऋ॒ष्टीर॑सृक्षत ।",
        false,
        TextDirection::Auto,
    );
}

#[snapshot]
fn text_complex_4(page: &mut Page) {
    let mut surface = page.surface();
    surface.draw_text(
        Point::from_xy(0.0, 50.0),
        Font::new(NOTO_SANS_DEVANAGARI.clone(), 0).unwrap(),
        10.0,
        "अन्वे॑नाँ॒ अह॑ वि॒द्युतो॑ म॒रुतो॒ जज्झ॑तीरव भनर॑र्त॒ त्मना॑ दि॒वः ॥",
        false,
        TextDirection::Auto,
    );
}

#[snapshot]
// Tests https://github.com/typst/typst/issues/5654
fn text_small_caps(page: &mut Page) {
    let glyphs = vec![
        KrillaGlyph {
            glyph_id: GlyphId::new(2464),
            text_range: 0..1,
            x_advance: 0.529,
            x_offset: 0.0,
            y_offset: 0.0,
            y_advance: 0.0,
            location: None,
        },
        KrillaGlyph {
            glyph_id: GlyphId::new(2464),
            text_range: 1..2,
            x_advance: 0.529,
            x_offset: 0.0,
            y_offset: 0.0,
            y_advance: 0.0,
            location: None,
        },
    ];

    let mut surface = page.surface();
    surface.draw_glyphs(
        Point::from_xy(0.0, 50.0),
        &glyphs,
        Font::new(LIBERTINUS_SERIF.clone(), 0).unwrap(),
        "Tt",
        12.0,
        false,
    );
}

#[visreg]
fn text_zalgo_outlined(surface: &mut Surface) {
    let font = Font::new(NOTO_SANS.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        32.0,
        "z͈̤̭͖̉͑́a̳ͫ́̇͑̽͒ͯlͨ͗̍̀̍̔̀ģ͔̫̫̄o̗̠͔̦͆̏̓͢",
        true,
        TextDirection::Auto,
    );
}

#[snapshot]
fn text_fill(page: &mut Page) {
    let mut surface = page.surface();
    surface.draw_text(
        Point::from_xy(0.0, 50.0),
        Font::new(NOTO_SANS.clone(), 0).unwrap(),
        16.0,
        "hi there",
        false,
        TextDirection::Auto,
    );
}

#[snapshot]
fn text_stroke(page: &mut Page) {
    let mut surface = page.surface();
    surface.set_stroke(Some(Stroke::default()));
    surface.draw_text(
        Point::from_xy(0.0, 50.0),
        Font::new(NOTO_SANS.clone(), 0).unwrap(),
        16.0,
        "hi there",
        false,
        TextDirection::Auto,
    );
}

// This would be nicer as a snapshot test, but since it's a system font
// we can't include it in the repository.
// The point of the test is to check that fonts that do have a bitmap table
// will still embed a CID font for glyphs that don't have an entry in the
// bitmap table instead of falling back to a Type3 font.
#[cfg(target_os = "macos")]
#[visreg]
fn text_mixed_ttf_ebdt_font(surface: &mut Surface) {
    let data = std::fs::read("/System/Library/Fonts/Supplemental/PTSans.ttc").unwrap();
    let font = Font::new(data.into(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(0.0, 100.0),
        font,
        32.0,
        "Hi!",
        false,
        TextDirection::Auto,
    );
}

// See https://github.com/typst/typst/pull/5420#issuecomment-2768899483.
// Make sure snapshot is stable.
#[test]
fn text_two_fonts_reproducibility() {
    let render_single = || {
        let mut document = Document::new();
        let mut page = document.start_page();
        let mut surface = page.surface();

        surface.draw_text(
            Point::from_xy(0.0, 50.0),
            Font::new(NOTO_SANS.clone(), 0).unwrap(),
            16.0,
            "hi there",
            false,
            TextDirection::Auto,
        );
        surface.draw_text(
            Point::from_xy(0.0, 20.0),
            Font::new(NOTO_SANS_CJK.clone(), 0).unwrap(),
            16.0,
            "你好",
            false,
            TextDirection::Auto,
        );

        surface.finish();
        page.finish();
        document.finish().unwrap()
    };

    let expected = render_single();

    for _ in 0..10 {
        assert_eq!(expected, render_single());
    }
}

fn variable_impl(surface: &mut Surface, coords: Vec<Vec<(Tag, f32)>>, font: Data, text: &str) {
    let mut cur_y = 20.0;

    for coords in coords {
        let font = Font::new_variable(font.clone(), 0, &coords).unwrap();

        surface.draw_text(
            Point::from_xy(0.0, cur_y),
            font,
            16.0,
            text,
            false,
            TextDirection::Auto,
        );

        cur_y += 20.0;
    }
}

#[visreg]
fn text_variable_font(surface: &mut Surface) {
    let coords = vec![
        vec![(Tag::new(b"wght"), 400.0)],
        vec![(Tag::new(b"wght"), 100.0)],
        vec![(Tag::new(b"wght"), 900.0)],
        vec![(Tag::new(b"wght"), 900.0), (Tag::new(b"wdth"), 62.5)],
    ];

    variable_impl(
        surface,
        coords,
        NOTO_SANS_VAR.clone(),
        "I love variable fonts!",
    );
}

#[visreg]
fn text_variable_font_cff2(surface: &mut Surface) {
    let coords = vec![
        vec![(Tag::new(b"wght"), 400.0)],
        vec![(Tag::new(b"wght"), 100.0)],
        vec![(Tag::new(b"wght"), 900.0)],
    ];

    variable_impl(
        surface,
        coords,
        CANTARELL_VAR.clone(),
        "I love variable fonts!",
    );
}

/// Render the word "Hi" once with [`TextRendering::Glyphs`] and once
/// with [`TextRendering::Vector`], then assert the byte stream emits
/// text-showing operators in the first case and path operators in the
/// second.
///
/// Both passes disable content-stream compression
/// ([`SerializeSettings::compress_content_streams`] = `false`) so the
/// page content stream is inspectable as ASCII PDF operators. krilla
/// writes several auxiliary streams (CID-to-GID maps, embedded font
/// programmes) interleaved with the page content stream — rather than
/// trying to identify "the" page stream we scan the entire PDF for the
/// presence and absence of the operators of interest, which is
/// equivalent because the auxiliary streams never contain `BT`/`ET`
/// text-block markers.
#[test]
fn text_rendering_setting_switches_glyph_to_vector_emission() {
    use krilla::{SerializeSettings, TextRendering};

    fn render(setting: TextRendering) -> Vec<u8> {
        let settings = SerializeSettings {
            compress_content_streams: false,
            text_rendering: setting,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);
        let mut page = document.start_page();
        let mut surface = page.surface();
        surface.draw_text(
            Point::from_xy(50.0, 50.0),
            Font::new(NOTO_SANS.clone(), 0).unwrap(),
            32.0,
            "Hi",
            // `outlined: false` — the document-level setting must be the
            // only thing that flips the emission path.
            false,
            TextDirection::Auto,
        );
        surface.finish();
        page.finish();
        document.finish().unwrap()
    }

    fn contains_op(pdf: &[u8], op: &[u8]) -> bool {
        pdf.windows(op.len()).any(|w| w == op)
    }

    let glyphs_pdf = render(TextRendering::Glyphs);
    let vector_pdf = render(TextRendering::Vector);

    // Glyphs mode: text-block operators (`BT`/`ET`) frame a `Tj` call.
    assert!(
        contains_op(&glyphs_pdf, b"\nBT\n") || contains_op(&glyphs_pdf, b" BT\n"),
        "glyphs mode must emit a `BT` text-block operator",
    );
    assert!(
        contains_op(&glyphs_pdf, b"\nET\n") || contains_op(&glyphs_pdf, b" ET\n"),
        "glyphs mode must emit a matching `ET` text-block operator",
    );

    // Vector mode: text-block operators absent in the page content
    // stream. (Auxiliary streams produced by krilla — font programmes,
    // CMap, /ToUnicode — never embed `BT`/`ET`.)
    assert!(
        !contains_op(&vector_pdf, b"\nBT\n") && !contains_op(&vector_pdf, b" BT\n"),
        "vector mode must NOT emit a `BT` text-block operator",
    );

    // Vector mode: path operators (`m`/`l`/`c`/`h`/`f`) are present.
    // We assert on at least `moveto` + `fill`; the curve/line operator
    // is glyph-shape-dependent and not strictly required.
    let has_moveto = contains_op(&vector_pdf, b" m\n") || contains_op(&vector_pdf, b"\nm\n");
    let has_fill = contains_op(&vector_pdf, b" f\n") || contains_op(&vector_pdf, b"\nf\n");
    assert!(
        has_moveto && has_fill,
        "vector mode must emit path operators (`m` and `f`); \
             moveto={has_moveto} fill={has_fill}",
    );
}

/// Render the same string under each [`FontEmbedding`] mode and verify
/// the embedded font programme behaves as documented:
///
/// - `Subset`: the document references `/FontFile2` and the resulting
///   PDF is small (subset of NOTO_SANS).
/// - `Full`: the document references `/FontFile2` and the resulting
///   PDF is materially larger than the subset PDF — the full NOTO_SANS
///   font programme is several hundred kilobytes, whereas a one-word
///   subset is well under twenty kilobytes.
/// - `None`: the document does not reference `/FontFile2` or
///   `/FontFile3` at all.
#[test]
fn font_embedding_setting_controls_fontfile_emission() {
    use krilla::{FontEmbedding, SerializeSettings};

    fn render(setting: FontEmbedding) -> Vec<u8> {
        let settings = SerializeSettings {
            compress_content_streams: false,
            font_embedding: setting,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);
        let mut page = document.start_page();
        let mut surface = page.surface();
        surface.draw_text(
            Point::from_xy(50.0, 50.0),
            Font::new(NOTO_SANS.clone(), 0).unwrap(),
            32.0,
            "Hi",
            false,
            TextDirection::Auto,
        );
        surface.finish();
        page.finish();
        document.finish().unwrap()
    }

    fn contains(pdf: &[u8], needle: &[u8]) -> bool {
        pdf.windows(needle.len()).any(|w| w == needle)
    }

    let subset_pdf = render(FontEmbedding::Subset);
    let full_pdf = render(FontEmbedding::Full);
    let none_pdf = render(FontEmbedding::None);

    // Subset and Full both reference `/FontFile2` (NOTO_SANS is
    // TrueType, so the descriptor entry is FontFile2, not FontFile3).
    assert!(
        contains(&subset_pdf, b"/FontFile2"),
        "subset embedding must reference /FontFile2",
    );
    assert!(
        contains(&full_pdf, b"/FontFile2"),
        "full embedding must reference /FontFile2",
    );

    // None embedding omits the font programme entirely.
    assert!(
        !contains(&none_pdf, b"/FontFile2") && !contains(&none_pdf, b"/FontFile3"),
        "none embedding must not reference /FontFile2 or /FontFile3",
    );

    // The font programme dominates the PDF size for these tiny
    // documents. Full-embedding the entire NOTO_SANS file produces a
    // document at least four times the size of the two-glyph subset.
    // (NOTO_SANS Regular ships at ~500 KiB on disk; a "Hi" subset is
    // well under 20 KiB.) We use a 4x ratio as the assertion to leave
    // generous head-room for any future flate-encoding wins, while
    // still cleanly distinguishing the two embedding modes.
    let subset_len = subset_pdf.len();
    let full_len = full_pdf.len();
    assert!(
        full_len >= subset_len * 4,
        "full embedding must produce a materially larger PDF than \
             subset embedding (subset={subset_len} bytes, full={full_len} bytes)",
    );

    // None embedding strips the largest stream from the PDF, so it
    // must be smaller than the subset PDF.
    let none_len = none_pdf.len();
    assert!(
        none_len < subset_len,
        "none embedding must produce a smaller PDF than subset \
             embedding (none={none_len} bytes, subset={subset_len} bytes)",
    );
}
