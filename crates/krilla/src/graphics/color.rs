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

use crate::configure::ValidationError;
use crate::graphics::icc::ICCBasedColorSpace;
use crate::serialize::SerializeContext;

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
#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub enum RegularColor {
    /// An RGB-based color.
    Rgb(rgb::Color),
    /// A luma-based color.
    Luma(luma::Color),
    /// A device CMYK color.
    Cmyk(cmyk::Color),
}

/// A special color space color.
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum SpecialColor {
    /// A separation color.
    Separation(separation::Color),
}

impl Color {
    pub(crate) fn to_pdf_color(&self) -> Vec<f32> {
        match self {
            Color::Regular(RegularColor::Rgb(rgb)) => rgb.to_pdf_color().to_vec(),
            Color::Regular(RegularColor::Luma(l)) => vec![l.to_pdf_color()],
            Color::Regular(RegularColor::Cmyk(cmyk)) => cmyk.to_pdf_color().to_vec(),
            Color::Special(SpecialColor::Separation(spot)) => vec![spot.to_pdf_color()],
        }
    }

    pub(crate) fn color_space(&self, sc: &mut SerializeContext) -> ColorSpace {
        match self {
            Color::Regular(c) => c.color_space(sc).into(),
            Color::Special(c) => c.color_space().into(),
        }
    }

    /// Convert a color to a regular color for use with constructs like tags or
    /// annotations that don't support special color spaces
    pub(crate) fn to_regular(&self) -> RegularColor {
        match self {
            Color::Regular(c) => *c,
            Color::Special(SpecialColor::Separation(c)) => c.space.fallback,
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
                Color::Regular(RegularColor::Luma(l)) => {
                    rgb::Color::new(l.0, l.0, l.0).into()
                }
                Color::Special(SpecialColor::Separation(spot)) => {
                    separation_to_regular(&spot)
                        .into_color()
                        .project(ColourConversion::ForceRgb)
                }
            },
            ColourConversion::ForceCmyk => match self {
                Color::Regular(RegularColor::Cmyk(_)) => self,
                Color::Regular(RegularColor::Rgb(r)) => rgb_to_cmyk(r).into(),
                Color::Regular(RegularColor::Luma(l)) => {
                    // Pure-K projection: c = m = y = 0, k = 1 - L.
                    let k = 255u8.saturating_sub(l.0);
                    cmyk::Color::new(0, 0, 0, k).into()
                }
                Color::Special(SpecialColor::Separation(spot)) => {
                    separation_to_regular(&spot)
                        .into_color()
                        .project(ColourConversion::ForceCmyk)
                }
            },
            ColourConversion::ForceGrey => match self {
                Color::Regular(RegularColor::Luma(_)) => self,
                Color::Regular(RegularColor::Rgb(r)) => rgb_to_grey(r).into(),
                Color::Regular(RegularColor::Cmyk(c)) => cmyk_to_grey(c).into(),
                Color::Special(SpecialColor::Separation(spot)) => {
                    separation_to_regular(&spot)
                        .into_color()
                        .project(ColourConversion::ForceGrey)
                }
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
    match spot.space.fallback {
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
        RegularColor::Luma(c) => {
            luma::Color::new(unit_to_u8(u8_to_unit(c.0) * tint)).into()
        }
    }
}

impl RegularColor {
    pub(crate) fn color_space(&self, sc: &mut SerializeContext) -> RegularColorSpace {
        match self {
            Self::Rgb(r) => {
                if sc.serialize_settings().validator().requires_cmyk_only() {
                    sc.register_validation_error(ValidationError::ContainsRgb(sc.location));
                }
                r.color_space(sc.serialize_settings().no_device_cs)
            }
            Self::Luma(_) => luma::color_space(sc.serialize_settings().no_device_cs),
            Self::Cmyk(_) => match cmyk::color_space(&sc.serialize_settings()) {
                None => {
                    sc.register_validation_error(ValidationError::MissingCMYKProfile);
                    DeviceColorSpace::Cmyk.into()
                }
                Some(cs) => cs,
            },
        }
    }

    /// Return the current color as RGB for use with colored glyphs (SVG and
    /// COLR).
    pub(crate) fn as_rgb(self) -> Option<rgb::Color> {
        Some(match self {
            Self::Rgb(r) => r,
            Self::Luma(l) => rgb::Color::new(l.0, l.0, l.0),
            Self::Cmyk(_) => return None,
        })
    }

    /// Returns true if this is a subtractive color space (CMYK), false otherwise (RGB, Luma).
    /// Used for determining the correct tint transform behavior in Separation color spaces.
    pub(crate) fn is_subtractive(self) -> bool {
        matches!(self, Self::Cmyk(_))
    }
}

impl SpecialColor {
    pub(crate) fn color_space(&self) -> SpecialColorSpace {
        match self {
            Self::Separation(spot) => spot.color_space().into(),
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

/// Colour-conversion policy applied to every fill, stroke, and glyph
/// paint before content-stream emission.
///
/// The variant is read once per paint dispatch from
/// [`SerializeSettings::colour_conversion`]; `Auto` (the default) and
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
}
