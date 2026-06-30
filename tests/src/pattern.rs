mod shading {
    use krilla::num::NormalizedF32;
    use krilla::page::Page;
    use krilla::paint::{Fill, LinearGradient, RadialGradient, SpreadMethod, SweepGradient};
    use krilla::surface::Surface;
    use krilla_macros::{snapshot, visreg};

    use crate::{
        rect_to_path, stops_with_1_solid, stops_with_2_solid_1, stops_with_3_luma,
        stops_with_3_solid_1,
    };

    #[visreg(all)]
    fn pattern_linear_gradient_pad(surface: &mut Surface) {
        let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
        let gradient = LinearGradient {
            x1: 50.0,
            y1: 0.0,
            x2: 150.0,
            y2: 0.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            stops: stops_with_2_solid_1(),
            anti_alias: false,
        };

        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&path);
    }

    #[visreg(all)]
    fn pattern_linear_gradient_repeat(surface: &mut Surface) {
        let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
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

        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&path);
    }

    #[visreg(all)]
    fn pattern_sweep_gradient_pad(surface: &mut Surface) {
        let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
        let gradient = SweepGradient {
            cx: 100.0,
            cy: 100.0,
            start_angle: 0.0,
            end_angle: 90.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            stops: stops_with_2_solid_1(),
            anti_alias: false,
        };

        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&path);
    }

    #[visreg(all)]
    fn pattern_sweep_gradient_repeat(surface: &mut Surface) {
        let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
        let gradient = SweepGradient {
            cx: 100.0,
            cy: 100.0,
            start_angle: 0.0,
            end_angle: 90.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Repeat,
            stops: stops_with_2_solid_1(),
            anti_alias: false,
        };

        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&path);
    }

    #[visreg(all)]
    fn pattern_radial_gradient_pad(surface: &mut Surface) {
        let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
        let gradient = RadialGradient {
            cx: 100.0,
            cy: 100.0,
            cr: 30.0,
            fx: 120.0,
            fy: 120.0,
            fr: 60.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            stops: stops_with_3_solid_1(),
            anti_alias: false,
        };

        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&path);
    }

    // Should be turned into a solid color.
    #[snapshot]
    fn pattern_gradient_single_stop(page: &mut Page) {
        let mut surface = page.surface();

        let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
        let gradient = RadialGradient {
            cx: 100.0,
            cy: 100.0,
            cr: 30.0,
            fx: 120.0,
            fy: 120.0,
            fr: 60.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            stops: stops_with_1_solid(),
            anti_alias: false,
        };

        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&path);
    }

    #[snapshot]
    fn pattern_luma_stops(page: &mut Page) {
        let mut surface = page.surface();

        let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
        let gradient = RadialGradient {
            cx: 100.0,
            cy: 100.0,
            cr: 30.0,
            fx: 120.0,
            fy: 120.0,
            fr: 60.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            stops: stops_with_3_luma(),
            anti_alias: false,
        };

        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&path);
    }
}

mod tiling {
    use krilla::num::NormalizedF32;
    use krilla::paint::{Fill, Pattern};
    use krilla::surface::Surface;
    use krilla_macros::visreg;

    use crate::{basic_pattern_stream, rect_to_path};

    #[visreg(all)]
    fn pattern_tiling_basic(surface: &mut Surface) {
        let path = rect_to_path(20.0, 20.0, 180.0, 180.0);
        let stream_builder = surface.stream_builder();
        let pattern_stream = basic_pattern_stream(stream_builder);

        let pattern = Pattern {
            stream: pattern_stream,
            transform: Default::default(),
            width: 20.0,
            height: 20.0,
        };

        surface.set_fill(Some(Fill {
            paint: pattern.into(),
            opacity: NormalizedF32::new(0.5).unwrap(),
            rule: Default::default(),
        }));
        surface.draw_path(&path)
    }
}

#[cfg(test)]
mod gradient_robustness {
    use krilla::geom::PathBuilder;
    use krilla::num::NormalizedF32;
    use krilla::paint::{Fill, LinearGradient, RadialGradient, SpreadMethod, SweepGradient};
    use krilla::{Document, SerializeSettings};

    fn rect_path() -> krilla::geom::Path {
        let mut b = PathBuilder::new();
        b.move_to(10.0, 10.0);
        b.line_to(90.0, 10.0);
        b.line_to(90.0, 90.0);
        b.line_to(10.0, 90.0);
        b.close();
        b.finish().unwrap()
    }

