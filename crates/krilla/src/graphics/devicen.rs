//! DeviceN colour space writer (ISO 32000-2 §8.6.6.5).
//!
//! Mirrors [`super::separation`]: the public user-facing types
//! (`Color`, `DeviceNSpace`, `TintTransform`) live in
//! [`crate::color::devicen`]; this module hosts the registration
//! writer that emits the
//! `[/DeviceN [<names>] <alt> <tint-fn>]` array into the chunk
//! container.

use pdf_writer::types::PostScriptOp;
use pdf_writer::{Finish, Name, Ref};

use crate::chunk_container::ChunkContainer;
use crate::color::devicen::{DeviceNSpace, TintTransform};
use crate::color::{DEVICE_CMYK, DEVICE_GRAY, DEVICE_RGB};
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

        if let Err(validation_error) = sc.validation_store().validate_devicen(&self.space) {
            sc.register_validation_error(validation_error);
        }

        let TintTransform::Linear {
            per_colorant_components,
        } = &self.space.tint_transform;
        // The DeviceNSpace constructor enforces these invariants.
        debug_assert_eq!(per_colorant_components.len(), colorant_count);

        let tint_transform_ref = sc.new_ref();

        if colorant_count == 1 {
            // N = 1 — emit a single Type 2 (exponential) function,
            // identical in shape to a Separation tint transform.
            // PDF/A-friendly: no PostScript involved.
            let components = &per_colorant_components[0];
            let chunk = &mut chunk_container.non_stream.color_spaces;
            chunk
                .exponential_function(tint_transform_ref)
                .domain([0.0, 1.0])
                .range([0.0, 1.0].repeat(num_alt_components))
                .c0(vec![0.0; num_alt_components])
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
            );
            let chunk = &mut chunk_container.non_stream.color_spaces;
            let encoded = PostScriptOp::encode(&ops);
            let mut function = chunk.post_script_function(tint_transform_ref, encoded.as_slice());
            function
                .domain([0.0, 1.0].repeat(colorant_count))
                .range([0.0, 1.0].repeat(num_alt_components));
        }

        let chunk = &mut chunk_container.non_stream.color_spaces;

        // Now write the DeviceN colour-space array:
        //   [/DeviceN [<name1> <name2> ...] <alt-space> <tint-fn>]
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
        match alternate_cs_resource {
            MaybeDeviceColorSpace::DeviceRgb => array.item(Name(DEVICE_RGB.as_bytes())),
            MaybeDeviceColorSpace::DeviceGray => array.item(Name(DEVICE_GRAY.as_bytes())),
            MaybeDeviceColorSpace::DeviceCMYK => array.item(Name(DEVICE_CMYK.as_bytes())),
            MaybeDeviceColorSpace::ColorSpace(cs) => array.item(cs.get_ref()),
        };

        array.item(tint_transform_ref);

        array.finish();
    }
}

impl Resourceable for DeviceNColorSpace {
    type Resource = resource::ColorSpace;
}

/// Build the Type 4 PostScript program implementing
///   out[m] = Σᵢ tintᵢ * per_colorant_components[i][m]
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
// arithmetic (`depth = (colorant_count - 1 - i) + m`), and `m` selects
// a column across the per-colorant rows; neither is a plain sequential
// walk, so index-free iterators would obscure the stack layout.
#[allow(clippy::needless_range_loop)]
fn build_linear_postscript<'a>(
    colorant_count: usize,
    num_alt_components: usize,
    per_colorant_components: &[Vec<f32>],
) -> Vec<PostScriptOp<'a>> {
    let mut ops: Vec<PostScriptOp<'a>> = Vec::new();
    for m in 0..num_alt_components {
        for i in 0..colorant_count {
            // Stack layout at the start of channel `m`'s loop:
            //   tint_0 tint_1 ... tint_{N-1} out_0 ... out_{m-1}
            // (`out_0..out_{m-1}` are `m` results, total depth so far
            // is `colorant_count + m`.) `index` takes the depth from
            // the top of the operand stack — tint_i sits at depth
            // (colorant_count - 1 - i) + m.
            let depth = (colorant_count - 1 - i) + m;
            ops.push(PostScriptOp::Integer(depth as i32));
            ops.push(PostScriptOp::Index);
            ops.push(PostScriptOp::Real(per_colorant_components[i][m]));
            ops.push(PostScriptOp::Mul);
        }
        // Sum the N products on the top of the stack via N-1 `add`s.
        for _ in 0..(colorant_count - 1) {
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
        let ops = build_linear_postscript(1, 3, &[vec![1.0, 0.5, 0.0]]);
        // Should contain at least one Mul and end with a pop of the
        // single tint.
        assert!(ops.iter().any(|op| matches!(op, PostScriptOp::Mul)));
        assert!(matches!(ops.last(), Some(PostScriptOp::Pop)));
    }

    #[test]
    fn linear_postscript_two_colorant_cmyk_alt() {
        // 2 colorants, alt = CMYK (4 channels) — expect 4 sums.
        let per_colorant = vec![vec![0.0, 1.0, 1.0, 0.0], vec![1.0, 1.0, 0.0, 0.0]];
        let ops = build_linear_postscript(2, 4, &per_colorant);
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
}
