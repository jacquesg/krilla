//! Dealing with colors and color spaces.
//!
//! # Color spaces
//!
//! krilla currently supports four color models:
//! - RGB
//! - Luma
//! - CMYK
//! - Separation (also known as Spot)
//!
//! Each color space is associated with its specific color type, which you can use to create new
//! instances of a specific color in that color space.
//!
//! # Representation of colors
//!
//! When specifying colors in the process color spaces RGB, Luma, and CMYK, it is important
//! to understand the distinction between device-dependent and decide-independent color
//! specification. What follows is only a very brief explanation, if you want to dive into
//! more details, please look for appropriate resources on the web.
//!
//! When specifying colors in a *device-dependent way*, if I instruct the program to draw
//! the RGB color (145, 120, 45), then the program will use these literal values to activate
//! the R/G/B lights to achieve displaying a certain color. The problem is that specifying
//! colors in such a way can lead to slightly different results when actually displaying it,
//! depending on the screen that is used, since each screen is calibrated differently and
//! based on different display technologies. This is especially critical for printers, where
//! different values for CMYK colors might result in different-looking colors when being printed.
//!
//! This is why there is also the option to specify colors in a *device-independent* way,
//! which basically means that the color value (145, 120, 45) is represented in a well-specified
//! color space, and each device can then convert the colors to their native color space
//! so that they match the representation in the given color space as closely as possible.
//! This should lead to a more accurate color representation across different screens.
//!
//! In 90% of the cases, it is totally fine to just use a device-dependent colorspace, and it's
//! what krilla does by default. However, if you do care about that, then you can set the
//! `no_device_cs` property of [`SerializeSettings`] to true, in which case krilla will embed an ICC profile for the
//! sGrey and sRGB color spaces (for Luma and RGB colors, respectively). If a CMYK profile
//! was provided to the serialize settings, this will be used for CMYK colors. Otherwise,
//! it will fall back to device CMYK.
//!
//! # Separations
//!
//! An alternative way to achieve exact color reproduction in print are Separation colors.
//! Using these colors, you can request a specific colorant through its well-known name
//! (e.g. through a color registry such as PANTONE or RAL). Your production shop can then
//! supply that exact pigment.
//!
//! It is important to recognize that, when viewed on a computer or printed on a home printer,
//! the requested colorant won't be available. For that reason, you will need to supply a fallback
//! process color.
//!
//! Pay attention to remain consistent: Both in your use of colorant names and fallback colors. A
//! given colorant should not be referred to by multiple, slightly different names. Just the same
//! for fallback colors: For a given colorant, there should be only one fallback color. Using multiple
//! Separation color spaces with the same colorant and distinct fallback colors is considered a bad
//! practice and forbidden in PDF/A-2 and later.
//!
//! [`SerializeSettings`]: crate::SerializeSettings

use std::fmt::Debug;
use std::hash::Hash;

use pdf_writer::{Finish, Name, Ref};

use crate::chunk_container::ChunkContainer;
use crate::configure::ValidationError;
use crate::graphics::icc::{ICCBasedColorSpace, ICCProfile};
use crate::resource;
use crate::resource::Resourceable;
use crate::serialize::{Cacheable, SerializeContext};

/// The PDF name for the device RGB color space.
pub(crate) const DEVICE_RGB: &str = "DeviceRGB";
/// The PDF name for the device gray color space.
pub(crate) const DEVICE_GRAY: &str = "DeviceGray";
/// The PDF name for the device CMYK color space.
pub(crate) const DEVICE_CMYK: &str = "DeviceCMYK";

/// A wrapper for storing colors from different color spaces.
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum Color {
    /// A device or CIE-based color.
    Regular(RegularColor),
    /// A special color space.
    Special(SpecialColor),
}

/// A device or CIE-based color.
///
/// `Clone` rather than `Copy` because [`RegularColor::IccBased`] carries
/// an [`ICCProfile<3>`] which is internally an `Arc`-backed handle. The
/// `Arc` makes the clone cheap (refcount bump) but precludes `Copy`.
/// All other variants remain trivially copyable; sites that move a
/// `RegularColor` by value now call `.clone()`.
///
/// `Hash` / `Eq` are implemented manually because [`RegularColor::IccBased`]
/// stores `[f32; 3]` components and `f32` does not implement either
/// trait. The bit-representation (`to_bits`) is hashed and compared so
/// equal colours produce equal hashes (with the standard caveat that
/// `+0.0` and `-0.0` hash to different values — acceptable here because
/// the components come from `[0.0, 1.0]` cascade values).
#[derive(Debug, Clone)]
pub enum RegularColor {
    /// An RGB-based color.
    Rgb(rgb::Color),
    /// A luma-based color.
    Luma(luma::Color),
    /// A device CMYK color.
    Cmyk(cmyk::Color),
    /// A three-component colour authored in a wide-gamut ICC-based
    /// space (`color(display-p3 …)`, `color(rec2020 …)`,
    /// `color(a98-rgb …)`, `color(prophoto-rgb …)`,
    /// `color(xyz-d50 …)`, `color(xyz-d65 …)` per CSS Color 5 §4).
    ///
    /// `profile` carries the ICC profile bytes the caller supplied; the
    /// content stream emits `/CS<n> cs <c0> <c1> <c2> scn` and the page
    /// `/Resources /ColorSpace` dictionary gains a `/CS<n> [/ICCBased
    /// <stream>]` entry (ISO 32000-2 §8.6.5.5). Components are in the
    /// `[0.0, 1.0]` range expected by an ICCBased N=3 space.
    ///
    /// The profile is hashed (and compared) by content via the
    /// internal `Prehashed` wrapper, so two `IccBased` colours sharing
    /// the same profile bytes reuse the same `/CS<n>` resource entry.
    IccBased {
        /// Three-component ICC profile (e.g. embedded display-p3.icc).
        profile: ICCProfile<3>,
        /// Source-space components in `[0.0, 1.0]` order matching the
        /// ICC profile's channel layout.
        components: [f32; 3],
    },
    /// A three-component colour authored in a PDF-native CalRGB
    /// calibrated colour space (ISO 32000-2 §8.6.5.6).
    ///
    /// `params` carries the `WhitePoint` / `BlackPoint` / `Gamma` /
    /// `Matrix` dictionary; the content stream emits
    /// `/CS<n> cs <r> <g> <b> scn` against a
    /// `[/CalRGB <<...>>]` colour-space array embedded inline in the
    /// page resources. Components are in the `[0.0, 1.0]` range that
    /// the gamma transfer expects.
    CalRgb {
        /// CalRGB parameter dictionary — WhitePoint (required),
        /// BlackPoint, Gamma, Matrix (all optional, ISO 32000-2 §8.6.5.6).
        params: CalRgbParams,
        /// Three calibrated-space components in `[0.0, 1.0]`.
        components: [f32; 3],
    },
    /// A single-component colour authored in a PDF-native CalGray
    /// calibrated colour space (ISO 32000-2 §8.6.5.5).
    ///
    /// `params` carries the `WhitePoint` / `BlackPoint` / `Gamma`
    /// scalar dictionary; the content stream emits
    /// `/CS<n> cs <g> scn` against a `[/CalGray <<...>>]` colour-space
    /// array.
    CalGray {
        /// CalGray parameter dictionary.
        params: CalGrayParams,
        /// Single calibrated-grey component in `[0.0, 1.0]`.
        component: f32,
    },
    /// A three-component colour authored in the CIE 1976 L*a*b*
    /// (Lab) colour space (ISO 32000-2 §8.6.5.4).
    ///
    /// `params` carries the `WhitePoint` / `BlackPoint` / `Range`
    /// dictionary; the content stream emits
    /// `/CS<n> cs <L> <a> <b> scn` against a `[/Lab <<...>>]`
    /// colour-space array. L is in `[0.0, 100.0]`; `a` and `b` are
    /// within the configured `Range` (default `[-100, 100]`).
    Lab {
        /// Lab parameter dictionary.
        params: LabParams,
        /// `[L, a, b]` triple. L in `[0, 100]`, a/b within the
        /// configured `Range`.
        components: [f32; 3],
    },
}

/// CalRGB parameter dictionary (ISO 32000-2 §8.6.5.6 Table 63).
///
/// `WhitePoint` is required and identifies the diffuse white of the
/// calibrated viewing condition (typically D65 = `[0.9505, 1.0, 1.089]`).
/// `BlackPoint` defaults to `[0, 0, 0]` when absent. `Gamma` is the
/// per-channel transfer-function exponent (default `[1, 1, 1]`).
/// `Matrix` is a 3x3 column-major linearisation matrix used by viewers
/// to map calibrated RGB to XYZ (default identity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalRgbParams {
    /// Tristimulus white-point in CIE 1931 XYZ.
    pub white_point: [f32; 3],
    /// Optional black-point. `None` defaults to `[0, 0, 0]`.
    pub black_point: Option<[f32; 3]>,
    /// Optional per-channel gamma. `None` defaults to `[1, 1, 1]`.
    pub gamma: Option<[f32; 3]>,
    /// Optional 3x3 RGB-to-XYZ matrix (column-major: `[Xr, Yr, Zr,
    /// Xg, Yg, Zg, Xb, Yb, Zb]`). `None` defaults to identity.
    pub matrix: Option<[f32; 9]>,
}

impl Eq for CalRgbParams {}

impl Hash for CalRgbParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for v in self.white_point {
            v.to_bits().hash(state);
        }
        match self.black_point {
            Some(bp) => {
                state.write_u8(1);
                for v in bp {
                    v.to_bits().hash(state);
                }
            }
            None => state.write_u8(0),
        }
        match self.gamma {
            Some(g) => {
                state.write_u8(1);
                for v in g {
                    v.to_bits().hash(state);
                }
            }
            None => state.write_u8(0),
        }
        match self.matrix {
            Some(m) => {
                state.write_u8(1);
                for v in m {
                    v.to_bits().hash(state);
                }
            }
            None => state.write_u8(0),
        }
    }
}

