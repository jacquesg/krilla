//! Content-stream-level integration tests for the
//! [`ColourConversion`] dispatch wired into
//! [`crate::content::ContentBuilder`].
//!
//! Unlike the unit tests in `krilla::graphics::color::tests`, these
//! verify that the projection actually executes during PDF
//! serialisation: a `ForceCmyk` policy with an RGB source must emit
//! a `k` operator (CMYK fill) into the content stream, not an `rg`
//! operator. The 3x3 matrix below covers every `(policy, source)`
//! pair the design audit called out.

use krilla::color::{cmyk, luma, rgb, ColourConversion};
use krilla::geom::PathBuilder;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::{Document, SerializeSettings};

use crate::settings_1;

/// Build a `SerializeSettings` that emits an *uncompressed* content
/// stream (so we can grep for fill operators) with the requested
/// projection policy.
fn settings_with_policy(policy: ColourConversion) -> SerializeSettings {
    SerializeSettings {
        colour_conversion: policy,
        ..settings_1()
    }
}

/// Render a single 100x100 filled rectangle with the supplied paint
/// and projection policy. Returns the raw PDF bytes (uncompressed
/// content stream).
fn render_filled_rect(policy: ColourConversion, paint: krilla::paint::Paint) -> Vec<u8> {
    let mut document = Document::new_with(settings_with_policy(policy));
    let page_settings = PageSettings::from_wh(200.0, 200.0).unwrap();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();
    let mut b = PathBuilder::new();
    b.move_to(20.0, 20.0);
    b.line_to(120.0, 20.0);
    b.line_to(120.0, 120.0);
    b.line_to(20.0, 120.0);
    b.close();
    let path = b.finish().unwrap();
    surface.set_fill(Some(Fill {
        paint,
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    }));
    surface.draw_path(&path);
    surface.finish();
    page.finish();
    document.finish().expect("serialisation must succeed")
}

/// Locate the n-th occurrence of `needle` (counting whole-word
/// matches preceded by whitespace) in `haystack` and return the
/// byte slice of the operands that precede it on the same line.
///
/// Used to assert specific operator-and-operand sequences such as
/// `1 0 0 rg` or `0 1 1 0 k`.
fn fill_operator_line(haystack: &[u8], op: &str) -> String {
    let text = String::from_utf8_lossy(haystack);
    for line in text.split('\n') {
        // The content stream emits each `set_fill_*` call on its
        // own line followed by the operator name.
        let trimmed = line.trim();
        if trimmed.ends_with(op)
            && trimmed
                .strip_suffix(op)
                .map(|prefix| prefix.ends_with(' ') || prefix.is_empty())
                .unwrap_or(false)
        {
            return trimmed.to_string();
        }
    }
    panic!(
        "operator `{op}` not found in content stream:\n{}",
        text.chars().take(2000).collect::<String>()
    );
}

fn assert_no_operator(haystack: &[u8], op: &str) {
    let text = String::from_utf8_lossy(haystack);
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.ends_with(op)
            && trimmed
                .strip_suffix(op)
                .map(|prefix| prefix.ends_with(' ') || prefix.is_empty())
                .unwrap_or(false)
        {
            panic!("operator `{op}` unexpectedly emitted on line `{trimmed}` in content stream");
        }
    }
}

// --- 3x3 matrix: policy in {ForceRgb, ForceCmyk, ForceGrey}
//                source in {RGB red, CMYK red, Luma 50%} ---------------

#[test]
fn force_rgb_from_rgb_emits_rg() {
    let pdf = render_filled_rect(
        ColourConversion::ForceRgb,
        rgb::Color::new(255, 0, 0).into(),
    );
    let line = fill_operator_line(&pdf, "rg");
    assert!(
        line.starts_with("1 0 0"),
        "expected `1 0 0 rg`, got `{line}`"
    );
}

#[test]
fn force_rgb_from_cmyk_emits_rg() {
    // CMYK red = (0, 1, 1, 0) -> RGB (1, 0, 0).
    let pdf = render_filled_rect(
        ColourConversion::ForceRgb,
        cmyk::Color::new(0, 255, 255, 0).into(),
    );
    let line = fill_operator_line(&pdf, "rg");
    assert!(
        line.starts_with("1 0 0"),
        "expected `1 0 0 rg`, got `{line}`"
    );
    // The CMYK source must NOT survive — no `k` operator.
    assert_no_operator(&pdf, "k");
}