    /// An empty `stops` vector on a gradient must not panic the serializer —
    /// the gradient is simply not emitted.
    #[test]
    fn linear_gradient_with_empty_stops_does_not_panic() {
        let mut document = Document::new_with(SerializeSettings::default());
        let mut page = document.start_page();
        let mut surface = page.surface();
        let gradient = LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            stops: Vec::new(),
            anti_alias: false,
        };
        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&rect_path());
        surface.finish();
        page.finish();
        document.finish().expect("serialisation must succeed");
    }

    #[test]
    fn radial_gradient_with_empty_stops_does_not_panic() {
        let mut document = Document::new_with(SerializeSettings::default());
        let mut page = document.start_page();
        let mut surface = page.surface();
        let gradient = RadialGradient {
            fx: 50.0,
            fy: 50.0,
            fr: 0.0,
            cx: 50.0,
            cy: 50.0,
            cr: 50.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            stops: Vec::new(),
            anti_alias: false,
        };
        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&rect_path());
        surface.finish();
        page.finish();
        document.finish().expect("serialisation must succeed");
    }

    #[test]
    fn sweep_gradient_with_empty_stops_does_not_panic() {
        let mut document = Document::new_with(SerializeSettings::default());
        let mut page = document.start_page();
        let mut surface = page.surface();
        let gradient = SweepGradient {
            cx: 50.0,
            cy: 50.0,
            start_angle: 0.0,
            end_angle: 360.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            stops: Vec::new(),
            anti_alias: false,
        };
        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&rect_path());
        surface.finish();
        page.finish();
        document.finish().expect("serialisation must succeed");
    }
}

/// Round-3 shading fixes: the output `/Range` of a Type 4 function now
/// tracks the real colour-space component count and bounds (rather than a
/// hard-coded three-component `[0 1 0 1 0 1]`), and a Type 3 stitching
/// function's `/Bounds` are strictly increasing even across coincident
/// stop offsets.
///
/// These are byte-assert `#[test]`s (mirroring `xref.rs`) rather than
/// snapshots: the `/Range` and `/Bounds` arrays live in the object
/// dictionaries, which are plain text under `settings_1` (pretty,
/// ASCII-compatible, uncompressed content streams), so a small windowed
/// scan can pin the exact values without a stored reference file.
#[cfg(test)]
mod shading_range {
    use krilla::color::devicen::{DeviceNSpace, TintTransform};
    use krilla::color::{cmyk, devicen, rgb, Color, LabParams, RegularColor};
    use krilla::num::NormalizedF32;
    use krilla::paint::{Fill, LinearGradient, SpreadMethod, Stop};
    use krilla::Document;

    use crate::{rect_to_path, settings_1};

    fn cmyk_stop(offset: f32, c: u8, m: u8, y: u8, k: u8) -> Stop {
        Stop {
            offset: NormalizedF32::new(offset).unwrap(),
            color: cmyk::Color::new(c, m, y, k).into(),
            opacity: NormalizedF32::ONE,
        }
    }

    fn rgb_stop(offset: f32, r: u8, g: u8, b: u8) -> Stop {
        Stop {
            offset: NormalizedF32::new(offset).unwrap(),
            color: rgb::Color::new(r, g, b).into(),
            opacity: NormalizedF32::ONE,
        }
    }

    /// Byte offset of the first `/FunctionType <n>` marker, panicking with
    /// a readable message if the PDF has none.
    fn function_type_pos(pdf: &[u8], function_type: u8) -> usize {
        let needle = format!("/FunctionType {function_type}");
        let bytes = needle.as_bytes();
        pdf.windows(bytes.len())
            .position(|w| w == bytes)
            .unwrap_or_else(|| panic!("no {needle} object in emitted PDF"))
    }

    /// Parse the numeric PDF array introduced by `key` at or after `from`:
    /// find `key`, then the following `[ … ]`, and parse its
    /// whitespace-separated numbers. `/Range` and `/Bounds` carry only
    /// numbers (no nested arrays), so the first `]` closes the array.
    /// pdf-writer renders integral floats as bare integers (`0`, `100`,
    /// `-100`) and the rest via `ryu` (`0.3`), both `f32`-parseable.
    fn numeric_array_after(pdf: &[u8], key: &[u8], from: usize) -> Vec<f32> {
        let key_at = from
            + pdf[from..]
                .windows(key.len())
                .position(|w| w == key)
                .unwrap_or_else(|| {
                    panic!(
                        "{} not found after offset {from}",
                        String::from_utf8_lossy(key)
                    )
                });
        let open = key_at
            + pdf[key_at..]
                .iter()
                .position(|&b| b == b'[')
                .expect("no '[' after key");
        let close = open
            + pdf[open..]
                .iter()
                .position(|&b| b == b']')
                .expect("no ']' closing the array");
        std::str::from_utf8(&pdf[open + 1..close])
            .expect("numeric array is valid UTF-8")
            .split_whitespace()
            .map(|token| {
                token
                    .parse::<f32>()
                    .unwrap_or_else(|_| panic!("non-numeric array element {token:?}"))
            })
            .collect()
    }