/// CalGray parameter dictionary (ISO 32000-2 §8.6.5.5 Table 62).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalGrayParams {
    /// Tristimulus white-point in CIE 1931 XYZ.
    pub white_point: [f32; 3],
    /// Optional black-point. `None` defaults to `[0, 0, 0]`.
    pub black_point: Option<[f32; 3]>,
    /// Optional gamma scalar. `None` defaults to `1.0`.
    pub gamma: Option<f32>,
}

impl Eq for CalGrayParams {}

impl Hash for CalGrayParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for v in self.white_point {
            v.to_bits().hash(state);
        }
        match self.black_point {
            Some(bp) => {
                state.write_u8(1);
                for v in bp {
                    v.to_bits().hash(state);
                }
            }
            None => state.write_u8(0),
        }
        match self.gamma {
            Some(g) => {
                state.write_u8(1);
                g.to_bits().hash(state);
            }
            None => state.write_u8(0),
        }
    }
}

/// Lab parameter dictionary (ISO 32000-2 §8.6.5.4 Table 61).
///
/// `Range` defaults to `[-100, 100, -100, 100]` (for `a`/`b`); the
/// L component is always `[0, 100]` and is not part of `Range`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabParams {
    /// Tristimulus white-point in CIE 1931 XYZ.
    pub white_point: [f32; 3],
    /// Optional black-point. `None` defaults to `[0, 0, 0]`.
    pub black_point: Option<[f32; 3]>,
    /// Optional `[a_min, a_max, b_min, b_max]` range. `None` defaults
    /// to `[-100, 100, -100, 100]`.
    pub range: Option<[f32; 4]>,
}

impl Eq for LabParams {}

impl Hash for LabParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for v in self.white_point {
            v.to_bits().hash(state);
        }
        match self.black_point {
            Some(bp) => {
                state.write_u8(1);
                for v in bp {
                    v.to_bits().hash(state);
                }
            }
            None => state.write_u8(0),
        }
        match self.range {
            Some(r) => {
                state.write_u8(1);
                for v in r {
                    v.to_bits().hash(state);
                }
            }
            None => state.write_u8(0),
        }
    }
}

impl PartialEq for RegularColor {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Rgb(a), Self::Rgb(b)) => a == b,
            (Self::Luma(a), Self::Luma(b)) => a == b,
            (Self::Cmyk(a), Self::Cmyk(b)) => a == b,
            (
                Self::IccBased {
                    profile: pa,
                    components: ca,
                },
                Self::IccBased {
                    profile: pb,
                    components: cb,
                },
            ) => pa == pb && ca.map(f32::to_bits) == cb.map(f32::to_bits),
            (
                Self::CalRgb {
                    params: pa,
                    components: ca,
                },
                Self::CalRgb {
                    params: pb,
                    components: cb,
                },
            ) => pa == pb && ca.map(f32::to_bits) == cb.map(f32::to_bits),
            (
                Self::CalGray {
                    params: pa,
                    component: ca,
                },
                Self::CalGray {
                    params: pb,
                    component: cb,
                },
            ) => pa == pb && ca.to_bits() == cb.to_bits(),
            (
                Self::Lab {
                    params: pa,
                    components: ca,
                },
                Self::Lab {
                    params: pb,
                    components: cb,
                },
            ) => pa == pb && ca.map(f32::to_bits) == cb.map(f32::to_bits),
            _ => false,
        }
    }
}

impl Eq for RegularColor {}

impl Hash for RegularColor {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Rgb(c) => c.hash(state),
            Self::Luma(c) => c.hash(state),
            Self::Cmyk(c) => c.hash(state),
            Self::IccBased {
                profile,
                components,
            } => {
                profile.hash(state);
                for c in components {
                    c.to_bits().hash(state);
                }
            }
            Self::CalRgb { params, components } => {
                params.hash(state);
                for c in components {
                    c.to_bits().hash(state);
                }
            }
            Self::CalGray { params, component } => {
                params.hash(state);
                component.to_bits().hash(state);
            }
            Self::Lab { params, components } => {
                params.hash(state);
                for c in components {
                    c.to_bits().hash(state);
                }
            }
        }
    }
}

/// A special color space color.
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum SpecialColor {
    /// A separation color.
    Separation(separation::Color),
    /// A DeviceN (multi-colorant) colour value. See
    /// [`devicen`] for the surface and [ISO 32000-2 §8.6.6.5] for the
    /// underlying colour-space construction.
    ///
    /// [ISO 32000-2 §8.6.6.5]: https://www.iso.org/standard/75839.html
    DeviceN(devicen::Color),
}

impl Color {
    pub(crate) fn to_pdf_color(&self) -> Vec<f32> {
        match self {
            Color::Regular(RegularColor::Rgb(rgb)) => rgb.to_pdf_color().to_vec(),
            Color::Regular(RegularColor::Luma(l)) => vec![l.to_pdf_color()],
            Color::Regular(RegularColor::Cmyk(cmyk)) => cmyk.to_pdf_color().to_vec(),
            Color::Regular(RegularColor::IccBased { components, .. }) => components.to_vec(),
            Color::Regular(RegularColor::CalRgb { components, .. }) => components.to_vec(),
            Color::Regular(RegularColor::CalGray { component, .. }) => vec![*component],
            Color::Regular(RegularColor::Lab { components, .. }) => components.to_vec(),
            Color::Special(SpecialColor::Separation(spot)) => vec![spot.to_pdf_color()],
            Color::Special(SpecialColor::DeviceN(dn)) => dn.to_pdf_color(),
        }
    }

    pub(crate) fn color_space(&self, sc: &mut SerializeContext) -> ColorSpace {
        match self {
            Color::Regular(c) => c.color_space(sc).into(),
            Color::Special(c) => c.color_space().into(),
        }
    }

    /// Convert a color to a regular color for use with constructs like tags or
    /// annotations that don't support special color spaces.
    ///
    /// Returns a `Clone` rather than a `Copy` because [`RegularColor`]
    /// holds an `Arc`-backed [`RegularColor::IccBased`] variant. The
    /// clone is cheap (Arc refcount bump for `IccBased`, byte-wise
    /// copy for every other arm).
    pub(crate) fn to_regular(&self) -> RegularColor {
        match self {
            Color::Regular(c) => c.clone(),
            Color::Special(SpecialColor::Separation(c)) => c.space.fallback.clone(),
            // DeviceN's alternate process space is the "fallback" we
            // surface to constructs that can't honour multi-colorant
            // paints (tag content, annotations).
            Color::Special(SpecialColor::DeviceN(c)) => c.space.alternate.clone(),
        }
    }

    /// Promote an RGB grey to a Luma colour when
    /// [`SerializeSettings::rgb_grey_to_devicegray`] is enabled.
    ///
    /// Returns `self` unchanged unless the setting is on, the colour is
    /// `RegularColor::Rgb`, and all three channels are byte-equal — in
    /// which case the colour is rewritten as `RegularColor::Luma`
    /// preserving the channel value. Special colours and CMYK paints
    /// are never promoted.
    ///
    /// Called from the content-builder solid-fill / solid-stroke
    /// dispatch alongside [`Color::project`]; the chain `project then
    /// maybe_promote_grey_to_luma` means a `ForceRgb` projection that
    /// produces an `(L, L, L)` triple still promotes to Luma.
    ///
    /// [`SerializeSettings::rgb_grey_to_devicegray`]:
    ///     crate::SerializeSettings::rgb_grey_to_devicegray
    pub(crate) fn maybe_promote_grey_to_luma(self, sc: &SerializeContext) -> Color {
        if !sc.serialize_settings().rgb_grey_to_devicegray {
            return self;
        }
        match self {
            Color::Regular(RegularColor::Rgb(r)) if r.0 == r.1 && r.1 == r.2 => {
                luma::Color::new(r.0).into()
            }
            // `IccBased` is a wide-gamut authoring path; promoting it
            // to DeviceGray would discard the source-space precision
            // the caller deliberately retained. Pass through unchanged.
            _ => self,
        }
    }

    /// Project this colour through the supplied [`ColourConversion`]
    /// policy.
    ///
    /// Returns a new `Color` in the target space (or `self` for the
    /// pass-through variants). Maths is performed in normalised
    /// `f32` `[0, 1]` and quantised back to `u8` on the way out.
    /// See the [`ColourConversion`] variants for the precise
    /// formulae.
    pub(crate) fn project(self, policy: ColourConversion) -> Color {
        match policy {
            ColourConversion::Auto
            | ColourConversion::None
            | ColourConversion::ContentOnly
            | ColourConversion::ForceSpot => self,
            ColourConversion::ForceRgb => match self {
                Color::Regular(RegularColor::Rgb(_)) => self,
                Color::Regular(RegularColor::Cmyk(c)) => cmyk_to_rgb(c).into(),
                Color::Regular(RegularColor::Luma(l)) => rgb::Color::new(l.0, l.0, l.0).into(),
                // `IccBased` is a wide-gamut path that the projection
                // helpers (which work in u8) cannot honour without
                // discarding the very precision the caller asked us to
                // preserve. Pass through; the content emission already
                // resolves to an ICC stream resource.
                Color::Regular(RegularColor::IccBased { .. }) => self,
                // `CalRgb`, `CalGray`, and `Lab` are calibrated CIE-based
                // spaces whose channel values are not in DeviceRGB
                // primaries; the integer u8 projection helpers cannot
                // honour them without a CIE-to-device transform. Pass
                // through; emission resolves them inline as
                // `[/CalRGB|/CalGray|/Lab <<…>>]` resources.
                Color::Regular(RegularColor::CalRgb { .. })
                | Color::Regular(RegularColor::CalGray { .. })
                | Color::Regular(RegularColor::Lab { .. }) => self,
                Color::Special(SpecialColor::Separation(spot)) => separation_to_regular(&spot)
                    .into_color()
                    .project(ColourConversion::ForceRgb),
                // Stage A DeviceN projection: fall back to the
                // alternate process colour. A proper blend over N
                // tints requires multi-channel arithmetic that Stage A
                // does not yet wire — the alt-space pass-through is
                // safe (it matches the `to_regular()` semantics) and
                // gets refined in Stage C.
                Color::Special(SpecialColor::DeviceN(c)) => c
                    .space
                    .alternate
                    .into_color()
                    .project(ColourConversion::ForceRgb),
            },
            ColourConversion::ForceCmyk => match self {
                Color::Regular(RegularColor::Cmyk(_)) => self,
                Color::Regular(RegularColor::Rgb(r)) => rgb_to_cmyk(r).into(),
                Color::Regular(RegularColor::Luma(l)) => {
                    // Pure-K projection: c = m = y = 0, k = 1 - L.
                    let k = 255u8.saturating_sub(l.0);
                    cmyk::Color::new(0, 0, 0, k).into()
                }
                Color::Regular(RegularColor::IccBased { .. }) => self,
                Color::Regular(RegularColor::CalRgb { .. })
                | Color::Regular(RegularColor::CalGray { .. })
                | Color::Regular(RegularColor::Lab { .. }) => self,
                Color::Special(SpecialColor::Separation(spot)) => separation_to_regular(&spot)
                    .into_color()
                    .project(ColourConversion::ForceCmyk),
                Color::Special(SpecialColor::DeviceN(c)) => c
                    .space
                    .alternate
                    .into_color()
                    .project(ColourConversion::ForceCmyk),
            },
            ColourConversion::ForceGrey => match self {
                Color::Regular(RegularColor::Luma(_)) => self,
                Color::Regular(RegularColor::Rgb(r)) => rgb_to_grey(r).into(),
                Color::Regular(RegularColor::Cmyk(c)) => cmyk_to_grey(c).into(),
                Color::Regular(RegularColor::IccBased { .. }) => self,
                Color::Regular(RegularColor::CalRgb { .. })
                | Color::Regular(RegularColor::CalGray { .. })
                | Color::Regular(RegularColor::Lab { .. }) => self,
                Color::Special(SpecialColor::Separation(spot)) => separation_to_regular(&spot)
                    .into_color()
                    .project(ColourConversion::ForceGrey),
                Color::Special(SpecialColor::DeviceN(c)) => c
                    .space
                    .alternate
                    .into_color()
                    .project(ColourConversion::ForceGrey),
            },
        }
    }
}

