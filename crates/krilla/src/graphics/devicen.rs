//! DeviceN colour space writer (ISO 32000-2 §8.6.6.5).
//!
//! Mirrors [`super::separation`]: the public user-facing types
//! (`Color`, `DeviceNSpace`, `TintTransform`) live in
//! [`crate::color::devicen`]; this module hosts the registration
//! writer that emits the
//! `[/DeviceN [<names>] <alt> <tint-fn> <attributes>]` array — the
//! trailing attributes dictionary carrying a `Colorants` dictionary of
//! per-colorant `Separation` spaces — into the chunk container.

use pdf_writer::types::PostScriptOp;
use pdf_writer::{Finish, Name, Ref};

use crate::chunk_container::ChunkContainer;
use crate::color::devicen::{DeviceNSpace, TintTransform};
use crate::color::{RegularColor, DEVICE_CMYK, DEVICE_GRAY, DEVICE_RGB};
use crate::configure::ValidationError;
use crate::resource::{self, Resource, Resourceable};
use crate::serialize::{Cacheable, MaybeDeviceColorSpace, SerializeContext};

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub(crate) struct DeviceNColorSpace {
    space: DeviceNSpace,
}

impl DeviceNColorSpace {
    pub fn new(space: DeviceNSpace) -> Self {
        Self { space }
    }
}