    fn assert_range_eq(actual: &[f32], expected: &[f32], label: &str) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{label}: /Range has {} elements, expected {} ({actual:?})",
            actual.len(),
            expected.len(),
        );
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (a - e).abs() < 1.0e-4,
                "{label}: /Range {actual:?} does not match expected {expected:?}",
            );
        }
    }

    fn assert_strictly_increasing(values: &[f32], label: &str) {
        assert!(
            !values.is_empty(),
            "{label}: expected a non-empty /Bounds array"
        );
        for pair in values.windows(2) {
            assert!(
                pair[0] < pair[1],
                "{label}: /Bounds not strictly increasing: {values:?}",
            );
        }
    }

    /// A repeating linear gradient serialises as a Type 4 PostScript
    /// shading whose `/Range` follows the stop colour space's component
    /// count. CMYK stops have four components, so the range is the
    /// eight-element `[0 1 0 1 0 1 0 1]` — not the old hard-coded
    /// three-component `[0 1 0 1 0 1]`.
    #[test]
    fn cmyk_postscript_shading_range_spans_four_components() {
        let mut document = Document::new_with(settings_1());
        let mut page = document.start_page();
        let mut surface = page.surface();

        let gradient = LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 200.0,
            y2: 0.0,
            transform: Default::default(),
            // Repeat forces the PostScript (Type 4) path; Pad would emit
            // an axial shading instead.
            spread_method: SpreadMethod::Repeat,
            stops: vec![cmyk_stop(0.0, 255, 0, 0, 0), cmyk_stop(1.0, 0, 255, 255, 0)],
            anti_alias: false,
        };
        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&rect_to_path(20.0, 20.0, 180.0, 180.0));
        surface.finish();
        page.finish();
        let pdf = document.finish().expect("serialisation must succeed");

        let ft4 = function_type_pos(&pdf, 4);
        let range = numeric_array_after(&pdf, b"/Range", ft4);
        assert_range_eq(
            &range,
            &[0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            "CMYK PostScript shading",
        );
    }

    /// A DeviceN space with N > 1 colorants emits its tint transform as a
    /// Type 4 PostScript function. With a Lab alternate the output
    /// `/Range` must span the Lab component extents
    /// `[0 100 -100 100 -100 100]` (ISO 32000-2 §8.6.5.4) — a hard-coded
    /// `[0 1 0 1 0 1]` would clip every Lab output to near-black.
    #[test]
    fn lab_devicen_tint_transform_range_reflects_lab_bounds() {
        let alternate = RegularColor::lab(
            LabParams {
                white_point: [0.9505, 1.0, 1.089],
                black_point: None,
                range: None,
            },
            [50.0, 30.0, -40.0],
        );
        let space = DeviceNSpace::new(
            vec!["SpotA".to_string(), "SpotB".to_string()],
            alternate,
            TintTransform::Linear {
                per_colorant_components: vec![vec![50.0, 30.0, -40.0], vec![80.0, -20.0, 10.0]],
            },
        )
        .expect("two-colorant DeviceN with a Lab alternate");
        let color: Color = devicen::Color::new(vec![1.0, 0.5], space)
            .expect("tint count matches colorant count")
            .into();

        let mut document = Document::new_with(settings_1());
        let mut page = document.start_page();
        let mut surface = page.surface();
        surface.set_fill(Some(Fill {
            paint: color.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&rect_to_path(20.0, 20.0, 180.0, 180.0));
        surface.finish();
        page.finish();
        let pdf = document.finish().expect("serialisation must succeed");

        let ft4 = function_type_pos(&pdf, 4);
        let range = numeric_array_after(&pdf, b"/Range", ft4);
        assert_range_eq(
            &range,
            &[0.0, 100.0, -100.0, 100.0, -100.0, 100.0],
            "Lab DeviceN tint transform",
        );
    }

    /// Three or more stops turn an axial (Pad) gradient into a Type 3
    /// stitching function. A CSS hard stop — here green then blue both at
    /// offset 0.3 — must not leak a duplicate `/Bounds` entry: the emitted
    /// `/Bounds` stay strictly increasing (ISO 32000-2 §7.10.4, Table 41).
    /// Before the fix this produced `[0.3 0.3]`; it must now be `[0.3 0.6]`.
    #[test]
    fn axial_gradient_stitching_bounds_strictly_increasing() {
        let mut document = Document::new_with(settings_1());
        let mut page = document.start_page();
        let mut surface = page.surface();

        let gradient = LinearGradient {
            x1: 20.0,
            y1: 0.0,
            x2: 180.0,
            y2: 0.0,
            transform: Default::default(),
            spread_method: SpreadMethod::Pad,
            // The pair at 0.3 carries a genuine colour change (green ->
            // blue), so it stays an exact hard stop rather than being
            // widened into a ramp — exercising the coincident-offset guard.
            stops: vec![
                rgb_stop(0.0, 255, 0, 0),
                rgb_stop(0.3, 0, 255, 0),
                rgb_stop(0.3, 0, 0, 255),
                rgb_stop(0.6, 255, 255, 0),
                rgb_stop(1.0, 0, 255, 255),
            ],
            anti_alias: false,
        };
        surface.set_fill(Some(Fill {
            paint: gradient.into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        surface.draw_path(&rect_to_path(20.0, 20.0, 180.0, 180.0));
        surface.finish();
        page.finish();
        let pdf = document.finish().expect("serialisation must succeed");

        let ft3 = function_type_pos(&pdf, 3);
        let bounds = numeric_array_after(&pdf, b"/Bounds", ft3);
        assert_strictly_increasing(&bounds, "axial stitching");
    }
}