impl RegularColor {
    /// Internal helper that lifts a [`RegularColor`] to a [`Color`]
    /// for `project`'s recursive case.
    #[inline]
    fn into_color(self) -> Color {
        Color::Regular(self)
    }

    /// Construct a three-component ICC-based wide-gamut colour.
    ///
    /// `profile` is an [`ICCProfile<3>`] (build it via
    /// [`crate::icc::ICCProfile::new`]) and `components` are the
    /// source-space channel values in the `[0.0, 1.0]` range that the
    /// profile expects. Each call returns a fresh `RegularColor` —
    /// dedup happens at the resource-registration layer via the
    /// profile's content-addressed hash, so it is safe (and cheap)
    /// to call this constructor every time a wide-gamut paint is
    /// resolved.
    pub fn icc_based(profile: ICCProfile<3>, components: [f32; 3]) -> Self {
        Self::IccBased {
            profile,
            components,
        }
    }

    /// Construct a CalRGB-calibrated three-component colour.
    ///
    /// `params` carries the `WhitePoint` / `BlackPoint` / `Gamma` /
    /// `Matrix` dictionary the PDF viewer uses to map calibrated RGB to
    /// XYZ. `components` are in the `[0.0, 1.0]` range that the gamma
    /// transfer expects. Per-document dedup happens at the
    /// colour-space-resource registration layer via the params'
    /// content hash.
    pub fn cal_rgb(params: CalRgbParams, components: [f32; 3]) -> Self {
        Self::CalRgb { params, components }
    }

    /// Construct a CalGray-calibrated single-component grey.
    ///
    /// `params` carries the `WhitePoint` / `BlackPoint` / `Gamma`
    /// scalar. `component` is in the `[0.0, 1.0]` range.
    pub fn cal_gray(params: CalGrayParams, component: f32) -> Self {
        Self::CalGray { params, component }
    }

    /// Construct a CIE 1976 L*a*b* (Lab) three-component colour.
    ///
    /// `params` carries the `WhitePoint` / `BlackPoint` / `Range`
    /// dictionary. `components[0]` (L) is in `[0.0, 100.0]`; `a` and
    /// `b` are within the configured `Range` (default
    /// `[-100, 100, -100, 100]`).
    pub fn lab(params: LabParams, components: [f32; 3]) -> Self {
        Self::Lab { params, components }
    }
}

// --- Projection helpers ---------------------------------------------------
//
// These free functions implement the channel-level maths for each
// source -> target projection. They are kept `pub(crate)` so they
// stay invisible to library consumers; the only public surface is
// [`Color::project`].

#[inline]
fn u8_to_unit(channel: u8) -> f32 {
    channel as f32 / 255.0
}

