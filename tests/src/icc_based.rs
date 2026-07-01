//! Content-stream-level integration tests for the new
//! [`RegularColor::IccBased`] wide-gamut path (CSS Color 5
//! `color(display-p3 …)`, `color(rec2020 …)`, etc).
//!
//! These verify that:
//!   1. An ICC-based paint emits `/CS<n> cs <c0> <c1> <c2> scn` into
//!      the content stream (ISO 32000-2 §8.6.5.5) instead of a
//!      DeviceRGB `rg` operator.
//!   2. The page `/Resources /ColorSpace` dictionary gains a
//!      `/CS<n> [/ICCBased <stream>]` entry referencing the embedded
//!      ICC profile stream.
//!   3. Two paints sharing the same `ICCProfile<3>` reuse the same
//!      `/CS<n>` resource entry (Arc-identity content-hash dedup at
//!      the `register_resourceable` layer).
//!   4. Two paints with different `ICCProfile<3>` instances produce
//!      distinct resource entries.
//!
//! The test fixture uses the public-domain `sRGB-v4.icc` profile
//! shipped under `crates/krilla/icc/` because (a) it is a valid
//! three-component ICC profile, (b) it is the smallest one to hand,
//! and (c) the assertions verify routing shape, not gamut accuracy.

use krilla::color::{rgb, RegularColor};
use krilla::geom::PathBuilder;
use krilla::icc::ICCProfile;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::{Document, SerializeSettings};

use crate::settings_1;

const SRGB_V4_BYTES: &[u8] = include_bytes!("../../crates/krilla/icc/sRGB-v4.icc");

/// A second three-component ICC profile distinct from `SRGB_V4_BYTES`.
/// Used to verify that two different profiles produce two different
/// `/CS<n>` resource entries (not deduplicated to one).
const SRGB_V2_BYTES: &[u8] = include_bytes!("../../crates/krilla/icc/sRGB-v2-magic.icc");

fn build_profile(bytes: &[u8]) -> ICCProfile<3> {
    ICCProfile::<3>::new(bytes).expect("three-channel ICC profile must parse")
}

fn icc_fill(profile: ICCProfile<3>, components: [f32; 3]) -> Fill {
    let color: krilla::color::Color = RegularColor::icc_based(profile, components).into();
    Fill {
        paint: color.into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    }
}

/// Render `paints.len()` filled rectangles, each using its own
/// `Fill` so distinct paints land in the content stream consecutively.
/// Returns the raw PDF bytes (uncompressed content stream).
fn render_filled_rects(settings: SerializeSettings, paints: Vec<Fill>) -> Vec<u8> {
    let mut document = Document::new_with(settings);
    let page_settings = PageSettings::from_wh(200.0, 200.0).unwrap();
    let mut page = document.start_page_with(page_settings);
    let mut surface = page.surface();
    for fill in paints {
        let mut b = PathBuilder::new();
        b.move_to(20.0, 20.0);
        b.line_to(120.0, 20.0);
        b.line_to(120.0, 120.0);
        b.line_to(20.0, 120.0);
        b.close();
        let path = b.finish().unwrap();
        surface.set_fill(Some(fill));
        surface.draw_path(&path);
    }
    surface.finish();
    page.finish();
    document.finish().expect("serialisation must succeed")
}

fn pdf_text(pdf: &[u8]) -> String {
    String::from_utf8_lossy(pdf).into_owned()
}

#[test]
fn icc_based_emits_named_color_space() {
    let profile = build_profile(SRGB_V4_BYTES);
    let fill = icc_fill(profile, [1.0, 0.0, 0.0]);
    let pdf = render_filled_rects(settings_1(), vec![fill]);
    let text = pdf_text(&pdf);

    // The content stream must NOT emit a DeviceRGB `rg` operator —
    // that would mean the wide-gamut path collapsed to sRGB.
    assert!(
        !text.lines().any(|line| line.trim().ends_with(" rg")),
        "unexpected DeviceRGB `rg` operator in content stream:\n{}",
        text.chars().take(800).collect::<String>()
    );

    // The content stream MUST set a named colour space (`/CS<n> cs`)
    // followed by the three source components and the `scn` operator.
    assert!(
        text.lines().any(|line| line.trim().ends_with(" cs")),
        "missing `cs` colour-space operator in content stream:\n{}",
        text.chars().take(800).collect::<String>()
    );
    assert!(
        text.lines().any(|line| line.trim().ends_with(" scn")),
        "missing `scn` colour-set operator in content stream:\n{}",
        text.chars().take(800).collect::<String>()
    );
}

