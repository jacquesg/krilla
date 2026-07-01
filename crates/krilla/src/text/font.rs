use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use skrifa::instance::{Location, LocationRef, Size};
use skrifa::metrics::GlyphMetrics;
use skrifa::raw::types::NameId;
use skrifa::raw::TableProvider;
use skrifa::{FontRef, MetadataProvider, OutlineGlyphCollection};
use tiny_skia_path::FiniteF32;
use yoke::{Yoke, Yokeable};

use crate::geom::Rect;
use crate::text::GlyphId;
use crate::util::Prehashed;
use crate::Data;

/// An OpenType font. Can be a TrueType, OpenType font or a TrueType collection.
/// It holds a reference to the underlying data as well as some basic information
/// about the font.
///
/// Cloning and hashing this type is cheap.
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct Font(Arc<Prehashed<Repr>>);

impl Font {
    /// Create a new font from some data. If you want to create a variable font at a specific
    /// location, use [`Font::new_variable`] instead. To select a non-default CPAL palette
    /// for COLR-coloured glyphs, use [`Font::new_with_palette`] or
    /// [`Font::new_variable_with_palette`].
    ///
    /// The `index` indicates the index that should be
    /// associated with this font for TrueType collections, otherwise this value should be
    /// set to 0.
    ///
    /// Returns `None` if the index is invalid or the font couldn't be read.
    pub fn new(data: Data, index: u32) -> Option<Self> {
        Self::new_variable_with_palette(data, index, &[], 0)
    }

    /// Like [`Font::new`], creates a new font from some data, but allows you to specify
    /// variation coordinates in case the font is variable.
    pub fn new_variable(data: Data, index: u32, variation_coords: &[(Tag, f32)]) -> Option<Self> {
        Self::new_variable_with_palette(data, index, variation_coords, 0)
    }

    /// Like [`Font::new`], but selects a non-default CPAL palette for
    /// COLR-coloured glyph rendering.
    ///
    /// `palette_base` is the zero-based index into the font's `CPAL`
    /// table — palette `0` is the default. When the supplied index is
    /// out of range for the font's palette count, krilla falls back to
    /// palette `0` at draw time; fonts without a `CPAL` table ignore
    /// the parameter entirely.
    ///
    /// Moegoe constructs a separate `Font` instance per cascade-resolved
    /// `font-palette` value so the per-call surface (`Surface::draw_glyphs`)
    /// stays palette-free.
    pub fn new_with_palette(data: Data, index: u32, palette_base: u16) -> Option<Self> {
        Self::new_variable_with_palette(data, index, &[], palette_base)
    }

    /// Combined constructor: variable-font location + CPAL palette
    /// selection. See [`Font::new_variable`] and
    /// [`Font::new_with_palette`] for the per-parameter semantics.
    pub fn new_variable_with_palette(
        data: Data,
        index: u32,
        variation_coords: &[(Tag, f32)],
        palette_base: u16,
    ) -> Option<Self> {
        let font_info = FontInfo::new(data.as_ref(), index, variation_coords, palette_base)?;

        Font::new_with_info(data.clone(), Arc::new(font_info))
    }

    pub(crate) fn new_with_info(data: Data, font_info: Arc<FontInfo>) -> Option<Self> {
        let yoke_data = YokeData {
            data: data.0.clone(),
            location: font_info.location.clone(),
        };

        let font_ref_yoke = Yoke::<FontRefYoke<'static>, Box<YokeData>>::attach_to_cart(
            Box::new(yoke_data),
            |data| {
                let font_ref =
                    FontRef::from_index(data.data.as_ref().as_ref(), font_info.index).unwrap();
                FontRefYoke {
                    font_ref: font_ref.clone(),
                    glyph_metrics: font_ref.glyph_metrics(Size::unscaled(), &data.location),
                    outline_glyphs: font_ref.outline_glyphs(),
                }
            },
        );