#[inline]
fn unit_to_u8(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// RGB -> CMYK per ISO 32000-2 §8.6.4.
///
/// `k = 1 - max(r, g, b)`, then `c = (1 - r - k) / (1 - k)` (and
/// analogously for `m`/`y`). When `k == 1` (pure black) the
/// divisor collapses, so `c`, `m`, `y` are forced to zero.
pub(crate) fn rgb_to_cmyk(rgb: rgb::Color) -> cmyk::Color {
    let r = u8_to_unit(rgb.0);
    let g = u8_to_unit(rgb.1);
    let b = u8_to_unit(rgb.2);
    let max = r.max(g).max(b);
    let k = 1.0 - max;
    let (c, m, y) = if (1.0 - k).abs() < f32::EPSILON {
        (0.0, 0.0, 0.0)
    } else {
        let denom = 1.0 - k;
        (
            (1.0 - r - k) / denom,
            (1.0 - g - k) / denom,
            (1.0 - b - k) / denom,
        )
    };
    cmyk::Color::new(unit_to_u8(c), unit_to_u8(m), unit_to_u8(y), unit_to_u8(k))
}

/// RGB -> Luma using Rec. 709 luminance coefficients.
///
/// `y = 0.2126*r + 0.7152*g + 0.0722*b`.
pub(crate) fn rgb_to_grey(rgb: rgb::Color) -> luma::Color {
    let r = u8_to_unit(rgb.0);
    let g = u8_to_unit(rgb.1);
    let b = u8_to_unit(rgb.2);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    luma::Color::new(unit_to_u8(y))
}

/// CMYK -> RGB using the straight-line subtractive model.
///
/// `r = (1 - c) * (1 - k)`, etc.
pub(crate) fn cmyk_to_rgb(cmyk: cmyk::Color) -> rgb::Color {
    let c = u8_to_unit(cmyk.0);
    let m = u8_to_unit(cmyk.1);
    let y = u8_to_unit(cmyk.2);
    let k = u8_to_unit(cmyk.3);
    let r = (1.0 - c) * (1.0 - k);
    let g = (1.0 - m) * (1.0 - k);
    let b = (1.0 - y) * (1.0 - k);
    rgb::Color::new(unit_to_u8(r), unit_to_u8(g), unit_to_u8(b))
}

/// CMYK -> Luma via the RGB intermediate.
pub(crate) fn cmyk_to_grey(cmyk: cmyk::Color) -> luma::Color {
    rgb_to_grey(cmyk_to_rgb(cmyk))
}

/// Resolve a Separation colour to its fallback [`RegularColor`]
/// linearly scaled by the tint.
///
/// This matches krilla's existing tint-into-fallback behaviour
/// (see [`separation::SeparationSpace`]'s doc-comment): a 50% tint
/// of a red fallback yields a 50%-saturated red. The scaling is
/// done channel-wise in the fallback's native space.
pub(crate) fn separation_to_regular(spot: &separation::Color) -> RegularColor {
    let tint = u8_to_unit(spot.tint);
    match &spot.space.fallback {
        RegularColor::Rgb(c) => rgb::Color::new(
            unit_to_u8(u8_to_unit(c.0) * tint),
            unit_to_u8(u8_to_unit(c.1) * tint),
            unit_to_u8(u8_to_unit(c.2) * tint),
        )
        .into(),
        RegularColor::Cmyk(c) => cmyk::Color::new(
            unit_to_u8(u8_to_unit(c.0) * tint),
            unit_to_u8(u8_to_unit(c.1) * tint),
            unit_to_u8(u8_to_unit(c.2) * tint),
            unit_to_u8(u8_to_unit(c.3) * tint),
        )
        .into(),
        RegularColor::Luma(c) => luma::Color::new(unit_to_u8(u8_to_unit(c.0) * tint)).into(),
        // Separation fallback cannot legally be an ICC-based wide-gamut
        // colour: PDF 32000-2 §8.6.6.4 requires the alternate space to
        // be a process colour (DeviceGray / DeviceRGB / DeviceCMYK / a
        // CIE-based equivalent). Callers passing an ICC fallback get a
        // black fallback (defensive — every existing constructor uses
        // RGB / CMYK / Luma).
        RegularColor::IccBased { components, .. } => rgb::Color::new(
            unit_to_u8(components[0] * tint),
            unit_to_u8(components[1] * tint),
            unit_to_u8(components[2] * tint),
        )
        .into(),
        // Calibrated CIE-based fallbacks (CalRGB / CalGray / Lab) are
        // outside the ISO 32000-2 §8.6.6.4 alternate-space contract.
        // Project the channel-zero components linearly by the tint as a
        // defensive device-space approximation — the constructors used
        // in practice route through the process arms above.
        RegularColor::CalRgb { components, .. } => rgb::Color::new(
            unit_to_u8(components[0] * tint),
            unit_to_u8(components[1] * tint),
            unit_to_u8(components[2] * tint),
        )
        .into(),
        RegularColor::CalGray { component, .. } => {
            luma::Color::new(unit_to_u8(*component * tint)).into()
        }
        RegularColor::Lab { components, .. } => rgb::Color::new(
            // L is in [0, 100]; rescale into [0, 1] before tint.
            unit_to_u8((components[0] / 100.0).clamp(0.0, 1.0) * tint),
            unit_to_u8(((components[1] + 100.0) / 200.0).clamp(0.0, 1.0) * tint),
            unit_to_u8(((components[2] + 100.0) / 200.0).clamp(0.0, 1.0) * tint),
        )
        .into(),
    }
}

impl RegularColor {
    pub(crate) fn color_space(&self, sc: &mut SerializeContext) -> RegularColorSpace {
        // `preserve_black` short-circuits the per-paint ICC routing
        // for pure black so it emits in the underlying device space
        // (DeviceRGB / DeviceCMYK) verbatim, sidestepping the near-
        // black drift that a CIE-based / fallback CMYK profile would
        // introduce. Luma is excluded because it has no ICC reroute
        // hazard for pure black: `DeviceGray` is the only place a
        // single-channel zero can land. The validator path is left
        // intact (RGB still triggers `ContainsRgb` under CMYK-only
        // validators) — `preserve_black` is documented as a non-
        // validated, print-oriented workflow opt-in.
        let preserve_black = sc.serialize_settings().preserve_black;
        match self {
            Self::Rgb(r) => {
                if sc.serialize_settings().validators().requires_cmyk_only() {
                    sc.register_validation_error(ValidationError::ContainsRgb(sc.location));
                }
                // `preserve_black` emits pure-black RGB verbatim as DeviceRGB,
                // placed after the validation registration so the validator path
                // stays intact (pure black still triggers `ContainsRgb` under a
                // CMYK-only validator).
                if preserve_black && r.0 == 0 && r.1 == 0 && r.2 == 0 {
                    return DeviceColorSpace::Rgb.into();
                }
                r.color_space(sc.serialize_settings().no_device_cs)
            }
            Self::Luma(_) => {
                // PDF/X: DeviceGray content is characterized by the output
                // intent, so it stays DeviceGray (matching the DeviceCMYK
                // treatment) rather than being ICC-wrapped to sGray.
                //
                // ISO 15930-7 §6.4.3.2 / ISO 15930-9 §6.6.3.2: a device colour
                // space may be used only if it matches the output intent, or the
                // intent is CMYK and the space is DeviceGray. DeviceGray is thus
                // valid under a CMYK or grayscale intent but not under an RGB one
                // (which would require a DefaultGray colour space that krilla
                // does not emit).
                if sc.serialize_settings().validators().is_pdf_x()
                    && sc.serialize_settings().pdfx_output_intent_is_rgb()
                {
                    sc.register_validation_error(ValidationError::OutputIntentColorSpaceMismatch(
                        sc.location,
                    ));
                }
                let no_device_cs = sc.serialize_settings().no_device_cs
                    && !sc.serialize_settings().validators().is_pdf_x();
                luma::color_space(no_device_cs)
            }
            Self::Cmyk(c) => {
                // PDF/X emits DeviceCMYK, which the GTS_PDFX output intent must
                // characterize: its profile has to be CMYK. A present-but-non-
                // CMYK output target (e.g. an external RGB profile for X-4p, or a
                // 4-channel non-`'CMYK'` profile) leaves the content
                // uncharacterized. A missing profile is reported separately.
                if sc.serialize_settings().validators().is_pdf_x()
                    && sc.serialize_settings().pdfx_output_intent_is_cmyk() == Some(false)
                {
                    sc.register_validation_error(ValidationError::OutputIntentColorSpaceMismatch(
                        sc.location,
                    ));
                }
                // `preserve_black` emits pure-black CMYK verbatim as DeviceCMYK,
                // after the validation registration above.
                if preserve_black && c.0 == 0 && c.1 == 0 && c.2 == 0 && c.3 == 255 {
                    return DeviceColorSpace::Cmyk.into();
                }
                match cmyk::color_space(&sc.serialize_settings()) {
                    None => {
                        sc.register_validation_error(ValidationError::MissingCMYKProfile);
                        DeviceColorSpace::Cmyk.into()
                    }
                    Some(cs) => cs,
                }
            }
            Self::IccBased { profile, .. } => {
                // PDF/X-1a (ISO 15930-4) forbids non-CMYK content,
                // which captures wide-gamut RGB-equivalent ICC paints
                // too. Surface the validator violation; emission still
                // proceeds via the ICCBased N=3 path.
                if sc.serialize_settings().validators().requires_cmyk_only() {
                    sc.register_validation_error(ValidationError::ContainsRgb(sc.location));
                }
                CieBasedColorSpace::IccRgb(ICCBasedColorSpace::<3>(profile.clone())).into()
            }
            Self::CalRgb { params, .. } => {
                // PDF/X-1a forbids RGB-equivalent content; CalRGB is a
                // three-component CIE-based RGB approximation and is
                // therefore caught by the same `requires_cmyk_only`
                // rule. Emission proceeds via the inline
                // `[/CalRGB <<…>>]` colour-space array.
                if sc.serialize_settings().validators().requires_cmyk_only() {
                    sc.register_validation_error(ValidationError::ContainsRgb(sc.location));
                }
                CieBasedColorSpace::CalRgb(CalRgbColorSpace(*params)).into()
            }
            Self::CalGray { params, .. } => {
                // CalGray is a single-component grey space. PDF/X-1a's
                // CMYK-only check does not fire for grey content
                // (DeviceGray fallback is unconditional), so no
                // validation error is registered here.
                CieBasedColorSpace::CalGray(CalGrayColorSpace(*params)).into()
            }
            Self::Lab { params, .. } => {
                // Lab is device-independent and addressable by
                // PDF/X-1a; the validator does not flag it as
                // RGB-equivalent.
                CieBasedColorSpace::Lab(LabColorSpace(*params)).into()
            }
        }
    }

    /// Return the current color as RGB for use with colored glyphs (SVG and
    /// COLR).
    pub(crate) fn as_rgb(&self) -> Option<rgb::Color> {
        Some(match self {
            Self::Rgb(r) => *r,
            Self::Luma(l) => rgb::Color::new(l.0, l.0, l.0),
            Self::Cmyk(_) => return None,
            // Colour-font glyph paint paths expect device-space RGB
            // bytes. An ICC-based wide-gamut colour has no defined
            // single u8 RGB projection without an ICC engine, so we
            // refuse here — the caller's existing `None` branch falls
            // back to the foreground colour.
            Self::IccBased { .. } => return None,
            // Calibrated CIE-based variants (CalRGB / CalGray / Lab)
            // require a viewer-side CIE transform that krilla does not
            // implement; surfacing a u8 RGB projection here would be
            // wrong by definition. Refuse so the caller falls back to
            // the foreground colour, identical to the IccBased path.
            Self::CalRgb { .. } | Self::CalGray { .. } | Self::Lab { .. } => return None,
        })
    }

    /// Returns true if this is a subtractive color space (CMYK), false otherwise (RGB, Luma).
    /// Used for determining the correct tint transform behavior in Separation color spaces.
    pub(crate) fn is_subtractive(&self) -> bool {
        matches!(self, Self::Cmyk(_))
    }
}

impl SpecialColor {
    pub(crate) fn color_space(&self) -> SpecialColorSpace {
        match self {
            Self::Separation(spot) => spot.color_space().into(),
            Self::DeviceN(dn) => dn.color_space().into(),
        }
    }
}

/// Gray-scale colors.
pub mod luma {
    use crate::color::{CieBasedColorSpace, DeviceColorSpace, RegularColor, RegularColorSpace};

    /// A luma color.
    #[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
    pub struct Color(pub(crate) u8);

    impl Color {
        /// Create a new luma color.
        pub fn new(lightness: u8) -> Color {
            Color(lightness)
        }

        /// Create a black luma color.
        pub fn black() -> Self {
            Self::new(0)
        }

        /// Create a white RGB color.
        pub fn white() -> Self {
            Self::new(255)
        }

        pub(crate) fn to_pdf_color(self) -> f32 {
            self.0 as f32 / 255.0
        }
    }

    impl From<Color> for super::RegularColor {
        fn from(val: Color) -> Self {
            super::RegularColor::Luma(val)
        }
    }

    impl From<Color> for super::Color {
        fn from(val: Color) -> Self {
            RegularColor::from(val).into()
        }
    }

    impl Default for Color {
        fn default() -> Self {
            Color::new(0)
        }
    }

    pub(crate) fn color_space(no_device_cs: bool) -> RegularColorSpace {
        if no_device_cs {
            CieBasedColorSpace::Luma.into()
        } else {
            DeviceColorSpace::Gray.into()
        }
    }
}

impl From<RegularColor> for Color {
    fn from(value: RegularColor) -> Self {
        Self::Regular(value)
    }
}

impl From<SpecialColor> for Color {
    fn from(value: SpecialColor) -> Self {
        Self::Special(value)
    }
}

/// CMYK colors.
pub mod cmyk {
    use crate::color::{CieBasedColorSpace, DeviceColorSpace, RegularColorSpace};
    use crate::graphics::icc::ICCBasedColorSpace;
    use crate::SerializeSettings;

    /// A CMYK color.
    #[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
    pub struct Color(pub(crate) u8, pub(crate) u8, pub(crate) u8, pub(crate) u8);

    impl Color {
        /// Create a new CMYK color.
        pub fn new(cyan: u8, magenta: u8, yellow: u8, black: u8) -> Color {
            Color(cyan, magenta, yellow, black)
        }

        pub(crate) fn to_pdf_color(self) -> [f32; 4] {
            [
                self.0 as f32 / 255.0,
                self.1 as f32 / 255.0,
                self.2 as f32 / 255.0,
                self.3 as f32 / 255.0,
            ]
        }
    }

    impl From<Color> for super::RegularColor {
        fn from(val: Color) -> Self {
            super::RegularColor::Cmyk(val)
        }
    }

    impl From<Color> for super::Color {
        fn from(val: Color) -> Self {
            super::RegularColor::from(val).into()
        }
    }

    impl Default for Color {
        fn default() -> Self {
            Color::new(0, 0, 0, 255)
        }
    }

    pub(crate) fn color_space(ss: &SerializeSettings) -> Option<RegularColorSpace> {
        // PDF/X always writes a CMYK output intent that characterizes
        // `DeviceCMYK` page content. Wrapping CMYK in an ICCBased color space
        // that duplicates the output-intent profile is disallowed by PDF/X, so
        // CMYK content remains `DeviceCMYK` regardless of `no_device_cs`. (RGB
        // and gray still honour `no_device_cs`, since a CMYK output intent does
        // not characterize them.)
        if ss.validators().is_pdf_x() {
            return Some(DeviceColorSpace::Cmyk.into());
        }

        if ss.no_device_cs {
            ss.clone()
                .cmyk_profile
                .map(|p| CieBasedColorSpace::Cmyk(ICCBasedColorSpace::<4>(p.clone())).into())
        } else {
            Some(DeviceColorSpace::Cmyk.into())
        }
    }
}

/// RGB colors.
pub mod rgb {
    use crate::color::{CieBasedColorSpace, DeviceColorSpace, RegularColorSpace};

    /// An RGB color.
    #[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
    pub struct Color(pub(crate) u8, pub(crate) u8, pub(crate) u8);

    impl Default for Color {
        fn default() -> Self {
            Color::black()
        }
    }

    impl Color {
        /// Create a new RGB color.
        pub fn new(red: u8, green: u8, blue: u8) -> Self {
            Color(red, green, blue)
        }

        /// Create a new linear RGB color.
        pub fn new_linear(red: u8, green: u8, blue: u8) -> Self {
            Color(red, green, blue)
        }

        /// Create a black RGB color.
        pub fn black() -> Self {
            Self::new(0, 0, 0)
        }

        /// Create a white RGB color.
        pub fn white() -> Self {
            Self::new(255, 255, 255)
        }

        /// The `red` component of the color.
        pub fn red(&self) -> u8 {
            self.0
        }

        /// The `green` component of the color.
        pub fn green(&self) -> u8 {
            self.1
        }

        /// The `blue` component of the color.
        pub fn blue(&self) -> u8 {
            self.2
        }

        pub(crate) fn to_pdf_color(self) -> [f32; 3] {
            [
                self.0 as f32 / 255.0,
                self.1 as f32 / 255.0,
                self.2 as f32 / 255.0,
            ]
        }

        pub(super) fn color_space(&self, no_device_cs: bool) -> RegularColorSpace {
            color_space(no_device_cs)
        }
    }

    impl From<Color> for super::RegularColor {
        fn from(val: Color) -> Self {
            super::RegularColor::Rgb(val)
        }
    }

    impl From<Color> for super::Color {
        fn from(val: Color) -> Self {
            super::RegularColor::from(val).into()
        }
    }

    pub(crate) fn color_space(no_device_cs: bool) -> RegularColorSpace {
        if no_device_cs {
            CieBasedColorSpace::Srgb.into()
        } else {
            DeviceColorSpace::Rgb.into()
        }
    }
}

/// Separation (spot) colors.
pub mod separation {
    use crate::color::RegularColor;

    /// A spot color.
    #[derive(Debug, Hash, Eq, PartialEq, Clone)]
    pub struct Color {
        pub(crate) tint: u8,
        pub(crate) space: SeparationSpace,
    }

    impl Color {
        /// Create a new spot color.
        pub fn new(tint: u8, space: SeparationSpace) -> Self {
            Self { tint, space }
        }

        pub(crate) fn to_pdf_color(&self) -> f32 {
            self.tint as f32 / 255.0
        }

        pub(crate) fn color_space(&self) -> SeparationSpace {
            self.space.clone()
        }
    }

    impl Default for Color {
        fn default() -> Self {
            Color::new(0, SeparationSpace::default())
        }
    }

    impl From<Color> for super::SpecialColor {
        fn from(val: Color) -> Self {
            super::SpecialColor::Separation(val)
        }
    }

    impl From<Color> for super::Color {
        fn from(val: Color) -> Self {
            super::SpecialColor::from(val).into()
        }
    }

    /// A Separation color space (also known as spot colors).
    ///
    /// Separation color spaces use a single, subtractive colorant and allow
    /// achieving exact color reproduction in print.
    ///
    /// Krilla automatically linearly scales the fallback color with the separation
    /// tint.
    #[derive(Debug, Eq, PartialEq, Hash, Clone)]
    pub struct SeparationSpace {
        pub(crate) colorant: SeparationColorant,
        pub(crate) fallback: RegularColor,
    }

    impl SeparationSpace {
        /// Create a new Separation space.
        ///
        /// To export PDF/A-2 and later, make sure that a single Separation Colorant
        /// is always used with the same fallback color.
        pub fn new(colorant: SeparationColorant, fallback: RegularColor) -> Self {
            Self { colorant, fallback }
        }
    }

    impl From<SeparationSpace> for super::SpecialColorSpace {
        fn from(value: SeparationSpace) -> Self {
            Self::Separation(value)
        }
    }

    impl Default for SeparationSpace {
        fn default() -> Self {
            Self {
                colorant: SeparationColorant::default(),
                fallback: super::rgb::Color::default().into(),
            }
        }
    }

    /// What colorant to use for colors in this space.
    #[derive(Debug, Eq, PartialEq, Hash, Clone, Default)]
    pub enum SeparationColorant {
        /// Don't apply colorant at all. Sometimes used to indicate other production
        /// info, such as cuts.
        #[default]
        NoColorant,
        /// Apply the same amount of each available colorants.
        AllColorants,
        /// Specify a colorant name.
        Custom(String),
    }

    impl SeparationColorant {
        pub(crate) fn to_pdf<'a>(&'a self) -> pdf_writer::Name<'a> {
            match self {
                Self::AllColorants => pdf_writer::Name(b"All"),
                Self::NoColorant => pdf_writer::Name(b"None"),
                Self::Custom(s) => pdf_writer::Name(s.as_bytes()),
            }
        }
    }
}