impl Cacheable for DeviceNColorSpace {
    fn serialize(
        self,
        sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    ) {
        // Register the alternate colour space (same dispatch as
        // Separation's writer uses).
        let alternate_cs = self.space.alternate().clone().color_space(sc);
        let alternate_cs_resource = sc.register_colorspace(chunk_container, alternate_cs.into());

        let num_alt_components =
            crate::color::devicen::alternate_channel_count(self.space.alternate());
        let colorant_count = self.space.colorant_count();

        let TintTransform::Linear {
            per_colorant_components,
        } = &self.space.tint_transform;
        // The DeviceNSpace constructor enforces these invariants.
        debug_assert_eq!(per_colorant_components.len(), colorant_count);

        // Output /Range and the tint = 0 (no ink) white anchor are
        // derived from the alternate space rather than hard-coded to
        // [0, 1] / 1.0. A Lab alternate has L* in [0, 100] and a*/b*
        // within its /Range (ISO 32000-2 §8.6.5.4), and §7.10.1 clips
        // every function output to /Range — a hard-coded [0, 1] range
        // would clip Lab outputs to garbage.
        let range = alternate_tint_range(self.space.alternate(), num_alt_components);
        let white = alternate_white_anchor(self.space.alternate(), num_alt_components);

        let tint_transform_ref = sc.new_ref();

        if colorant_count == 1 {
            // N = 1 — emit a single Type 2 (exponential) function,
            // identical in shape to a Separation tint transform.
            // PDF/A-friendly: no PostScript involved. Tint 0 anchors at
            // the alternate's white (`white`); tint 1 is the colorant's
            // full-tint appearance.
            let components = &per_colorant_components[0];
            let chunk = &mut chunk_container.non_stream.color_spaces;
            chunk
                .exponential_function(tint_transform_ref)
                .domain([0.0, 1.0])
                .range(range.iter().copied())
                .c0(white.iter().copied())
                .c1(components.clone())
                .n(1.0);
        } else {
            // N > 1 — the spec-correct linear blend
            //   out[m] = Σᵢ tintᵢ * componentsᵢ[m]
            // is only expressible as a Type 4 PostScript calculator
            // (Type 2 is single-input, Type 3 is single-input
            // stitching; neither can sum N inputs in M outputs).
            //
            // ISO 19005 (every PDF/A profile) forbids PostScript
            // functions; we surface that here via
            // `ValidationError::ContainsPostScript`. PDF/X-4 and PDF
            // 2.0 admit Type 4 and accept this output unchanged.
            sc.register_validation_error(ValidationError::ContainsPostScript(sc.location));

            let ops = build_linear_postscript(
                colorant_count,
                num_alt_components,
                per_colorant_components,
                &white,
                self.space.alternate().is_subtractive(),
            );
            let chunk = &mut chunk_container.non_stream.color_spaces;
            let encoded = PostScriptOp::encode(&ops);
            let mut function = chunk.post_script_function(tint_transform_ref, encoded.as_slice());
            function
                .domain([0.0, 1.0].repeat(colorant_count))
                .range(range.iter().copied());
        }

        // Per-colorant `Separation` tint transforms for the `Colorants`
        // dictionary. PDF/A-2/3/4 (ISO 19005-2 §6.2.4.4) and PDF/X-4+
        // require a `Colorants` entry for every spot colorant; ISO
        // 32000-2 §8.6.6.5 Table 70 defines each entry as a `Separation`
        // colour space describing that colorant alone. Each is a
        // single-colorant white-anchored Type 2 exponential (the same
        // shape as the N = 1 tint transform). The reserved process
        // names and `None` are not spot colorants and are skipped; for
        // N = 1 the DeviceN tint transform already *is* the sole
        // colorant's `Separation` tint transform, so it is reused.
        let mut colorant_entries: Vec<(usize, Ref)> = Vec::new();
        for (i, name) in self.space.colorants().iter().enumerate() {
            if !is_spot_colorant(name) {
                continue;
            }
            let sep_tint_ref = if colorant_count == 1 {
                tint_transform_ref
            } else {
                let sep_tint_ref = sc.new_ref();
                let chunk = &mut chunk_container.non_stream.color_spaces;
                chunk
                    .exponential_function(sep_tint_ref)
                    .domain([0.0, 1.0])
                    .range(range.iter().copied())
                    .c0(white.iter().copied())
                    .c1(per_colorant_components[i].clone())
                    .n(1.0);
                sep_tint_ref
            };
            colorant_entries.push((i, sep_tint_ref));
        }

        let chunk = &mut chunk_container.non_stream.color_spaces;

        // Now write the DeviceN colour-space array:
        //   [/DeviceN [<name1> <name2> ...] <alt-space> <tint-fn>
        //    <<attributes>>]
        // The trailing attributes dictionary is present only when the
        // space has spot colorants; it carries the `Colorants`
        // dictionary of per-colorant `Separation` spaces.
        let mut array = chunk.indirect(root_ref).array();
        array.item(Name(b"DeviceN"));

        // Colorant names sub-array.
        {
            let mut names = array.push().array();
            for name in self.space.colorants() {
                names.item(Name(name.as_bytes()));
            }
            names.finish();
        }

        // Alternate space — device name for the three process spaces,
        // indirect reference otherwise.
        write_alternate_space(&mut array, &alternate_cs_resource);

        array.item(tint_transform_ref);

        // Attributes dictionary carrying the `Colorants` sub-dictionary:
        // one `[/Separation name alt tintTransform]` per spot colorant.
        // The key matches the colorant name (ISO 32000-2 §8.6.6.5
        // Table 70).
        if !colorant_entries.is_empty() {
            let mut attributes = array.push().dict();
            let mut colorants = attributes.insert(Name(b"Colorants")).dict();
            for (i, sep_tint_ref) in &colorant_entries {
                let name = self.space.colorants()[*i].as_bytes();
                let mut separation = colorants.insert(Name(name)).array();
                separation.item(Name(b"Separation"));
                separation.item(Name(name));
                write_alternate_space(&mut separation, &alternate_cs_resource);
                separation.item(*sep_tint_ref);
                separation.finish();
            }
            colorants.finish();
            attributes.finish();
        }

        array.finish();
    }
}

impl Resourceable for DeviceNColorSpace {
    type Resource = resource::ColorSpace;
}