        Some(Font(Arc::new(Prehashed::new(Repr {
            font_data: data,
            font_ref_yoke,
            font_info,
        }))))
    }

    pub(crate) fn postscript_name(&self) -> Option<&str> {
        self.0.font_info.postscript_name.as_deref()
    }

    /// Return the index of the font.
    pub(crate) fn index(&self) -> u32 {
        self.font_info().index
    }

    pub(crate) fn font_info(&self) -> Arc<FontInfo> {
        self.0.font_info.clone()
    }

    pub(crate) fn variation_coordinates(&self) -> &[(Tag, FiniteF32)] {
        &self.0.font_info.var_coords
    }

    pub(crate) fn cap_height(&self) -> Option<f32> {
        self.0.font_info.cap_height.map(|n| n.get())
    }

    pub(crate) fn ascent(&self) -> f32 {
        self.0.font_info.ascent.get()
    }

    pub(crate) fn weight(&self) -> f32 {
        self.0.font_info.weight.get()
    }

    pub(crate) fn stretch(&self) -> f32 {
        self.0.font_info.stretch.get()
    }

    pub(crate) fn descent(&self) -> f32 {
        self.0.font_info.descent.get()
    }

    pub(crate) fn num_glyphs(&self) -> u32 {
        self.0.font_info.num_glyphs
    }

    pub(crate) fn is_monospaced(&self) -> bool {
        self.0.font_info.is_monospaced
    }

    pub(crate) fn italic_angle(&self) -> f32 {
        self.0.font_info.italic_angle.get()
    }

    /// The units per em of the font.
    pub fn units_per_em(&self) -> f32 {
        self.0.font_info.units_per_em as f32
    }

    /// The CPAL palette index this font instance reads colour records
    /// from when drawing COLR glyphs. Returns `0` when the embedder
    /// constructed the font without an explicit palette selection or
    /// when the font carries no `CPAL` table.
    pub fn palette_base(&self) -> u16 {
        self.0.font_info.palette_base
    }

    pub(crate) fn bbox(&self) -> Rect {
        self.0.font_info.global_bbox
    }

    pub(crate) fn location_ref(&self) -> LocationRef<'_> {
        (&self.0.font_info.location).into()
    }

    pub(crate) fn font_ref(&self) -> &FontRef<'_> {
        &self.0.font_ref_yoke.get().font_ref
    }

    pub(crate) fn glyph_metrics(&self) -> &GlyphMetrics<'_> {
        &self.0.font_ref_yoke.get().glyph_metrics
    }

    pub(crate) fn outline_glyphs(&self) -> &OutlineGlyphCollection<'_> {
        &self.0.font_ref_yoke.get().outline_glyphs
    }

    pub(crate) fn font_data(&self) -> Data {
        self.0.font_data.clone()
    }

    #[inline]
    pub(crate) fn advance_width(&self, glyph_id: GlyphId) -> Option<f32> {
        self.glyph_metrics().advance_width(glyph_id.to_skrifa())
    }
}

/// A 4-byte OpenType tag.
#[derive(Copy, Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Tag([u8; 4]);

impl Tag {
    /// Create a new tag.
    pub fn new(tag: &[u8; 4]) -> Self {
        Self(*tag)
    }

    /// Try to create a new tag from a string.
    ///
    /// Return `None` if the string is not 4 bytes in size.
    pub fn try_from_str(s: &str) -> Option<Self> {
        let tag: [u8; 4] = s.as_bytes().try_into().ok()?;

        Some(Self(tag))
    }

    /// Return the value of the tag.
    pub fn get(&self) -> &[u8; 4] {
        &self.0
    }
}

impl Debug for Font {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Font {{..}}")
    }
}

struct YokeData {
    data: Arc<dyn AsRef<[u8]> + Send + Sync>,
    location: Location,
}

impl Deref for YokeData {
    type Target = YokeData;

    fn deref(&self) -> &Self::Target {
        self
    }
}

/// `FontInfo` holds basic information about the font which is necessary
/// to distinguish a `Font` object from others. The `Hash` implementation
/// of the `Font` struct solely depends on its `FontInfo` object. The reason
/// we do this is to avoid hashing the whole font, which can be dozens of megabytes.
/// Instead, we parse the most basic information as well as additional distinguishing
/// information, such as the font name and the checksum, and has this instead.
/// This is much faster, and since we also include the checksum, the odds of two
/// different fonts ending up with the same hash is pretty much zero.
#[derive(Debug, Hash, Eq, PartialEq)]
pub(crate) struct FontInfo {
    index: u32,
    checksum: u32,
    data_len: usize,
    // `location` is derived from `var_coords`, but we need to store `var_coords` it explicitly
    // so we can later pass it to the subsetter.
    var_coords: Vec<(Tag, FiniteF32)>,
    location: Location,
    units_per_em: u16,
    global_bbox: Rect,
    num_glyphs: u32,
    postscript_name: Option<String>,
    ascent: FiniteF32,
    descent: FiniteF32,
    cap_height: Option<FiniteF32>,
    is_monospaced: bool,
    italic_angle: FiniteF32,
    weight: FiniteF32,
    has_glyf: bool,
    has_cff: bool,
    has_cff2: bool,
    stretch: FiniteF32,
    /// CPAL palette index for COLR glyph rendering. Zero is the
    /// default palette per OpenType CPAL §5.7.11.1. Stored on
    /// `FontInfo` so two `Font` instances differing only in palette
    /// selection hash distinctly.
    palette_base: u16,
}

struct Repr {
    font_info: Arc<FontInfo>,
    font_data: Data,
    font_ref_yoke: Yoke<FontRefYoke<'static>, Box<YokeData>>,
}

impl Hash for Repr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // We assume that if the font info is distinct, the font itself is distinct as well. This
        // doesn't have to be the case: while the font does have a checksum, it's "only" a
        // u32. The proper way would be to hash the whole font data, but this is just too expensive.
        // However, the odds of the checksum AND all font metrics (including font name) being the same
        // with the font being different is diminishingly low.
        self.font_info.hash(state);
    }
}