#[test]
fn force_rgb_from_luma_emits_rg() {
    // L = 128 -> RGB (128, 128, 128) -> 128/255 ~= 0.5020 (rounded
    // by the PDF writer; we just assert the operator + that all
    // three channels are equal).
    let pdf = render_filled_rect(ColourConversion::ForceRgb, luma::Color::new(128).into());
    let line = fill_operator_line(&pdf, "rg");
    let parts: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(parts.len(), 4, "expected `r g b rg`, got `{line}`");
    assert_eq!(parts[0], parts[1], "channels must match: {line}");
    assert_eq!(parts[1], parts[2], "channels must match: {line}");
    assert_no_operator(&pdf, "g");
}

#[test]
fn force_cmyk_from_rgb_emits_k() {
    // RGB (255, 0, 0) -> CMYK (0, 1, 1, 0).
    let pdf = render_filled_rect(
        ColourConversion::ForceCmyk,
        rgb::Color::new(255, 0, 0).into(),
    );
    let line = fill_operator_line(&pdf, "k");
    assert!(
        line.starts_with("0 1 1 0"),
        "expected `0 1 1 0 k`, got `{line}`"
    );
    assert_no_operator(&pdf, "rg");
}

#[test]
fn force_cmyk_from_cmyk_emits_k() {
    let pdf = render_filled_rect(
        ColourConversion::ForceCmyk,
        cmyk::Color::new(0, 255, 255, 0).into(),
    );
    let line = fill_operator_line(&pdf, "k");
    assert!(
        line.starts_with("0 1 1 0"),
        "expected `0 1 1 0 k`, got `{line}`"
    );
}

#[test]
fn force_cmyk_from_luma_emits_k_pure_black() {
    // L = 128 -> K = 1 - 128/255 ~= 0.498. Channel-wise the
    // emitted CMYK is (0, 0, 0, k).
    let pdf = render_filled_rect(ColourConversion::ForceCmyk, luma::Color::new(128).into());
    let line = fill_operator_line(&pdf, "k");
    let parts: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(parts.len(), 5, "expected `c m y k k`, got `{line}`");
    assert_eq!(parts[0], "0", "C must be 0: {line}");
    assert_eq!(parts[1], "0", "M must be 0: {line}");
    assert_eq!(parts[2], "0", "Y must be 0: {line}");
    assert_no_operator(&pdf, "g");
    assert_no_operator(&pdf, "rg");
}

#[test]
fn force_grey_from_rgb_emits_g() {
    // RGB (255, 0, 0) -> Y ~= 54/255 (Rec. 709). Just assert the
    // operator switches to `g`.
    let pdf = render_filled_rect(
        ColourConversion::ForceGrey,
        rgb::Color::new(255, 0, 0).into(),
    );
    fill_operator_line(&pdf, "g");
    assert_no_operator(&pdf, "rg");
    assert_no_operator(&pdf, "k");
}

#[test]
fn force_grey_from_cmyk_emits_g() {
    let pdf = render_filled_rect(
        ColourConversion::ForceGrey,
        cmyk::Color::new(0, 255, 255, 0).into(),
    );
    fill_operator_line(&pdf, "g");
    assert_no_operator(&pdf, "k");
}

#[test]
fn force_grey_from_luma_emits_g() {
    let pdf = render_filled_rect(ColourConversion::ForceGrey, luma::Color::new(128).into());
    fill_operator_line(&pdf, "g");
}

// --- Auto baseline -----------------------------------------------------

#[test]
fn auto_policy_is_pass_through_rgb() {
    // Without projection an RGB source must emit `rg`.
    let pdf = render_filled_rect(ColourConversion::Auto, rgb::Color::new(255, 0, 0).into());
    fill_operator_line(&pdf, "rg");
    assert_no_operator(&pdf, "k");
    assert_no_operator(&pdf, "g");
}

#[test]
fn auto_policy_is_pass_through_cmyk() {
    let pdf = render_filled_rect(
        ColourConversion::Auto,
        cmyk::Color::new(50, 100, 150, 200).into(),
    );
    fill_operator_line(&pdf, "k");
    assert_no_operator(&pdf, "rg");
}
