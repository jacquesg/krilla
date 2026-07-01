//! Content-stream-level integration tests for the
//! `rgb_grey_to_devicegray` and `preserve_black`
//! [`SerializeSettings`] fields wired through
//! `ContentBuilder`.
//!
//! Unlike the unit-test pairs in `krilla::graphics::color`, these
//! verify that the promotion / bypass actually executes during PDF
//! serialisation: an RGB grey under `rgb_grey_to_devicegray: true`
//! must emit a `g` operator (not `rg`), and a pure-black source
//! under `preserve_black: true` plus `no_device_cs: true` must
//! stay in its device space (no CIE reroute).

use krilla::color::{cmyk, rgb};
use krilla::geom::PathBuilder;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::{Document, SerializeSettings};

use crate::settings_1;

/// Build a `SerializeSettings` with the requested colour-options
/// flags applied on top of `settings_1` (uncompressed content stream,
/// device colour spaces enabled).
fn settings_with_options(rgb_grey_to_devicegray: bool, preserve_black: bool) -> SerializeSettings {
    SerializeSettings {
        rgb_grey_to_devicegray,
        preserve_black,
        ..settings_1()
    }
}

/// Build a `SerializeSettings` with `no_device_cs: true` so the
/// per-paint CIE reroute is active. Used to verify the
/// `preserve_black` bypass.
fn settings_with_icc_routing(preserve_black: bool) -> SerializeSettings {
    SerializeSettings {
        no_device_cs: true,
        preserve_black,
        ..settings_1()
    }
}

/// Render a single 100x100 filled rectangle with the supplied paint
/// and settings. Returns the raw PDF bytes (uncompressed content
/// stream).
fn render_filled_rect(settings: SerializeSettings, paint: krilla::paint::Paint) -> Vec<u8> {
    let mut document = Document::new_with(settings);
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

/// Locate a line in `haystack` ending in `op` (with `op` either at
/// the start of the line or preceded by whitespace). Returns the
/// full trimmed line on success and panics otherwise. Distinguishes
/// `g` from `rg` because `"rg".strip_suffix("g")` returns `"r"`,
/// which does not end in whitespace.
fn fill_operator_line(haystack: &[u8], op: &str) -> String {
    let text = String::from_utf8_lossy(haystack);
    for line in text.split('\n') {
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

// --- rgb_grey_to_devicegray --------------------------------------------

#[test]
fn rgb_grey_to_devicegray_promotes_when_set() {
    // RGB (128, 128, 128) under the promotion flag must emit as
    // DeviceGray. The exact float (128/255 ≈ 0.5019608) is decided
    // by pdf_writer's formatting; we assert the operator and that
    // the operand count is one.
    let pdf = render_filled_rect(
        settings_with_options(true, false),
        rgb::Color::new(128, 128, 128).into(),
    );
    let line = fill_operator_line(&pdf, "g");
    let parts: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(parts.len(), 2, "expected `<value> g`, got `{line}`");
    let value: f32 = parts[0].parse().expect("operand must parse as float");
    assert!(
        (value - 128.0 / 255.0).abs() < 0.001,
        "expected operand ~0.5020, got `{line}`",
    );
    assert_no_operator(&pdf, "rg");
}

#[test]
fn rgb_grey_passes_through_when_unset() {
    // Same colour, flag off: the existing `rg` emission stays.
    let pdf = render_filled_rect(
        settings_with_options(false, false),
        rgb::Color::new(128, 128, 128).into(),
    );
    let line = fill_operator_line(&pdf, "rg");
    let parts: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(parts.len(), 4, "expected `r g b rg`, got `{line}`");
    assert_eq!(parts[0], parts[1], "channels must match: {line}");
    assert_eq!(parts[1], parts[2], "channels must match: {line}");
    assert_no_operator(&pdf, "g");
}

#[test]
fn non_grey_rgb_unaffected_by_promotion() {
    // r != g != b must not be promoted, even with the flag on.
    let pdf = render_filled_rect(
        settings_with_options(true, false),
        rgb::Color::new(128, 64, 32).into(),
    );
    let line = fill_operator_line(&pdf, "rg");
    let parts: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(parts.len(), 4, "expected `r g b rg`, got `{line}`");
    assert_no_operator(&pdf, "g");
}

// --- preserve_black ----------------------------------------------------

#[test]
fn preserve_black_bypasses_icc_for_zero_rgb() {
    // `no_device_cs: true` triggers the CieBased reroute for RGB;
    // `preserve_black: true` must short-circuit it so `rgb(0, 0, 0)`
    // still emits `0 0 0 rg` in /DeviceRGB.
    let pdf = render_filled_rect(
        settings_with_icc_routing(true),
        rgb::Color::new(0, 0, 0).into(),
    );
    let line = fill_operator_line(&pdf, "rg");
    assert!(
        line.starts_with("0 0 0"),
        "expected `0 0 0 rg`, got `{line}`"
    );
}

#[test]
fn preserve_black_pass_through_for_non_black() {
    // Near-black RGB must NOT bypass: `preserve_black` is the
    // pure-black escape hatch only. With `no_device_cs: true` the
    // CieBased reroute applies, so the line emits via the
    // `Named` colour-space path (`SCN` / `scn`), not `rg`.
    let pdf = render_filled_rect(
        settings_with_icc_routing(true),
        rgb::Color::new(10, 10, 10).into(),
    );
    assert_no_operator(&pdf, "rg");
}

#[test]
fn preserve_black_handles_cmyk_pure_black() {
    // CMYK (0, 0, 0, 1) under `preserve_black: true` must emit
    // verbatim as `0 0 0 1 k` regardless of ICC routing.
    let pdf = render_filled_rect(
        settings_with_icc_routing(true),
        cmyk::Color::new(0, 0, 0, 255).into(),
    );
    let line = fill_operator_line(&pdf, "k");
    assert!(
        line.starts_with("0 0 0 1"),
        "expected `0 0 0 1 k`, got `{line}`"
    );
}