/// DeviceN (multi-colorant spot) colour space surface, ISO 32000-2
/// §8.6.6.5.
///
/// A DeviceN colour space lists *N* colorant names alongside an
/// alternate process colour space and a tint-transform function that
/// maps the *N* tints onto the alternate space's components. Authoring
/// flow:
///
/// 1. Build a [`TintTransform`](devicen::TintTransform) describing how each colorant contributes
///    to the alternate-space components at full tint.
/// 2. Build a [`DeviceNSpace`](devicen::DeviceNSpace) from the colorant names, the alternate
///    process colour (its variant fixes the channel count), and the
///    tint transform.
/// 3. Paint with a [`Color`] carrying *N* per-channel tints in
///    `[0.0, 1.0]`.
///
/// Stage A exposes only [`TintTransform::Linear`](devicen::TintTransform::Linear) — a blend in the
/// alternate space. PDF/A profiles forbid DeviceN that uses a Type 4
/// (PostScript) tint transform; the writer therefore emits a single
/// Type 2 exponential function for the `N = 1` case (PDF/A-friendly)
/// and a Type 4 PostScript calculator for `N > 1` (PDF/X-4 / PDF 2.0
/// only — the validator flags the PostScript dependency).
pub mod devicen {
    use super::RegularColor;

    /// A DeviceN colour value: per-colorant tints in `[0.0, 1.0]`
    /// against a [`DeviceNSpace`].
    ///
    /// `tints.len()` must equal `space.colorants.len()` — the
    /// constructor enforces this invariant.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Color {
        pub(crate) tints: Vec<f32>,
        pub(crate) space: DeviceNSpace,
    }

    impl Eq for Color {}

    impl std::hash::Hash for Color {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            // Hash bit patterns: `f32` has no `Hash`. `+0.0` and
            // `-0.0` hash differently — acceptable; tints are
            // normalised cascade inputs.
            for t in &self.tints {
                t.to_bits().hash(state);
            }
            self.space.hash(state);
        }
    }

    impl Color {
        /// Create a new DeviceN colour value.
        ///
        /// Returns `None` if `tints.len() != space.colorants.len()`.
        /// Callers supply normalised tints in `[0.0, 1.0]`; values
        /// outside that range are passed through verbatim to the
        /// content stream (PDF interpreters clamp).
        pub fn new(tints: Vec<f32>, space: DeviceNSpace) -> Option<Self> {
            if tints.len() == space.colorants.len() {
                Some(Self { tints, space })
            } else {
                None
            }
        }

        pub(crate) fn to_pdf_color(&self) -> Vec<f32> {
            self.tints.clone()
        }

        pub(crate) fn color_space(&self) -> DeviceNSpace {
            self.space.clone()
        }

        /// Borrow the per-colorant tints.
        pub fn tints(&self) -> &[f32] {
            &self.tints
        }

        /// Borrow the colour space this value paints in.
        pub fn space(&self) -> &DeviceNSpace {
            &self.space
        }
    }

    impl From<Color> for super::SpecialColor {
        fn from(val: Color) -> Self {
            super::SpecialColor::DeviceN(val)
        }
    }

    impl From<Color> for super::Color {
        fn from(val: Color) -> Self {
            super::SpecialColor::from(val).into()
        }
    }

    /// A DeviceN colour space — *N* colorant names plus an alternate
    /// process colour space plus a tint transform.
    ///
    /// The alternate space is carried as a [`RegularColor`] whose
    /// variant fixes the channel layout: [`RegularColor::Rgb`] yields
    /// a `/DeviceRGB` alt-space entry, [`RegularColor::Cmyk`] yields
    /// `/DeviceCMYK`, etc. The actual colour value of `alternate` is
    /// ignored by the writer — only its variant matters — but the
    /// type is held verbatim so `Color::to_regular()` returns a
    /// well-defined fallback for clients that cannot honour multi-
    /// colorant paints (tag-tree alt-text, annotation colours).
    ///
    /// PDF/A-1 forbids DeviceN; PDF/A-2 onward and every PDF/X profile
    /// admit it. The writer dispatches the validator hook through
    /// `ValidationStore::validate_devicen`.
    #[derive(Debug, Eq, PartialEq, Hash, Clone)]
    pub struct DeviceNSpace {
        pub(crate) colorants: Vec<String>,
        pub(crate) alternate: RegularColor,
        pub(crate) tint_transform: TintTransform,
    }

    impl DeviceNSpace {
        /// Create a new DeviceN colour space.
        ///
        /// Returns `None` if `colorants` is empty, the tint transform's
        /// per-colorant component vector has a different length to
        /// `colorants`, or any inner per-colorant vector's length
        /// disagrees with the alternate space's channel count.
        pub fn new(
            colorants: Vec<String>,
            alternate: RegularColor,
            tint_transform: TintTransform,
        ) -> Option<Self> {
            if colorants.is_empty() {
                return None;
            }
            if !tint_transform.matches_arity(colorants.len(), &alternate) {
                return None;
            }
            Some(Self {
                colorants,
                alternate,
                tint_transform,
            })
        }

        /// Number of colorants. Written verbatim as the `/N` count in
        /// the DeviceN colour-space array.
        pub fn colorant_count(&self) -> usize {
            self.colorants.len()
        }

        /// Borrow the colorant-name slice.
        pub fn colorants(&self) -> &[String] {
            &self.colorants
        }

        /// Borrow the alternate-space anchor.
        pub fn alternate(&self) -> &RegularColor {
            &self.alternate
        }
    }

    impl From<DeviceNSpace> for super::SpecialColorSpace {
        fn from(value: DeviceNSpace) -> Self {
            Self::DeviceN(value)
        }
    }

    /// Tint-transform function for a [`DeviceNSpace`].
    ///
    /// Stage A exposes only [`TintTransform::Linear`]. A second
    /// variant carrying a raw Type 4 PostScript program will join the
    /// enum in Stage C of the moegoe wire-through.
    ///
    /// `Hash`/`Eq` are implemented manually because the variant carries
    /// `f32` channel values. The `f32::to_bits` round-trip is hashed
    /// and compared — `+0.0` and `-0.0` therefore hash to different
    /// values, which is acceptable: tint-transform inputs come from
    /// the cascade as normalised `[0.0, 1.0]` values that never carry
    /// a negative zero in practice.
    #[derive(Debug, Clone)]
    pub enum TintTransform {
        /// Linear blend in the alternate process space.
        ///
        /// `per_colorant_components[i]` lists the alternate-space
        /// channel values produced by colorant `i` at full tint
        /// (tint = 1.0). Each inner vector must have the same length
        /// as the alternate space's channel count (3 for RGB / 4 for
        /// CMYK / 1 for Luma / 3 for ICC-based wide gamut). The
        /// blend at output channel `m` is
        ///   `out[m] = Σᵢ tintᵢ * per_colorant_components[i][m]`
        /// emitted as a single Type 2 exponential function when `N
        /// == 1` (PDF/A-friendly) and as a Type 4 PostScript
        /// calculator otherwise (PDF/X-4 / PDF 2.0 only).
        Linear {
            /// Outer-vector length equals the colorant count; each
            /// inner vector lists that colorant's contribution to the
            /// alternate space at full tint.
            per_colorant_components: Vec<Vec<f32>>,
        },
    }

    impl PartialEq for TintTransform {
        fn eq(&self, other: &Self) -> bool {
            match (self, other) {
                (
                    Self::Linear {
                        per_colorant_components: a,
                    },
                    Self::Linear {
                        per_colorant_components: b,
                    },
                ) => {
                    if a.len() != b.len() {
                        return false;
                    }
                    a.iter().zip(b.iter()).all(|(av, bv)| {
                        av.len() == bv.len()
                            && av
                                .iter()
                                .zip(bv.iter())
                                .all(|(x, y)| x.to_bits() == y.to_bits())
                    })
                }
            }
        }
    }

    impl Eq for TintTransform {}

    impl std::hash::Hash for TintTransform {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            std::mem::discriminant(self).hash(state);
            match self {
                Self::Linear {
                    per_colorant_components,
                } => {
                    for inner in per_colorant_components {
                        inner.len().hash(state);
                        for v in inner {
                            v.to_bits().hash(state);
                        }
                    }
                }
            }
        }
    }

    impl TintTransform {
        /// Verify that the tint transform's per-colorant data agrees
        /// with both the colorant count and the alternate-space
        /// channel count.
        pub(crate) fn matches_arity(
            &self,
            colorant_count: usize,
            alternate: &RegularColor,
        ) -> bool {
            let alt_channels = alternate_channel_count(alternate);
            match self {
                TintTransform::Linear {
                    per_colorant_components,
                } => {
                    per_colorant_components.len() == colorant_count
                        && per_colorant_components
                            .iter()
                            .all(|v| v.len() == alt_channels)
                }
            }
        }
    }

    /// Number of components an alternate [`RegularColor`] contributes
    /// to the DeviceN tint transform's range.
    pub(crate) fn alternate_channel_count(alternate: &RegularColor) -> usize {
        match alternate {
            RegularColor::Rgb(_) => 3,
            RegularColor::Cmyk(_) => 4,
            RegularColor::Luma(_) => 1,
            RegularColor::IccBased { .. } => 3,
            RegularColor::CalRgb { .. } => 3,
            RegularColor::CalGray { .. } => 1,
            RegularColor::Lab { .. } => 3,
        }
    }
}