#[test]
fn icc_based_registers_iccbased_color_space_resource() {
    let profile = build_profile(SRGB_V4_BYTES);
    let fill = icc_fill(profile, [0.5, 0.25, 0.125]);
    let pdf = render_filled_rects(settings_1(), vec![fill]);
    let text = pdf_text(&pdf);

    // The serialised PDF must contain an `/ICCBased` colour-space
    // resource — the per-profile array `[/ICCBased <stream>]` that
    // `ICCBasedColorSpace::serialize` writes via the
    // `register_colorspace` -> `register_resourceable` chain.
    assert!(
        text.contains("/ICCBased"),
        "missing `/ICCBased` colour-space array in PDF:\n{}",
        text.chars().take(1200).collect::<String>()
    );
}

#[test]
fn icc_based_three_components_emitted() {
    let profile = build_profile(SRGB_V4_BYTES);
    let fill = icc_fill(profile, [0.5, 0.25, 0.125]);
    let pdf = render_filled_rects(settings_1(), vec![fill]);
    let text = pdf_text(&pdf);

    // Find the `scn` operator line and verify three operands precede
    // it (the source-space components in [0.0, 1.0]).
    let scn_line = text
        .lines()
        .find(|line| line.trim().ends_with(" scn"))
        .expect("missing scn operator");
    let prefix = scn_line
        .trim()
        .strip_suffix(" scn")
        .expect("scn line must split");
    let operands: Vec<&str> = prefix.split_whitespace().collect();
    assert_eq!(
        operands.len(),
        3,
        "expected three operands before scn, got {:?} on line `{}`",
        operands,
        scn_line.trim()
    );
}

#[test]
fn icc_based_same_profile_deduplicates_resource() {
    // Two paints sharing the same `ICCProfile<3>` Arc must share a
    // single `/CS<n>` resource entry. The first paint's components
    // (1,0,0) and the second's (0,1,0) verify that the dedup is on
    // profile identity, not on the colour value.
    let profile = build_profile(SRGB_V4_BYTES);
    let fill_red = icc_fill(profile.clone(), [1.0, 0.0, 0.0]);
    let fill_green = icc_fill(profile, [0.0, 1.0, 0.0]);
    let pdf = render_filled_rects(settings_1(), vec![fill_red, fill_green]);
    let text = pdf_text(&pdf);

    // Exactly one `/ICCBased` entry across the document.
    let count = text.matches("/ICCBased").count();
    assert_eq!(
        count,
        1,
        "two paints sharing one profile must produce exactly one /ICCBased \
         resource (got {count}); PDF:\n{}",
        text.chars().take(1500).collect::<String>()
    );
}

#[test]
fn icc_based_different_profiles_distinct_resources() {
    // Two paints with two different profiles must produce two
    // separate `/CS<n>` resource entries — dedup keys on profile
    // content hash, not on the source-stream byte layout of the
    // colour value.
    let p1 = build_profile(SRGB_V4_BYTES);
    let p2 = build_profile(SRGB_V2_BYTES);
    let fill1 = icc_fill(p1, [1.0, 0.0, 0.0]);
    let fill2 = icc_fill(p2, [0.0, 1.0, 0.0]);
    let pdf = render_filled_rects(settings_1(), vec![fill1, fill2]);
    let text = pdf_text(&pdf);

    let count = text.matches("/ICCBased").count();
    assert_eq!(
        count, 2,
        "two distinct profiles must produce two /ICCBased resources (got {count})"
    );
}

#[test]
fn icc_based_coexists_with_srgb_paint() {
    // Regression guard: emitting an ICC-based paint must not affect
    // adjacent sRGB paints, which still go through the DeviceRGB
    // `rg` operator.
    let profile = build_profile(SRGB_V4_BYTES);
    let icc = icc_fill(profile, [1.0, 0.0, 0.0]);
    let plain = Fill {
        paint: rgb::Color::new(255, 255, 0).into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    };
    let pdf = render_filled_rects(settings_1(), vec![icc, plain]);
    let text = pdf_text(&pdf);

    // The sRGB paint must still emit `rg`.
    assert!(
        text.lines().any(|line| line.trim().ends_with(" rg")),
        "sRGB paint after an ICC-based paint must still emit `rg`:\n{}",
        text.chars().take(1500).collect::<String>()
    );
    // The ICC paint must still emit `cs` + `scn`.
    assert!(
        text.lines().any(|line| line.trim().ends_with(" cs")),
        "ICC-based paint must emit `cs`:\n{}",
        text.chars().take(1500).collect::<String>()
    );
}