/// Whether `name` denotes a spot colorant that requires a `Colorants`
/// dictionary entry. The reserved process-colour names (`Cyan`,
/// `Magenta`, `Yellow`, `Black`) and `None` (which produces no marks)
/// are not spot colorants (ISO 32000-2 §8.6.6.5); every other name is.
fn is_spot_colorant(name: &str) -> bool {
    !matches!(name, "Cyan" | "Magenta" | "Yellow" | "Black" | "None")
}

/// The tint = 0 (no ink) white anchor in the alternate space, one value
/// per output channel. CMYK is subtractive, so no ink is all-zero; the
/// additive device and CIE RGB/grey spaces reach paper white at
/// all-ones; a Lab alternate is white at L* = 100, a* = b* = 0
/// (ISO 32000-2 §8.6.5.4). Used as the Type 2 exponential /C0 (both the
/// DeviceN tint transform and the per-colorant `Separation` spaces) and
/// as the per-channel anchor of the N > 1 PostScript blend.
fn alternate_white_anchor(alternate: &RegularColor, num_alt_components: usize) -> Vec<f32> {
    match alternate {
        RegularColor::Lab { .. } => vec![100.0, 0.0, 0.0],
        alt if alt.is_subtractive() => vec![0.0; num_alt_components],
        _ => vec![1.0; num_alt_components],
    }
}

/// The tint transform's output /Range: the alternate space's valid
/// component intervals. The additive/subtractive device and CIE RGB/grey
/// spaces are [0, 1] per channel; a Lab alternate is
/// `[0 100 a_min a_max b_min b_max]` (ISO 32000-2 §8.6.5.4; the a*/b*
/// range defaults to [-100, 100]). ISO 32000-2 §7.10.1 clips every
/// function output to /Range, so this must span the real output extent —
/// a hard-coded [0, 1] would clip Lab L*/a*/b* to garbage.
fn alternate_tint_range(alternate: &RegularColor, num_alt_components: usize) -> Vec<f32> {
    match alternate {
        RegularColor::Lab { params, .. } => {
            let [a_min, a_max, b_min, b_max] =
                params.range.unwrap_or([-100.0, 100.0, -100.0, 100.0]);
            vec![0.0, 100.0, a_min, a_max, b_min, b_max]
        }
        _ => [0.0, 1.0].repeat(num_alt_components),
    }
}

/// Emit the alternate colour space entry into `array` — a device colour
/// space name for the three process spaces, or the registered indirect
/// reference otherwise. Used for the DeviceN array's alternate space and
/// for each per-colorant `Separation` array in the `Colorants`
/// dictionary.
fn write_alternate_space(array: &mut pdf_writer::Array<'_>, alternate: &MaybeDeviceColorSpace) {
    match alternate {
        MaybeDeviceColorSpace::DeviceRgb => {
            array.item(Name(DEVICE_RGB.as_bytes()));
        }
        MaybeDeviceColorSpace::DeviceGray => {
            array.item(Name(DEVICE_GRAY.as_bytes()));
        }
        MaybeDeviceColorSpace::DeviceCMYK => {
            array.item(Name(DEVICE_CMYK.as_bytes()));
        }
        MaybeDeviceColorSpace::ColorSpace(cs) => {
            array.item(cs.get_ref());
        }
    }
}