impl FontInfo {
    pub(crate) fn new(
        data: &[u8],
        index: u32,
        var_coords: &[(Tag, f32)],
        palette_base: u16,
    ) -> Option<Self> {
        let font_ref = FontRef::from_index(data, index).ok()?;
        let location = font_ref.axes().location(
            var_coords
                .iter()
                .map(|i| (skrifa::Tag::new(i.0.get()), i.1)),
        );
        let data_len = data.len();
        let checksum = font_ref.head().ok()?.checksum_adjustment();
        let num_glyphs = font_ref.glyph_names().num_glyphs();

        let metrics = font_ref.metrics(Size::unscaled(), &location);
        let os_2 = font_ref.os2().ok();
        let ascent = FiniteF32::new(
            os_2.as_ref()
                .map(|s| s.s_typo_ascender() as f32)
                .unwrap_or(metrics.ascent),
        )?;
        let descent = FiniteF32::new(
            os_2.as_ref()
                .map(|s| s.s_typo_descender() as f32)
                .unwrap_or(metrics.descent),
        )?;
        let is_monospaced = metrics.is_monospace;
        let cap_height = metrics.cap_height.map(|n| FiniteF32::new(n).unwrap());
        let italic_angle = FiniteF32::new(metrics.italic_angle).unwrap();
        let weight = FiniteF32::new(font_ref.attributes().weight.value())?;
        let stretch = FiniteF32::new(font_ref.attributes().stretch.ratio())?;
        let units_per_em = metrics.units_per_em;

        let global_bbox = metrics
            .bounds
            .and_then(|b| Rect::from_ltrb(b.x_min, b.y_min, b.x_max, b.y_max))
            .unwrap_or(Rect::from_xywh(
                0.0,
                0.0,
                units_per_em as f32,
                units_per_em as f32,
            )?);

        let postscript_name = {
            if let Ok(name) = font_ref.name() {
                name.name_record().iter().find_map(|n| {
                    if n.name_id.get() == NameId::POSTSCRIPT_NAME {
                        if let Ok(string) = n.string(name.string_data()) {
                            return Some(string.to_string());
                        }
                    }

                    None
                })
            } else {
                None
            }
        };

        let has_glyf = font_ref.glyf().is_ok();
        let has_cff = font_ref.cff().is_ok();
        let has_cff2 = font_ref.cff2().is_ok();

        Some(FontInfo {
            index,
            data_len,
            checksum,
            var_coords: var_coords
                .iter()
                .map(|v| (v.0, FiniteF32::new(v.1).unwrap_or_default()))
                .collect(),
            location,
            num_glyphs,
            units_per_em,
            postscript_name,
            ascent,
            cap_height,
            has_glyf,
            has_cff,
            has_cff2,
            descent,
            is_monospaced,
            weight,
            stretch,
            italic_angle,
            global_bbox,
            palette_base,
        })
    }

    pub(crate) fn can_be_cid_font(&self) -> bool {
        self.has_cff || self.has_glyf || self.has_cff2
    }
}

/// A yoke so that we can attach a `FontRef` object to the corresponding `Font`,
/// without running into lifetime issues.
#[derive(Yokeable, Clone)]
struct FontRefYoke<'a> {
    pub font_ref: FontRef<'a>,
    pub glyph_metrics: GlyphMetrics<'a>,
    pub outline_glyphs: OutlineGlyphCollection<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Embedded COLR/CPAL test font from the workspace assets — has a
    /// CPAL table with multiple palettes, which is what the palette
    /// support needs to exercise.
    const COLR_TEST_FONT: &[u8] =
        include_bytes!("../../../../assets/fonts/colr_test_glyphs.ttf");

    fn test_font_data() -> Data {
        Data::from(COLR_TEST_FONT.to_vec())
    }

    /// Default constructor reports palette 0.
    #[test]
    fn font_default_palette_base_is_zero() {
        let font = Font::new(test_font_data(), 0).expect("font load");
        assert_eq!(font.palette_base(), 0);
    }

    /// Explicit palette base round-trips through the accessor.
    #[test]
    fn font_palette_base_round_trips() {
        let font = Font::new_with_palette(test_font_data(), 0, 3).expect("font load");
        assert_eq!(font.palette_base(), 3);
    }

    /// Two `Font` instances differing only in the palette selection
    /// hash distinctly — the palette index participates in the
    /// `FontInfo` hash so colour-glyph caching keys correctly.
    #[test]
    fn font_palette_base_participates_in_hash() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        let f0 = Font::new_with_palette(test_font_data(), 0, 0).expect("font load");
        let f1 = Font::new_with_palette(test_font_data(), 0, 1).expect("font load");

        let mut h0 = DefaultHasher::new();
        f0.hash(&mut h0);
        let mut h1 = DefaultHasher::new();
        f1.hash(&mut h1);
        assert_ne!(h0.finish(), h1.finish(), "palette base must affect hash");
    }
}