/// Colour-conversion policy applied to every fill, stroke, and glyph
/// paint before content-stream emission.
///
/// The variant is read once per paint dispatch from
/// [`crate::SerializeSettings::colour_conversion`]; `Auto` (the default) and
/// `None` pass colours through unchanged, preserving the existing
/// krilla behaviour. The `Force*` variants project regular RGB / CMYK
/// / Luma source colours into the requested target space using the
/// straight-line conversions in ISO 32000-2 §8.6.4 and Rec. 709 for
/// the RGB->Y transform.
///
/// `ContentOnly` is currently a pass-through at this layer; the
/// "skip annotations / interactive content" routing it implies in
/// PDFreactor's CSS-conversion policy is owned by Phase 3 and is
/// **not** yet equivalent to PDFreactor's `ContentOnly` policy.
/// Likewise, `ForceSpot` is reserved for the Phase-3 spot-registry
/// routing and is currently a pass-through.
///
/// British spelling is intentional: the new public surface follows
/// the moegoe naming convention. The existing `Color` type and the
/// `color` module retain their American spelling to avoid breaking
/// the rest of the public API.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ColourConversion {
    /// No projection. Reserved for future policies that may depend on
    /// the active validator or output intent. Currently identical to
    /// [`ColourConversion::None`].
    #[default]
    Auto,
    /// No projection. Source colours are emitted as-is.
    None,
    /// Pass-through at the content-stream layer. Phase 3 will route
    /// the "skip annotations" decision here; today this variant does
    /// not match PDFreactor's `ContentOnly` semantics.
    ContentOnly,
    /// Project the source colour to `RegularColor::Rgb`. CMYK source
    /// colours are converted with `r = (1-c)*(1-k)` etc.; Luma maps
    /// to `r = g = b = L`. Separation colours recurse on
    /// `tint * fallback`.
    ForceRgb,
    /// Project the source colour to `RegularColor::Cmyk` per ISO
    /// 32000-2 §8.6.4. RGB->CMYK: `k = 1 - max(r,g,b)`,
    /// `c = (1-r-k)/(1-k)` (with `k == 1` forcing `c = m = y = 0`).
    /// Luma maps to pure black: `c = m = y = 0; k = 1 - L`.
    ForceCmyk,
    /// Project the source colour to `RegularColor::Luma` using
    /// Rec. 709: `y = 0.2126*r + 0.7152*g + 0.0722*b`. CMYK is
    /// converted via the RGB intermediate.
    ForceGrey,
    /// Reserved for the Phase-3 spot-registry routing. Currently a
    /// pass-through.
    ForceSpot,
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub(crate) enum ColorSpace {
    Device(DeviceColorSpace),
    CieBased(CieBasedColorSpace),
    Special(SpecialColorSpace),
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub(crate) enum RegularColorSpace {
    Device(DeviceColorSpace),
    CieBased(CieBasedColorSpace),
}

impl From<RegularColorSpace> for ColorSpace {
    fn from(value: RegularColorSpace) -> Self {
        match value {
            RegularColorSpace::Device(s) => Self::Device(s),
            RegularColorSpace::CieBased(s) => Self::CieBased(s),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub(crate) enum DeviceColorSpace {
    Rgb,
    Gray,
    Cmyk,
}

impl From<DeviceColorSpace> for ColorSpace {
    fn from(value: DeviceColorSpace) -> Self {
        Self::Device(value)
    }
}

impl From<DeviceColorSpace> for RegularColorSpace {
    fn from(value: DeviceColorSpace) -> Self {
        Self::Device(value)
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub(crate) enum CieBasedColorSpace {
    Srgb,
    Luma,
    Cmyk(ICCBasedColorSpace<4>),
    /// Three-component ICC-based RGB-equivalent space used for CSS
    /// Color 5 wide-gamut paints (`display-p3`, `rec2020`, `a98-rgb`,
    /// `prophoto-rgb`, `xyz-d50`, `xyz-d65`). Plumbed through
    /// `register_colorspace` -> `register_resourceable`, identical
    /// dedup semantics as the CMYK ICC path.
    IccRgb(ICCBasedColorSpace<3>),
    /// CalRGB PDF-native calibrated RGB space (ISO 32000-2 §8.6.5.6).
    /// Emits an inline `[/CalRGB <<WhitePoint … >>]` colour-space
    /// array as a `/CS<n>` resource.
    CalRgb(CalRgbColorSpace),
    /// CalGray PDF-native calibrated grey space (ISO 32000-2 §8.6.5.5).
    /// Emits an inline `[/CalGray <<WhitePoint … >>]` colour-space
    /// array as a `/CS<n>` resource.
    CalGray(CalGrayColorSpace),
    /// CIE 1976 L*a*b* (Lab) space (ISO 32000-2 §8.6.5.4). Emits an
    /// inline `[/Lab <<WhitePoint … >>]` colour-space array as a
    /// `/CS<n>` resource.
    Lab(LabColorSpace),
}

/// Colour-space-resource wrapper around [`CalRgbParams`] that emits the
/// inline `[/CalRGB <<…>>]` array into the document's colour-space chunk
/// (ISO 32000-2 §8.6.5.6).
///
/// Hashed and compared by params, so two paints sharing the same
/// dictionary reuse the same `/CS<n>` resource entry.
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub(crate) struct CalRgbColorSpace(pub(crate) CalRgbParams);

/// Colour-space-resource wrapper around [`CalGrayParams`] that emits the
/// inline `[/CalGray <<…>>]` array (ISO 32000-2 §8.6.5.5).
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub(crate) struct CalGrayColorSpace(pub(crate) CalGrayParams);

/// Colour-space-resource wrapper around [`LabParams`] that emits the
/// inline `[/Lab <<…>>]` array (ISO 32000-2 §8.6.5.4).
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub(crate) struct LabColorSpace(pub(crate) LabParams);

impl Cacheable for CalRgbColorSpace {
    fn serialize(
        self,
        _sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    ) {
        let chunk = &mut chunk_container.non_stream.color_spaces;
        let mut array = chunk.indirect(root_ref).array();
        array.item(Name(b"CalRGB"));

        let mut dict = array.push().dict();
        dict.insert(Name(b"WhitePoint"))
            .array()
            .items(self.0.white_point);
        if let Some(black_point) = self.0.black_point {
            dict.insert(Name(b"BlackPoint")).array().items(black_point);
        }
        if let Some(gamma) = self.0.gamma {
            dict.insert(Name(b"Gamma")).array().items(gamma);
        }
        if let Some(matrix) = self.0.matrix {
            dict.insert(Name(b"Matrix")).array().items(matrix);
        }
        dict.finish();
        array.finish();
    }
}

impl Resourceable for CalRgbColorSpace {
    type Resource = resource::ColorSpace;
}

impl Cacheable for CalGrayColorSpace {
    fn serialize(
        self,
        _sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    ) {
        let chunk = &mut chunk_container.non_stream.color_spaces;
        let mut array = chunk.indirect(root_ref).array();
        array.item(Name(b"CalGray"));

        let mut dict = array.push().dict();
        dict.insert(Name(b"WhitePoint"))
            .array()
            .items(self.0.white_point);
        if let Some(black_point) = self.0.black_point {
            dict.insert(Name(b"BlackPoint")).array().items(black_point);
        }
        if let Some(gamma) = self.0.gamma {
            dict.pair(Name(b"Gamma"), gamma);
        }
        dict.finish();
        array.finish();
    }
}

impl Resourceable for CalGrayColorSpace {
    type Resource = resource::ColorSpace;
}

impl Cacheable for LabColorSpace {
    fn serialize(
        self,
        _sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    ) {
        let chunk = &mut chunk_container.non_stream.color_spaces;
        let mut array = chunk.indirect(root_ref).array();
        array.item(Name(b"Lab"));

        let mut dict = array.push().dict();
        dict.insert(Name(b"WhitePoint"))
            .array()
            .items(self.0.white_point);
        if let Some(black_point) = self.0.black_point {
            dict.insert(Name(b"BlackPoint")).array().items(black_point);
        }
        if let Some(range) = self.0.range {
            dict.insert(Name(b"Range")).array().items(range);
        }
        dict.finish();
        array.finish();
    }
}

impl Resourceable for LabColorSpace {
    type Resource = resource::ColorSpace;
}

impl From<CieBasedColorSpace> for ColorSpace {
    fn from(value: CieBasedColorSpace) -> Self {
        Self::CieBased(value)
    }
}

impl From<CieBasedColorSpace> for RegularColorSpace {
    fn from(value: CieBasedColorSpace) -> Self {
        Self::CieBased(value)
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub(crate) enum SpecialColorSpace {
    /// A Separation color space with its colorant and fallback.
    Separation(separation::SeparationSpace),
    /// A DeviceN colour space (ISO 32000-2 §8.6.6.5) with its
    /// colorant list, alternate process space, and tint transform.
    DeviceN(devicen::DeviceNSpace),
}

impl From<SpecialColorSpace> for ColorSpace {
    fn from(value: SpecialColorSpace) -> Self {
        Self::Special(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Pass-through variants ---------------------------------------

    #[test]
    fn auto_passes_through_rgb() {
        let c: Color = rgb::Color::new(200, 100, 50).into();
        assert_eq!(c.clone().project(ColourConversion::Auto), c);
    }

    #[test]
    fn none_passes_through_cmyk() {
        let c: Color = cmyk::Color::new(10, 20, 30, 40).into();
        assert_eq!(c.clone().project(ColourConversion::None), c);
    }

    #[test]
    fn content_only_passes_through_luma() {
        let c: Color = luma::Color::new(128).into();
        assert_eq!(c.clone().project(ColourConversion::ContentOnly), c);
    }

    #[test]
    fn force_spot_passes_through_separation() {
        let space = separation::SeparationSpace::new(
            separation::SeparationColorant::Custom("PANTONE 185 C".into()),
            rgb::Color::new(255, 0, 0).into(),
        );
        let c: Color = separation::Color::new(128, space).into();
        assert_eq!(c.clone().project(ColourConversion::ForceSpot), c);
    }

    // --- ForceRgb ---------------------------------------------------

    #[test]
    fn force_rgb_keeps_rgb() {
        let c: Color = rgb::Color::new(123, 45, 67).into();
        assert_eq!(c.clone().project(ColourConversion::ForceRgb), c);
    }

    #[test]
    fn force_rgb_from_luma() {
        let c: Color = luma::Color::new(128).into();
        let expected: Color = rgb::Color::new(128, 128, 128).into();
        assert_eq!(c.project(ColourConversion::ForceRgb), expected);
    }

    #[test]
    fn force_rgb_from_cmyk_pure_red() {
        // CMYK red = (0, 255, 255, 0) -> RGB red.
        let c: Color = cmyk::Color::new(0, 255, 255, 0).into();
        let projected = c.project(ColourConversion::ForceRgb);
        let Color::Regular(RegularColor::Rgb(rgb_out)) = projected else {
            panic!("expected RGB projection, got {projected:?}");
        };
        assert_eq!(rgb_out.0, 255);
        assert_eq!(rgb_out.1, 0);
        assert_eq!(rgb_out.2, 0);
    }

    #[test]
    fn force_rgb_from_cmyk_pure_black() {
        let c: Color = cmyk::Color::new(0, 0, 0, 255).into();
        let projected = c.project(ColourConversion::ForceRgb);
        let Color::Regular(RegularColor::Rgb(rgb_out)) = projected else {
            panic!("expected RGB projection, got {projected:?}");
        };
        assert_eq!(rgb_out, rgb::Color::new(0, 0, 0));
    }

    // --- ForceCmyk --------------------------------------------------

    #[test]
    fn force_cmyk_keeps_cmyk() {
        let c: Color = cmyk::Color::new(50, 100, 150, 200).into();
        assert_eq!(c.clone().project(ColourConversion::ForceCmyk), c);
    }

    #[test]
    fn force_cmyk_from_rgb_pure_red() {
        // RGB (255, 0, 0): max = 1.0, k = 0.0, c = 0, m = 1, y = 1.
        let c: Color = rgb::Color::new(255, 0, 0).into();
        let projected = c.project(ColourConversion::ForceCmyk);
        let Color::Regular(RegularColor::Cmyk(out)) = projected else {
            panic!("expected CMYK projection, got {projected:?}");
        };
        assert_eq!(out.0, 0);
        assert_eq!(out.1, 255);
        assert_eq!(out.2, 255);
        assert_eq!(out.3, 0);
    }

    #[test]
    fn force_cmyk_from_rgb_pure_black() {
        // RGB (0, 0, 0): max = 0, k = 1; special-case c = m = y = 0.
        let c: Color = rgb::Color::new(0, 0, 0).into();
        let projected = c.project(ColourConversion::ForceCmyk);
        let Color::Regular(RegularColor::Cmyk(out)) = projected else {
            panic!("expected CMYK projection, got {projected:?}");
        };
        assert_eq!(out, cmyk::Color::new(0, 0, 0, 255));
    }

    #[test]
    fn force_cmyk_from_luma_half() {
        // L = 128/255 ~= 0.502, k = 1 - L ~= 0.498, c = m = y = 0.
        let c: Color = luma::Color::new(128).into();
        let projected = c.project(ColourConversion::ForceCmyk);
        let Color::Regular(RegularColor::Cmyk(out)) = projected else {
            panic!("expected CMYK projection, got {projected:?}");
        };
        assert_eq!(out.0, 0);
        assert_eq!(out.1, 0);
        assert_eq!(out.2, 0);
        assert_eq!(out.3, 127); // 255 - 128 = 127.
    }

    // --- ForceGrey --------------------------------------------------

    #[test]
    fn force_grey_keeps_luma() {
        let c: Color = luma::Color::new(64).into();
        assert_eq!(c.clone().project(ColourConversion::ForceGrey), c);
    }

    #[test]
    fn force_grey_from_rgb_white() {
        let c: Color = rgb::Color::new(255, 255, 255).into();
        let projected = c.project(ColourConversion::ForceGrey);
        let Color::Regular(RegularColor::Luma(out)) = projected else {
            panic!("expected Luma projection, got {projected:?}");
        };
        assert_eq!(out, luma::Color::new(255));
    }

    #[test]
    fn force_grey_from_rgb_red_rec709() {
        // Rec. 709 Y for pure red = 0.2126 -> 54.213, round to 54.
        let c: Color = rgb::Color::new(255, 0, 0).into();
        let projected = c.project(ColourConversion::ForceGrey);
        let Color::Regular(RegularColor::Luma(out)) = projected else {
            panic!("expected Luma projection, got {projected:?}");
        };
        assert_eq!(out.0, 54);
    }

    #[test]
    fn force_grey_from_cmyk_pure_red() {
        // CMYK red -> RGB (255, 0, 0) -> Y = 54.
        let c: Color = cmyk::Color::new(0, 255, 255, 0).into();
        let projected = c.project(ColourConversion::ForceGrey);
        let Color::Regular(RegularColor::Luma(out)) = projected else {
            panic!("expected Luma projection, got {projected:?}");
        };
        assert_eq!(out.0, 54);
    }

    // --- Separation recursion ---------------------------------------

    #[test]
    fn force_rgb_from_separation_recurses_on_fallback() {
        // Half-tint of an RGB-red fallback -> RGB (128, 0, 0).
        // 128/255 = 0.502; 0.502 * 255 = 128 (rounded).
        let space = separation::SeparationSpace::new(
            separation::SeparationColorant::Custom("PANTONE 185 C".into()),
            rgb::Color::new(255, 0, 0).into(),
        );
        let c: Color = separation::Color::new(128, space).into();
        let projected = c.project(ColourConversion::ForceRgb);
        let Color::Regular(RegularColor::Rgb(out)) = projected else {
            panic!("expected RGB projection, got {projected:?}");
        };
        assert_eq!(out.0, 128);
        assert_eq!(out.1, 0);
        assert_eq!(out.2, 0);
    }

    // --- Default ----------------------------------------------------

    #[test]
    fn colour_conversion_default_is_auto() {
        assert_eq!(ColourConversion::default(), ColourConversion::Auto);
    }

    // --- DeviceN constructor invariants -----------------------------

    #[test]
    fn devicen_constructs_two_colorant_cmyk_alt() {
        let space = devicen::DeviceNSpace::new(
            vec!["PANTONE 185 C".to_string(), "PANTONE 286 C".to_string()],
            cmyk::Color::new(0, 0, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![vec![0.0, 1.0, 1.0, 0.0], vec![1.0, 1.0, 0.0, 0.0]],
            },
        );
        assert!(space.is_some());
        assert_eq!(space.unwrap().colorant_count(), 2);
    }

    #[test]
    fn devicen_three_colorant_rgb_alt_n_equals_three() {
        let space = devicen::DeviceNSpace::new(
            vec![
                "Spot1".to_string(),
                "Spot2".to_string(),
                "Spot3".to_string(),
            ],
            rgb::Color::new(0, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![
                    vec![1.0, 0.0, 0.0],
                    vec![0.0, 1.0, 0.0],
                    vec![0.0, 0.0, 1.0],
                ],
            },
        )
        .expect("three-colorant space should construct");
        assert_eq!(space.colorant_count(), 3);
        assert_eq!(space.colorants(), &["Spot1", "Spot2", "Spot3"]);
    }

    #[test]
    fn devicen_rejects_empty_colorants() {
        let space = devicen::DeviceNSpace::new(
            vec![],
            cmyk::Color::new(0, 0, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![],
            },
        );
        assert!(space.is_none());
    }

    #[test]
    fn devicen_rejects_colorant_arity_mismatch() {
        // Two names but one per-colorant component vector.
        let space = devicen::DeviceNSpace::new(
            vec!["A".to_string(), "B".to_string()],
            cmyk::Color::new(0, 0, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![vec![1.0, 0.0, 0.0, 0.0]],
            },
        );
        assert!(space.is_none());
    }

    #[test]
    fn devicen_rejects_alt_channel_mismatch() {
        // Alt is CMYK (4 channels) but inner data has 3 components.
        let space = devicen::DeviceNSpace::new(
            vec!["A".to_string()],
            cmyk::Color::new(0, 0, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![vec![1.0, 0.0, 0.0]],
            },
        );
        assert!(space.is_none());
    }

    #[test]
    fn devicen_color_arity_must_match_space() {
        let space = devicen::DeviceNSpace::new(
            vec!["A".to_string(), "B".to_string()],
            cmyk::Color::new(0, 0, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]],
            },
        )
        .expect("space should construct");
        assert!(devicen::Color::new(vec![0.5, 0.25], space.clone()).is_some());
        assert!(devicen::Color::new(vec![0.5], space.clone()).is_none());
        assert!(devicen::Color::new(vec![0.5, 0.25, 0.1], space).is_none());
    }

    // --- DeviceN integrates with the `Color` / `SpecialColor` chain --

    #[test]
    fn devicen_color_lifts_through_special_into_color() {
        let space = devicen::DeviceNSpace::new(
            vec!["Spot".to_string()],
            rgb::Color::new(255, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![vec![1.0, 0.0, 0.0]],
            },
        )
        .unwrap();
        let dn = devicen::Color::new(vec![0.5], space).unwrap();
        let lifted: Color = dn.clone().into();
        assert_eq!(lifted, Color::Special(SpecialColor::DeviceN(dn)));
    }

    #[test]
    fn devicen_to_pdf_color_returns_all_tints() {
        let space = devicen::DeviceNSpace::new(
            vec!["A".to_string(), "B".to_string()],
            cmyk::Color::new(0, 0, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]],
            },
        )
        .unwrap();
        let dn: Color = devicen::Color::new(vec![0.25, 0.75], space).unwrap().into();
        let components = dn.to_pdf_color();
        assert_eq!(components.len(), 2);
        assert!((components[0] - 0.25).abs() < f32::EPSILON);
        assert!((components[1] - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn force_rgb_from_devicen_projects_alt_space() {
        // Alt is pure red; ForceRgb should pass through the alt's
        // ForceRgb projection (which keeps RGB unchanged).
        let space = devicen::DeviceNSpace::new(
            vec!["Spot".to_string()],
            rgb::Color::new(255, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![vec![1.0, 0.0, 0.0]],
            },
        )
        .unwrap();
        let c: Color = devicen::Color::new(vec![0.5], space).unwrap().into();
        let projected = c.project(ColourConversion::ForceRgb);
        let Color::Regular(RegularColor::Rgb(out)) = projected else {
            panic!("expected RGB projection, got {projected:?}");
        };
        assert_eq!(out, rgb::Color::new(255, 0, 0));
    }

    #[test]
    fn force_spot_passes_through_devicen() {
        let space = devicen::DeviceNSpace::new(
            vec!["Spot".to_string()],
            cmyk::Color::new(0, 0, 0, 0).into(),
            devicen::TintTransform::Linear {
                per_colorant_components: vec![vec![0.0, 1.0, 0.0, 0.0]],
            },
        )
        .unwrap();
        let c: Color = devicen::Color::new(vec![0.5], space).unwrap().into();
        assert_eq!(c.clone().project(ColourConversion::ForceSpot), c);
    }

    // --- Calibrated CIE-based spaces: end-to-end emission -----------
    //
    // Each test fills a path with a CalRGB / CalGray / Lab colour and
    // asserts the inline colour-space array name (`/CalRGB` /
    // `/CalGray` / `/Lab`) lands in the serialised PDF byte stream.
    // The tests catch a routing regression — the `Cacheable` impl on
    // `Cal{Rgb,Gray}ColorSpace` / `LabColorSpace`, the
    // `register_colorspace` dispatch, or the `RegularColor::color_space`
    // mapping — without coupling to the rest of the dictionary layout.

    use crate::document::Document;
    use crate::geom::PathBuilder;
    use crate::graphics::paint::{Fill, FillRule};
    use crate::num::NormalizedF32;
    use crate::page::PageSettings;
    use crate::SerializeSettings;

    fn finish_with_fill(fill: Fill) -> Vec<u8> {
        let settings = SerializeSettings {
            pretty: false,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);
        let mut page = document.start_page_with(PageSettings::from_wh(100.0, 100.0).unwrap());
        let mut surface = page.surface();
        surface.set_fill(Some(fill));

        let mut pb = PathBuilder::new();
        pb.move_to(10.0, 10.0);
        pb.line_to(90.0, 10.0);
        pb.line_to(90.0, 90.0);
        pb.line_to(10.0, 90.0);
        pb.close();
        let path = pb.finish().expect("path should build");
        surface.draw_path(&path);
        surface.finish();
        page.finish();
        document
            .finish()
            .expect("document serialisation should succeed")
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn cal_rgb_colour_space_array_lands_in_pdf() {
        let params = CalRgbParams {
            // D65 white point per ISO 32000-2 §8.6.5.6.
            white_point: [0.9505, 1.0, 1.089],
            black_point: Some([0.0, 0.0, 0.0]),
            gamma: Some([2.2, 2.2, 2.2]),
            matrix: None,
        };
        let color: Color = RegularColor::cal_rgb(params, [0.5, 0.25, 0.75]).into();
        let pdf = finish_with_fill(Fill {
            paint: color.into(),
            opacity: NormalizedF32::ONE,
            rule: FillRule::default(),
        });
        assert!(
            contains(&pdf, b"/CalRGB"),
            "PDF should carry an inline /CalRGB colour-space array"
        );
        assert!(
            contains(&pdf, b"/WhitePoint"),
            "PDF should carry the CalRGB /WhitePoint key"
        );
    }

    #[test]
    fn cal_gray_colour_space_array_lands_in_pdf() {
        let params = CalGrayParams {
            white_point: [0.9505, 1.0, 1.089],
            black_point: None,
            gamma: Some(2.2),
        };
        let color: Color = RegularColor::cal_gray(params, 0.5).into();
        let pdf = finish_with_fill(Fill {
            paint: color.into(),
            opacity: NormalizedF32::ONE,
            rule: FillRule::default(),
        });
        assert!(
            contains(&pdf, b"/CalGray"),
            "PDF should carry an inline /CalGray colour-space array"
        );
        assert!(
            contains(&pdf, b"/Gamma"),
            "PDF should carry the CalGray /Gamma key"
        );
    }

    #[test]
    fn lab_colour_space_array_lands_in_pdf() {
        let params = LabParams {
            white_point: [0.9505, 1.0, 1.089],
            black_point: None,
            range: Some([-128.0, 127.0, -128.0, 127.0]),
        };
        // L = 50 (mid-grey), a = 20, b = -30.
        let color: Color = RegularColor::lab(params, [50.0, 20.0, -30.0]).into();
        let pdf = finish_with_fill(Fill {
            paint: color.into(),
            opacity: NormalizedF32::ONE,
            rule: FillRule::default(),
        });
        assert!(
            contains(&pdf, b"/Lab"),
            "PDF should carry an inline /Lab colour-space array"
        );
        assert!(
            contains(&pdf, b"/Range"),
            "PDF should carry the Lab /Range key"
        );
    }
}