/// Build the Type 4 PostScript program for the DeviceN tint transform.
///
/// For a subtractive alternate (CMYK) the blend is the superposition
///   out[m] = Σᵢ tintᵢ · per_colorant_components[i][m]
/// (anchored at tint 0 = alternate 0 = white). For an additive
/// alternate (RGB/Gray/Lab) each colorant instead subtracts from the
/// per-channel white `white[m]` (1.0 for device/CIE RGB and grey,
/// [100, 0, 0] for Lab):
///   out[m] = white[m] − Σᵢ tintᵢ · (white[m] − per_colorant_components[i][m])
/// (anchored at tint 0 = white), consistent with the Separation writer
/// and the N=1 exponential ramp. Out-of-range sums are clamped to the
/// alternate space's /Range by the function.
///
/// The PostScript stack on entry holds the *N* tints with
/// `tint_{N-1}` on top:
///   bottom -> tint_0 tint_1 ... tint_{N-1} <- top
///
/// For each output channel `m`:
///   1. push the per-colorant component using `index` to copy each
///      tint without consuming it;
///   2. multiply by that colorant's contribution to channel `m`;
///   3. sum the *N* products with N-1 `add`s.
///
/// After emitting all M outputs the original tints sit underneath M
/// output values. A single `roll` brings the tints to the top of the
/// stack and `M` `pop`s discard them, leaving the M outputs as the
/// function's return values.
// The indices `m` and `i` drive PostScript operand-stack depth
// arithmetic (`depth = (colorant_count - 1) + m`, see the inner note),
// and `m` selects a column across the per-colorant rows; neither is a
// plain sequential walk, so index-free iterators would obscure the
// stack layout.
#[allow(clippy::needless_range_loop)]
fn build_linear_postscript<'a>(
    colorant_count: usize,
    num_alt_components: usize,
    per_colorant_components: &[Vec<f32>],
    white: &[f32],
    subtractive: bool,
) -> Vec<PostScriptOp<'a>> {
    let mut ops: Vec<PostScriptOp<'a>> = Vec::new();
    for m in 0..num_alt_components {
        for i in 0..colorant_count {
            // At the start of channel `m`'s loop the stack is
            //   tint_0 tint_1 ... tint_{N-1} out_0 ... out_{m-1}
            // so tint_i sits at depth (N-1-i) + m. But this inner loop
            // has already pushed `i` partial products (one per prior
            // colorant) above the tints, shifting tint_i down by a
            // further `i`. Its live depth is therefore
            //   (N-1-i) + m + i = (N-1) + m
            // — constant across `i`. `index` copies from that depth.
            let depth = (colorant_count - 1) + m;
            ops.push(PostScriptOp::Integer(depth as i32));
            ops.push(PostScriptOp::Index);
            // Subtractive alternate (CMYK): out[m] = Σ tintᵢ·compᵢ[m],
            // anchored at 0 = white. Additive alternate (RGB/Gray/Lab):
            // each ink subtracts from the per-channel white white[m], so
            // multiply by (white[m] − comp) and finish with white[m] − Σ
            // below (anchored at white).
            let coeff = if subtractive {
                per_colorant_components[i][m]
            } else {
                white[m] - per_colorant_components[i][m]
            };
            ops.push(PostScriptOp::Real(coeff));
            ops.push(PostScriptOp::Mul);
        }
        // Sum the N products on the top of the stack via N-1 `add`s.
        for _ in 0..(colorant_count - 1) {
            ops.push(PostScriptOp::Add);
        }
        if !subtractive {
            // out[m] = white[m] − Σ tintᵢ·(white[m] − compᵢ[m]): negate
            // the sum, add the per-channel white white[m].
            ops.push(PostScriptOp::Neg);
            ops.push(PostScriptOp::Real(white[m]));
            ops.push(PostScriptOp::Add);
        }
    }
    // Stack is now `tint_0 .. tint_{N-1} out_0 .. out_{M-1}`. Roll
    // the (N+M) entries `M` times: each positive-`j` iteration of
    // `n j roll` moves the top element to the bottom of the n-element
    // window. After M iterations the M outputs are at the bottom and
    // the N tints sit on top. Pop the tints; the M outputs remain.
    let total = colorant_count + num_alt_components;
    ops.push(PostScriptOp::Integer(total as i32));
    ops.push(PostScriptOp::Integer(num_alt_components as i32));
    ops.push(PostScriptOp::Roll);
    for _ in 0..colorant_count {
        ops.push(PostScriptOp::Pop);
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::cmyk;

    #[test]
    fn linear_postscript_single_colorant_unused_in_writer() {
        // The N = 1 path emits a Type 2 exponential, not a Type 4 —
        // but the helper still produces a sane program when called
        // directly with N = 1 (no `roll` issued).
        let ops = build_linear_postscript(1, 3, &[vec![1.0, 0.5, 0.0]], &[0.0, 0.0, 0.0], true);
        // Should contain at least one Mul and end with a pop of the
        // single tint.
        assert!(ops.iter().any(|op| matches!(op, PostScriptOp::Mul)));
        assert!(matches!(ops.last(), Some(PostScriptOp::Pop)));
    }

    #[test]
    fn linear_postscript_two_colorant_cmyk_alt() {
        // 2 colorants, alt = CMYK (4 channels) — expect 4 sums.
        let per_colorant = vec![vec![0.0, 1.0, 1.0, 0.0], vec![1.0, 1.0, 0.0, 0.0]];
        let ops = build_linear_postscript(2, 4, &per_colorant, &[0.0, 0.0, 0.0, 0.0], true);
        let add_count = ops
            .iter()
            .filter(|op| matches!(op, PostScriptOp::Add))
            .count();
        // 4 output channels × (2 - 1) add per channel = 4 adds.
        assert_eq!(add_count, 4);
        let mul_count = ops
            .iter()
            .filter(|op| matches!(op, PostScriptOp::Mul))
            .count();
        // 4 channels × 2 colorants = 8 muls.
        assert_eq!(mul_count, 8);
        // Regression guard for the stack-depth arithmetic: the integer
        // pushed immediately before each `index` is the live depth of
        // the tint being copied, which must be (N-1)+m == 1+m for both
        // colorants of channel m — i.e. [1,1,2,2,3,3,4,4], never the
        // buggy `-i` form [1,0,2,1,3,2,4,3].
        let index_depths: Vec<i32> = ops
            .windows(2)
            .filter_map(|w| match (&w[0], &w[1]) {
                (PostScriptOp::Integer(d), PostScriptOp::Index) => Some(*d),
                _ => None,
            })
            .collect();
        assert_eq!(index_depths, vec![1, 1, 2, 2, 3, 3, 4, 4]);
    }

    #[test]
    fn linear_postscript_additive_alt_anchors_at_white() {
        // 2 colorants, additive (RGB) alternate: each channel multiplies
        // by (1 - comp) and finishes with `neg 1 add`, giving
        // out[m] = 1 - Σ tintᵢ·(1-compᵢ[m]) — anchored at white.
        let per_colorant = vec![vec![1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0]];
        let ops = build_linear_postscript(2, 3, &per_colorant, &[1.0, 1.0, 1.0], false);
        // One `neg` per output channel (3).
        let neg_count = ops
            .iter()
            .filter(|op| matches!(op, PostScriptOp::Neg))
            .count();
        assert_eq!(neg_count, 3);
        // Channel 0's two multiplier constants are (1 - comp):
        // 1-1.0=0.0 (colorant 0) and 1-0.0=1.0 (colorant 1).
        let reals: Vec<f32> = ops
            .iter()
            .filter_map(|op| match op {
                PostScriptOp::Real(r) => Some(*r),
                _ => None,
            })
            .collect();
        assert_eq!(reals[0], 0.0);
        assert_eq!(reals[1], 1.0);
    }

    #[test]
    fn devicen_color_space_constructs_one_colorant() {
        use crate::color::devicen::{DeviceNSpace, TintTransform};
        let alt: crate::color::RegularColor = cmyk::Color::new(0, 0, 0, 0).into();
        let space = DeviceNSpace::new(
            vec!["PANTONE 185 C".to_string()],
            alt,
            TintTransform::Linear {
                per_colorant_components: vec![vec![0.0, 1.0, 1.0, 0.0]],
            },
        )
        .expect("one-colorant DeviceN should construct");
        let cs = DeviceNColorSpace::new(space);
        assert_eq!(cs.space.colorant_count(), 1);
    }

    #[test]
    fn spot_colorant_excludes_process_and_none() {
        // Only genuine spot colorants earn a `Colorants` entry; the
        // reserved process names and `None` are excluded.
        assert!(is_spot_colorant("PANTONE 185 C"));
        assert!(!is_spot_colorant("Cyan"));
        assert!(!is_spot_colorant("Magenta"));
        assert!(!is_spot_colorant("Yellow"));
        assert!(!is_spot_colorant("Black"));
        assert!(!is_spot_colorant("None"));
    }

    #[test]
    fn lab_alternate_range_and_white_anchor_track_the_lab_space() {
        use crate::color::{LabParams, RegularColor};
        // Default a*/b* range [-100, 100]: /Range spans L* [0, 100] and
        // a*/b* [-100, 100]; the white anchor is Lab white L* = 100,
        // a* = b* = 0 — not the [0, 1] / 1.0 that would clip Lab output.
        let lab = RegularColor::lab(
            LabParams {
                white_point: [0.9505, 1.0, 1.089],
                black_point: None,
                range: None,
            },
            [50.0, 30.0, -40.0],
        );
        assert_eq!(
            alternate_tint_range(&lab, 3),
            vec![0.0, 100.0, -100.0, 100.0, -100.0, 100.0]
        );
        assert_eq!(alternate_white_anchor(&lab, 3), vec![100.0, 0.0, 0.0]);

        // A custom a*/b* range flows into the emitted /Range verbatim.
        let lab_custom = RegularColor::lab(
            LabParams {
                white_point: [0.9505, 1.0, 1.089],
                black_point: None,
                range: Some([-128.0, 127.0, -128.0, 127.0]),
            },
            [50.0, 30.0, -40.0],
        );
        assert_eq!(
            alternate_tint_range(&lab_custom, 3),
            vec![0.0, 100.0, -128.0, 127.0, -128.0, 127.0]
        );

        // CMYK (subtractive) anchors at all-zero, RGB (additive) at
        // all-ones; both clip to [0, 1] per channel.
        let cmyk_alt: RegularColor = cmyk::Color::new(0, 0, 0, 0).into();
        assert_eq!(alternate_white_anchor(&cmyk_alt, 4), vec![0.0; 4]);
        assert_eq!(
            alternate_tint_range(&cmyk_alt, 4),
            vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]
        );
        let rgb_alt: RegularColor = crate::color::rgb::Color::new(0, 0, 0).into();
        assert_eq!(alternate_white_anchor(&rgb_alt, 3), vec![1.0; 3]);
        assert_eq!(
            alternate_tint_range(&rgb_alt, 3),
            vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0]
        );
    }

    #[test]
    fn linear_postscript_lab_alt_anchors_at_lab_white() {
        // Additive Lab alternate: the per-channel white anchor is
        // [100, 0, 0], so each output channel finishes with
        // `neg <white[m]> add`. The Real immediately after each Neg is
        // that channel's white anchor.
        let per_colorant = vec![vec![50.0, 30.0, -40.0], vec![80.0, -20.0, 10.0]];
        let ops = build_linear_postscript(2, 3, &per_colorant, &[100.0, 0.0, 0.0], false);
        let anchors: Vec<f32> = ops
            .windows(2)
            .filter_map(|w| match (&w[0], &w[1]) {
                (PostScriptOp::Neg, PostScriptOp::Real(r)) => Some(*r),
                _ => None,
            })
            .collect();
        assert_eq!(anchors, vec![100.0, 0.0, 0.0]);
        // Channel 0 (L*) coefficients are white[0] − comp: 100 − 50 and
        // 100 − 80.
        let first_reals: Vec<f32> = ops
            .iter()
            .filter_map(|op| match op {
                PostScriptOp::Real(r) => Some(*r),
                _ => None,
            })
            .take(2)
            .collect();
        assert_eq!(first_reals, vec![50.0, 20.0]);
    }
}
