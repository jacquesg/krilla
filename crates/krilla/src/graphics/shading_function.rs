use std::hash::{Hash, Hasher};
use std::ops::DerefMut;
use std::sync::Arc;

use bumpalo::Bump;
use pdf_writer::types::{FunctionShadingType, PostScriptOp};
use pdf_writer::{Chunk, Finish, Ref};
use tiny_skia_path::Point;

use crate::chunk_container::ChunkContainer;
use crate::configure::ValidationError;
use crate::geom::{Rect, Transform};
use crate::graphics::color::luma;
use crate::graphics::color::{
    CieBasedColorSpace, Color, ColorSpace, DeviceColorSpace, RegularColor,
};
use crate::graphics::paint::{LinearGradient, RadialGradient, SweepGradient};
use crate::graphics::paint::{SpreadMethod, Stop};
use crate::num::NormalizedF32;
use crate::resource;
use crate::resource::Resourceable;
use crate::serialize::{Cacheable, MaybeDeviceColorSpace, SerializeContext};
use crate::stream::FilterStreamBuilder;
use crate::util::set_colorspace;

#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub(crate) enum GradientType {
    Sweep,
    Linear,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct RadialAxialGradient {
    pub(crate) coords: Vec<f32>,
    pub(crate) shading_type: FunctionShadingType,
    pub(crate) stops: Vec<Stop>,
    pub(crate) anti_alias: bool,
}

impl Eq for RadialAxialGradient {}

impl Hash for RadialAxialGradient {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for el in &self.coords {
            el.to_bits().hash(state);
        }

        self.shading_type.hash(state);
        self.stops.hash(state);
        self.anti_alias.hash(state);
    }
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct PostScriptGradient {
    pub(crate) min: f32,
    pub(crate) max: f32,
    // Only used for sweep gradient
    pub(crate) cx: f32,
    // Only used for sweep gradient
    pub(crate) cy: f32,
    pub(crate) stops: Vec<Stop>,
    pub(crate) domain: Rect,
    pub(crate) spread_method: SpreadMethod,
    pub(crate) gradient_type: GradientType,
    pub(crate) anti_alias: bool,
}

impl Eq for PostScriptGradient {}

impl Hash for PostScriptGradient {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.min.to_bits().hash(state);
        self.max.to_bits().hash(state);
        self.stops.hash(state);
        self.domain.hash(state);
        self.spread_method.hash(state);
        self.gradient_type.hash(state);
        self.anti_alias.hash(state);
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub(crate) enum GradientProperties {
    RadialAxialGradient(RadialAxialGradient),
    PostScriptGradient(PostScriptGradient),
}

impl GradientProperties {
    // Check if the gradient could be encoded as a solid fill instead.
    pub(crate) fn single_stop_color(&self) -> Option<(&Color, NormalizedF32)> {
        match self {
            GradientProperties::RadialAxialGradient(rag) => {
                if rag.stops.len() == 1 {
                    return Some((&rag.stops[0].color, rag.stops[0].opacity));
                }
            }
            GradientProperties::PostScriptGradient(psg) => {
                if psg.stops.len() == 1 {
                    return Some((&psg.stops[0].color, psg.stops[0].opacity));
                }
            }
        }

        None
    }

    /// Whether this gradient has no stops at all. Such a gradient is not
    /// renderable and should be skipped rather than fall through to shading
    /// serialization — the `stops` field on the public gradient structs is
    /// `pub Vec<Stop>`, so an empty vector is constructible by callers.
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            GradientProperties::RadialAxialGradient(rag) => rag.stops.is_empty(),
            GradientProperties::PostScriptGradient(psg) => psg.stops.is_empty(),
        }
    }
}

pub(crate) trait GradientPropertiesExt {
    fn gradient_properties(self, bbox: Rect) -> (GradientProperties, Transform);
}

fn get_expanded_bbox(mut bbox: Rect, shading_transform: Transform) -> Rect {
    // We need to make sure the shading covers the whole bbox of the object after
    // the transform as been applied. In order to know that, we need to calculate the
    // resulting bbox from the inverted transform.
    bbox.expand(&bbox.transform(shading_transform.invert().unwrap()).unwrap());
    bbox
}

/// When writing a PostScript shading, we assume that both points are on a horizontal
/// line. Here, we calculate by how much we need to rotate the second point so that it
/// is horizontal to the first point, as well as the position of the rotated point.
fn get_point_ts(start: Point, end: Point) -> (Transform, f32, f32) {
    let dist = start.distance(end);

    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let angle = dy.atan2(dx).to_degrees();

    (
        Transform::from_rotate_at(angle, start.x, start.y),
        start.x,
        start.x + dist,
    )
}

impl GradientPropertiesExt for LinearGradient {
    fn gradient_properties(self, bbox: Rect) -> (GradientProperties, Transform) {
        if self.spread_method == SpreadMethod::Pad {
            (
                GradientProperties::RadialAxialGradient(RadialAxialGradient {
                    coords: vec![self.x1, self.y1, self.x2, self.y2],
                    shading_type: FunctionShadingType::Axial,
                    stops: self.stops,
                    anti_alias: self.anti_alias,
                }),
                self.transform,
            )
        } else {
            let p1 = Point::from_xy(self.x1, self.y1);
            let p2 = Point::from_xy(self.x2, self.y2);

            let (ts, min, max) = get_point_ts(p1, p2);
            (
                GradientProperties::PostScriptGradient(PostScriptGradient {
                    min,
                    max,
                    cx: 0.0,
                    cy: 0.0,
                    stops: self.stops,
                    domain: get_expanded_bbox(bbox, self.transform.pre_concat(ts)),
                    spread_method: self.spread_method,
                    gradient_type: GradientType::Linear,
                    anti_alias: self.anti_alias,
                }),
                self.transform.pre_concat(ts),
            )
        }
    }
}

impl GradientPropertiesExt for SweepGradient {
    fn gradient_properties(self, bbox: Rect) -> (GradientProperties, Transform) {
        let min = self.start_angle;
        let max = self.end_angle;

        let transform = self.transform;

        (
            GradientProperties::PostScriptGradient(PostScriptGradient {
                min,
                max,
                cx: self.cx,
                cy: self.cy,
                stops: self.stops,
                domain: get_expanded_bbox(bbox, transform),
                spread_method: self.spread_method,
                gradient_type: GradientType::Sweep,
                anti_alias: self.anti_alias,
            }),
            transform,
        )
    }
}

impl GradientPropertiesExt for RadialGradient {
    fn gradient_properties(self, _: Rect) -> (GradientProperties, Transform) {
        // TODO: Support other spread methods
        (
            GradientProperties::RadialAxialGradient(RadialAxialGradient {
                coords: vec![self.fx, self.fy, self.fr, self.cx, self.cy, self.cr],
                shading_type: FunctionShadingType::Radial,
                stops: self.stops,
                anti_alias: self.anti_alias,
            }),
            self.transform,
        )
    }
}

#[derive(Debug, Hash, Eq, PartialEq)]
struct Repr {
    pub(crate) properties: GradientProperties,
    pub(crate) use_opacities: bool,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub(crate) struct ShadingFunction(Arc<Repr>);

impl ShadingFunction {
    pub(crate) fn new(properties: GradientProperties, use_opacities: bool) -> Self {
        Self(Arc::new(Repr {
            properties,
            use_opacities,
        }))
    }
}

impl Cacheable for ShadingFunction {
    fn serialize(
        self,
        sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    ) {
        let mut stream_chunk = sc.new_chunk();

        match &self.0.properties {
            GradientProperties::RadialAxialGradient(rag) => {
                let stops = sanitize_gradient_stops(&rag.stops, sc);
                let shading_cs =
                    shading_color_space(sc, rag.stops[0].color.clone(), self.0.use_opacities);
                let registered_cs = sc.register_colorspace(chunk_container, shading_cs);
                let chunk = &mut chunk_container.non_stream.shading_functions;
                serialize_axial_radial_shading(
                    sc,
                    chunk,
                    root_ref,
                    rag,
                    &stops,
                    self.0.use_opacities,
                    registered_cs,
                )
            }
            GradientProperties::PostScriptGradient(psg) => {
                sc.register_validation_error(ValidationError::ContainsPostScript(sc.location));
                let stops = sanitize_gradient_stops(&psg.stops, sc);
                let shading_cs =
                    shading_color_space(sc, psg.stops[0].color.clone(), self.0.use_opacities);
                let registered_cs = sc.register_colorspace(chunk_container, shading_cs);
                let chunk = &mut chunk_container.non_stream.shading_functions;
                serialize_postscript_shading(
                    sc,
                    chunk,
                    &mut stream_chunk,
                    root_ref,
                    psg,
                    &stops,
                    self.0.use_opacities,
                    registered_cs,
                )
            }
        }

        // Note: The stream chunk might be empty.
        chunk_container.streams.shading_functions.push(stream_chunk);
    }
}

impl Resourceable for ShadingFunction {
    type Resource = resource::Shading;
}

fn shading_color_space(sc: &mut SerializeContext, color: Color, use_opacities: bool) -> ColorSpace {
    if use_opacities {
        luma::color_space(sc.serialize_settings().no_device_cs).into()
    } else {
        color.color_space(sc)
    }
}

/// Normalize gradient stops so they all share a single color space.
///
/// All stops are coerced to the first stop's color space; a stop whose colour
/// resolves to a different colour *model* (see [`same_gradient_color_space`])
/// has its color replaced by the first stop's color, and
/// [`ValidationError::MixedGradientColorSpaces`] is registered. The first stop
/// is left untouched, so the shading color space (computed separately from
/// `stops[0]`) is unaffected.
///
/// Returns an empty `Vec` if `stops` is empty. An empty `stops` vector is
/// permitted by the public `LinearGradient`/`RadialGradient`/`SweepGradient`
/// structs, but callers guard against it via [`GradientProperties::is_empty`]
/// before a shading is ever created.
fn sanitize_gradient_stops(stops: &[Stop], sc: &mut SerializeContext) -> Vec<Stop> {
    let Some((first, rest)) = stops.split_first() else {
        return Vec::new();
    };

    let first_color_space = first.color.color_space(sc);
    let mut sanitized = Vec::with_capacity(stops.len());
    let mut mixed_color_spaces = false;

    sanitized.push(first.clone());

    for stop in rest {
        if same_gradient_color_space(&stop.color.color_space(sc), &first_color_space) {
            sanitized.push(stop.clone());
        } else {
            mixed_color_spaces = true;
            let mut sanitized_stop = stop.clone();
            sanitized_stop.color = first.color.clone();
            sanitized.push(sanitized_stop);
        }
    }

    if mixed_color_spaces {
        sc.register_validation_error(ValidationError::MixedGradientColorSpaces(sc.location));
    }

    sanitized
}

/// Whether two resolved colour spaces denote the same colour *model* for the
/// single-`/ColorSpace` requirement of a shading dictionary (ISO 32000-2
/// §8.7.4.3, Table 77): all of a shading's colour values are expressed in one
/// colour space.
///
/// [`Color::color_space`] is value-dependent. With `preserve_black` a pure-black
/// `rgb(0, 0, 0)` / `cmyk(0, 0, 0, 1)` stop resolves to its device spelling
/// (`DeviceRGB` / `DeviceCMYK`), whereas its non-black neighbours resolve to the
/// CIE-based spelling (sRGB / an ICC CMYK space) under `no_device_cs`. Those
/// spellings share a component layout and register as one shading colour space,
/// so they must not be read as a genuine mix. `no_device_cs` is document-wide,
/// so this per-paint black short-circuit is the only intra-model split; genuine
/// cross-model mixes (RGB vs CMYK, sRGB vs Lab, sRGB vs a wide-gamut ICC RGB)
/// still compare unequal.
fn same_gradient_color_space(a: &ColorSpace, b: &ColorSpace) -> bool {
    // Collapse the device and CIE-based spelling of each base device model onto
    // a single canonical spelling; anything else keeps its exact identity and
    // is compared exactly by the `a == b` short-circuit.
    fn canonical_base_model(cs: &ColorSpace) -> Option<DeviceColorSpace> {
        match cs {
            ColorSpace::Device(d) => Some(d.clone()),
            ColorSpace::CieBased(CieBasedColorSpace::Srgb) => Some(DeviceColorSpace::Rgb),
            ColorSpace::CieBased(CieBasedColorSpace::Luma) => Some(DeviceColorSpace::Gray),
            ColorSpace::CieBased(CieBasedColorSpace::Cmyk(_)) => Some(DeviceColorSpace::Cmyk),
            _ => None,
        }
    }

    if a == b {
        return true;
    }

    match (canonical_base_model(a), canonical_base_model(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn serialize_postscript_shading(
    sc: &mut SerializeContext,
    chunk: &mut Chunk,
    stream_chunk: &mut Chunk,
    root_ref: Ref,
    post_script_gradient: &PostScriptGradient,
    stops: &[Stop],
    use_opacities: bool,
    cs: MaybeDeviceColorSpace,
) {
    let domain = post_script_gradient.domain;

    let bump = Bump::new();
    let function_ref = select_postscript_function(
        post_script_gradient,
        stops,
        stream_chunk,
        sc,
        &bump,
        use_opacities,
    );
    let mut shading = chunk.function_shading(root_ref);
    shading.shading_type(FunctionShadingType::Function);

    set_colorspace(cs, shading.deref_mut());

    // Write the identity matrix, because ghostscript has a bug where
    // it thinks the entry is mandatory.
    shading.matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    shading.anti_alias(post_script_gradient.anti_alias);
    shading.function(function_ref);

    shading.domain([domain.left(), domain.right(), domain.top(), domain.bottom()]);
    shading.finish();
}

fn serialize_axial_radial_shading(
    sc: &mut SerializeContext,
    chunk: &mut Chunk,
    root_ref: Ref,
    radial_axial_gradient: &RadialAxialGradient,
    stops: &[Stop],
    use_opacities: bool,
    cs: MaybeDeviceColorSpace,
) {
    let function_ref = select_axial_radial_function(stops, chunk, sc, use_opacities);
    let mut shading = chunk.function_shading(root_ref);
    if radial_axial_gradient.shading_type == FunctionShadingType::Radial {
        shading.shading_type(FunctionShadingType::Radial);
    } else {
        shading.shading_type(FunctionShadingType::Axial);
    }

    set_colorspace(cs, shading.deref_mut());

    shading.anti_alias(radial_axial_gradient.anti_alias);
    shading.function(function_ref);
    shading.coords(radial_axial_gradient.coords.iter().copied());
    shading.extend([true, true]);
    shading.finish();
}

fn select_axial_radial_function(
    stops: &[Stop],
    chunk: &mut Chunk,
    sc: &mut SerializeContext,
    use_opacities: bool,
) -> Ref {
    debug_assert!(stops.len() > 1);

    let mut stops = stops.to_vec();

    if let Some(first) = stops.first() {
        if first.offset.get() != 0.0 {
            let mut new_stop = first.clone();
            new_stop.offset = NormalizedF32::ZERO;
            stops.insert(0, new_stop);
        }
    }

    if let Some(last) = stops.last() {
        if last.offset.get() != 1.0 {
            let mut new_stop = last.clone();
            new_stop.offset = NormalizedF32::ONE;
            stops.push(new_stop);
        }
    }

    if stops.len() == 2 {
        if use_opacities {
            serialize_exponential(
                vec![stops[0].opacity.get()],
                vec![stops[1].opacity.get()],
                vec![0.0, 1.0],
                chunk,
                sc,
            )
        } else {
            serialize_exponential(
                stops[0]
                    .color
                    .to_pdf_color()
                    .into_iter()
                    .collect::<Vec<_>>(),
                stops[1]
                    .color
                    .to_pdf_color()
                    .into_iter()
                    .collect::<Vec<_>>(),
                color_output_range(&stops[0].color),
                chunk,
                sc,
            )
        }
    } else {
        serialize_stitching(&stops, chunk, sc, use_opacities)
    }
}

fn select_postscript_function(
    properties: &PostScriptGradient,
    stops: &[Stop],
    chunk: &mut Chunk,
    sc: &mut SerializeContext,
    bump: &Bump,
    use_opacities: bool,
) -> Ref {
    debug_assert!(stops.len() > 1);

    if properties.gradient_type == GradientType::Linear {
        serialize_linear_postscript(properties, stops, chunk, sc, use_opacities)
    } else if properties.gradient_type == GradientType::Sweep {
        serialize_sweep_postscript(properties, stops, chunk, sc, bump, use_opacities)
    } else {
        todo!();
    }
}

// Not working yet
// fn serialize_radial_postscript(
//     properties: &GradientProperties,
//     sc: &mut SerializeContext,
//     bbox: &Rect,
// ) -> Ref {
// let root_ref = sc.new_ref();
//
// let start_code = [
//     "{".to_string(),
//     // Stack: x y
//     "80 exch 80 sub dup mul 3 1 roll sub dup mul add sqrt 120 div 0 0".to_string(),
// ];
//
// let end_code = ["}".to_string()];
//
// let mut code = Vec::new();
// code.extend(start_code);
// // code.push(encode_spread_method(min, max, properties.spread_method));
// // code.push(encode_stops(&properties.stops, min, max));
// code.extend(end_code);
//
// let code = code.join(" ").into_bytes();
// let mut postscript_function = sc.chunk_mut().post_script_function(root_ref, &code);
// postscript_function.domain([bbox.left(), bbox.right(), bbox.top(), bbox.bottom()]);
// postscript_function.range([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
//
// root_ref
// }

fn serialize_sweep_postscript(
    properties: &PostScriptGradient,
    stops: &[Stop],
    chunk: &mut Chunk,
    sc: &mut SerializeContext,
    bump: &Bump,
    use_opacities: bool,
) -> Ref {
    use pdf_writer::types::PostScriptOp::*;

    let root_ref = sc.new_ref();

    let min: f32 = properties.min;
    let max: f32 = properties.max;

    let mut code = vec![];
    code.extend([
        // Stack: x y
        // Shift by cy, so gradient actually starts from the center
        Real(properties.cy),
        Sub,
        Exch,
        // y x
        // Shift by cx, so gradient actually starts from the center
        Real(properties.cx),
        Sub,
        // Make sure x is never 0.
        Dup,
        Dup,
        Real(0.0001),
        Lt,
        Exch,
        Real(-0.0001),
        Gt,
        And,
        If(bump.alloc([Pop, Real(0.0001)])),
        // Get the angle
        Atan,
    ]);

    encode_spread_method(min, max, &mut code, bump, properties.spread_method);
    encode_postscript_stops(stops, min, max, &mut code, bump, use_opacities);

    let encoded = PostScriptOp::encode(&code);
    sc.register_limits(encoded.limits());
    let encoded = FilterStreamBuilder::new_from_content_stream(&encoded, &sc.serialize_settings())
        .finish(&sc.serialize_settings());
    let mut postscript_function = chunk.post_script_function(root_ref, encoded.encoded_data());
    encoded.write_filters(postscript_function.deref_mut().deref_mut());
    postscript_function.domain([
        properties.domain.left(),
        properties.domain.right(),
        properties.domain.top(),
        properties.domain.bottom(),
    ]);

    if use_opacities {
        postscript_function.range([0.0, 1.0]);
    } else {
        postscript_function.range(color_output_range(&stops[0].color));
    }

    root_ref
}

const MAX_POSTSCRIPT_STOPS: usize = 97;

fn trim_stops(stops: &[Stop]) -> Vec<Stop> {
    let len = stops.len();
    let factor = len as f32 / MAX_POSTSCRIPT_STOPS as f32;
    let mut cur_index: f32 = 0.0;

    let get_index = |i: f32| i.round() as usize;

    let mut new_stops = vec![];

    while get_index(cur_index) < stops.len() {
        new_stops.push(stops[get_index(cur_index)].clone());
        cur_index += factor;
    }

    new_stops
}

fn serialize_linear_postscript(
    properties: &PostScriptGradient,
    stops: &[Stop],
    chunk: &mut Chunk,
    sc: &mut SerializeContext,
    use_opacities: bool,
) -> Ref {
    use pdf_writer::types::PostScriptOp::*;

    let bump = Bump::new();
    let root_ref = sc.new_ref();

    let min: f32 = properties.min;
    let max: f32 = properties.max;

    let mut code = vec![];
    code.extend([
        // Stack: x y
        // Ignore the y coordinate. We account for it in the gradient transform.
        Pop,
        // x
    ]);

    encode_spread_method(min, max, &mut code, &bump, properties.spread_method);
    encode_postscript_stops(stops, min, max, &mut code, &bump, use_opacities);

    let encoded = PostScriptOp::encode(&code);
    sc.register_limits(encoded.limits());
    let encoded = FilterStreamBuilder::new_from_content_stream(&encoded, &sc.serialize_settings())
        .finish(&sc.serialize_settings());
    let mut postscript_function = chunk.post_script_function(root_ref, encoded.encoded_data());
    encoded.write_filters(postscript_function.deref_mut().deref_mut());
    postscript_function.domain([
        properties.domain.left(),
        properties.domain.right(),
        properties.domain.top(),
        properties.domain.bottom(),
    ]);

    if use_opacities {
        postscript_function.range([0.0, 1.0]);
    } else {
        postscript_function.range(color_output_range(&stops[0].color));
    }

    root_ref
}

/// Postscript code that, given an arbitrary x coordinate, normalizes it to an x coordinate
/// between min and max that yields the correct color, depending on the spread mode. In the case
/// of the `Pad` spread methods, the coordinate will not be normalized since the Postscript functions
/// assign the correct value by default.
fn encode_spread_method<'a>(
    min: f32,
    max: f32,
    code: &mut Vec<PostScriptOp<'a>>,
    bump: &'a Bump,
    spread_method: SpreadMethod,
) {
    use pdf_writer::types::PostScriptOp::*;

    if spread_method == SpreadMethod::Pad {
        return;
    }

    let length = max - min;

    code.extend([
        // We do the following:
        // 1. Normalize by doing n = x - min.
        // 2. Calculate the "interval" we are in by doing i = floor(n / length)
        // 3. Calculate the offset by doing o = n - i * length
        // 4. If the spread method is repeat, we additionally calculate o = length - 0 if i % 2 == 1
        // 5. Calculate the final value with x_new = min + o.

        // Current stack:
        // x
        Real(length),
        Real(min),
        // x length min
        Integer(2),
        Index,
        // x length min x
        Integer(1),
        Index,
        // x length min x min
        Sub,
        // x length min n
        Dup,
        // x length min n n
        Integer(3),
        Index,
        // x length min n n length
        Div,
        // x length min n {n/length}
        Floor,
        // x length min n i
        Exch,
        // x length min i n
        Integer(1),
        Index,
        // x length min i n i
        Integer(4),
        Index,
        // x length min i n i length
        Mul,
        // x length min i n {i * length}
        Sub,
        // x length min i o
        Exch,
        // x length min o i
        Cvi,
        Abs,
        // x length min o abs(i)
        Integer(2),
        Mod,
        // x length min o {abs(i) % 2}
        // See https://github.com/google/skia/blob/645b77ce61449951cb9f3cf754b47d4977b68e1a/src/pdf/SkPDFGradientShader.cpp#L402-L408
        // for why we check > 0 instead of == 1.
        Integer(0),
        Gt,
        // x length min o {(abs(i) % 2) > 0}
        if spread_method == SpreadMethod::Reflect {
            If(bump.alloc([Integer(2), Index, Exch, Sub]))
        } else {
            Pop
        },
        // x length min o
        Add,
        // x length x_new
        Integer(3),
        Integer(1),
        Roll,
        // x_new x length
        Pop,
        Pop,
        // x_new
    ]);
}

/// Postscript code that, given an x coordinate between the min and max
/// of a gradient, returns the interpolated color value depending on where it
/// lies within the stops.
fn encode_postscript_stops<'a>(
    stops: &[Stop],
    min: f32,
    max: f32,
    code: &mut Vec<PostScriptOp<'a>>,
    bump: &'a Bump,
    use_opacities: bool,
) {
    // Our algorithm requires the stops to be padded.
    let mut stops = stops.to_vec();

    // Most viewers have a nesting depth on how many `elseif` can be nested (100 for mupdf,
    // 128 for Chrome and 256 for Acrobat), so we trim the stops if they are too large.
    // this is of course a bit unfortunate, but it should be pretty rare to have that many
    // stops, and if we do, the stops are most likely going to be very similar, so it's
    // safe to just sample from in-between.
    if stops.len() > MAX_POSTSCRIPT_STOPS {
        stops = trim_stops(&stops);
    }

    if let Some(first) = stops.first() {
        let mut first = first.clone();
        first.offset = NormalizedF32::ZERO;
        stops.insert(0, first);
    }

    if let Some(last) = stops.last() {
        let mut last = last.clone();
        last.offset = NormalizedF32::ONE;
        stops.push(last);
    }

    encode_stops_impl(&stops, min, max, code, bump, use_opacities);
}

fn encode_stops_impl<'a>(
    stops: &[Stop],
    min: f32,
    max: f32,
    code: &mut Vec<PostScriptOp<'a>>,
    bump: &'a Bump,
    use_opacities: bool,
) {
    use pdf_writer::types::PostScriptOp::*;

    let encode_two_stops =
        |c0: &[f32], c1: &[f32], min: f32, max: f32, code: &mut Vec<PostScriptOp>| {
            if min == max {
                code.push(Pop);
                code.extend(c0.iter().map(|n| Real(*n)));
                return;
            }

            // Sanity check that both stops have the same number of components.
            assert_eq!(
                c0.len(),
                c1.len(),
                "cannot create gradient with stops from different color spaces"
            );

            // Normalize the x coordinate to be between 0 and 1.
            code.extend([Real(min), Sub, Real(max), Real(min), Sub, Div]);

            for i in 0..c0.len() {
                // Interpolate each color component c0 + x_norm * (x1 - c0).
                code.extend([
                    Integer(i as i32),
                    Index,
                    Real(c0[i]),
                    Exch,
                    Real(c1[i]),
                    Real(c0[i]),
                    Sub,
                    Mul,
                    Add,
                ]);
                // x_norm, c0, c1, ...
            }
            // Remove x_norm from the stack.
            code.extend([Integer((c0.len() + 1) as i32), Integer(-1), Roll, Pop]);
            // c0, c1, c2, ...
        };

    if stops.len() == 1 {
        if use_opacities {
            code.push(Real(stops[0].opacity.get()));
        } else {
            code.extend(stops[0].color.to_pdf_color().into_iter().map(Real));
        }
    } else {
        let length = max - min;
        let stops_min = min + length * stops[0].offset.get();
        let stops_max = min + length * stops[1].offset.get();
        // Write the if conditions to find the corresponding set of two stops.

        let if_stops = bump.alloc(vec![]);
        if use_opacities {
            encode_two_stops(
                &[stops[0].opacity.get()],
                &[stops[1].opacity.get()],
                stops_min,
                stops_max,
                if_stops,
            )
        } else {
            encode_two_stops(
                &stops[0]
                    .color
                    .to_pdf_color()
                    .into_iter()
                    .collect::<Vec<_>>(),
                &stops[1]
                    .color
                    .to_pdf_color()
                    .into_iter()
                    .collect::<Vec<_>>(),
                stops_min,
                stops_max,
                if_stops,
            )
        };
        let else_stops = bump.alloc(vec![]);
        encode_stops_impl(&stops[1..], min, max, else_stops, bump, use_opacities);

        code.extend([Dup, Real(stops_max), Le, IfElse(if_stops, else_stops)]);
    }
}

fn serialize_stitching(
    stops: &[Stop],
    chunk: &mut Chunk,
    sc: &mut SerializeContext,
    use_opacities: bool,
) -> Ref {
    // CSS hard colour stops place two stops at the same offset (e.g.
    // `transparent 0 36pt, black 36pt 72pt`). Emitting a sub-function for that
    // empty interval yields a zero-width FunctionType 3 sub-domain and a
    // duplicate Bounds entry — non-increasing Bounds, malformed per PDF
    // 32000-2 §7.10.4.
    //
    // `spread_coincident_offsets` decides, per coincident run, whether to widen
    // it into a sub-pixel ramp or leave it as an exact step; the `>=` guard in
    // the loop then drops the zero-width segment any exact step leaves behind.
    let spread = spread_coincident_offsets(stops, use_opacities);
    let stops = spread.as_slice();

    // The stitching function and every exponential sub-function share the
    // shading's output range: a single opacity channel for a soft mask,
    // otherwise the per-component range of the (uniform) stop colour space.
    let output_range = if use_opacities {
        vec![0.0, 1.0]
    } else {
        color_output_range(&stops[0].color)
    };

    let root_ref = sc.new_ref();
    let mut functions = vec![];
    let mut bounds = vec![];
    let mut encode = vec![];

    // /Bounds shall be strictly increasing (ISO 32000-2 §7.10.4, Table 41:
    // "Bounds elements shall be in order of increasing value"). The `>=` guard
    // below only compares each window's own pair, but `spread_coincident_offsets`
    // nudges a coincident run symmetrically without regard to neighbouring
    // distinct stops, so a nudged offset can overshoot a later distinct stop. An
    // increasing adjacent window can then still push a `second.offset` that is
    // <= a bound already emitted across a skipped window. Track the last bound
    // pushed (starting at Domain0 = 0.0) and drop any window that fails to
    // advance past it, so the retained bounds stay strictly increasing.
    let mut last_bound = 0.0_f32;

    for window in stops.windows(2) {
        let (first, second) = (&window[0], &window[1]);

        // Drop degenerate (zero- or negative-width) segments; see above.
        if first.offset.get() >= second.offset.get() {
            continue;
        }

        // Drop a window whose upper offset does not advance past the last
        // retained bound; see `last_bound` above.
        if second.offset.get() <= last_bound {
            continue;
        }
        last_bound = second.offset.get();

        bounds.push(second.offset.get());

        let (c0_components, c1_components) = if use_opacities {
            (vec![first.opacity.get()], vec![second.opacity.get()])
        } else {
            (
                first.color.to_pdf_color().into_iter().collect::<Vec<_>>(),
                second.color.to_pdf_color().into_iter().collect::<Vec<_>>(),
            )
        };

        let exp_ref = serialize_exponential(
            c0_components,
            c1_components,
            output_range.clone(),
            chunk,
            sc,
        );

        functions.push(exp_ref);
        encode.extend([0.0, 1.0]);
    }

    bounds.pop();
    let mut stitching_function = chunk.stitching_function(root_ref);
    stitching_function.domain([0.0, 1.0]);
    stitching_function.range(output_range);
    stitching_function.functions(functions);
    stitching_function.bounds(bounds);
    stitching_function.encode(encode);

    root_ref
}

/// Spread runs of stops that share an offset into a sub-pixel ramp, so a CSS
/// hard stop does not reach PDFium as a true discontinuity.
///
/// Per run, [`should_spread_run`] decides: an opacity (soft-mask) transition is
/// always widened — a discontinuous opacity inside a soft-mask group makes
/// PDFium subdivide without bound (~14 s for one masked box); a colour run is
/// widened only when it is degenerate (all its stops share a colour, e.g. the
/// constant black of an alpha-only `transparent`→`black` gradient), so a genuine
/// colour hard stop (`red 50%, blue 50%`) stays an exact step — cheap for PDFium
/// as a fill, and left for the caller's `>=` guard to collapse.
///
/// Widened runs are spread symmetrically about the shared offset by
/// [`HARD_STOP_EPSILON`] and clamped to `[0, 1]`. The nudged offsets are then
/// strictly increasing, except that any nudge reaching a boundary is clamped to
/// `0.0` or `1.0`; a run crowded against a boundary can therefore leave two or
/// more stops coincident at that boundary value, which the caller's `>=` guard
/// then drops.
fn spread_coincident_offsets(stops: &[Stop], use_opacities: bool) -> Vec<Stop> {
    /// Spacing inserted between coincident stops — i.e. the ramp width produced
    /// for a single hard stop — in normalised gradient space. Measured PDFium
    /// soft-mask knee: a ramp ≤0.1% of the gradient still subdivides
    /// pathologically (~20 s), whereas ≥0.2% rasterises in microseconds. 0.3%
    /// sits comfortably past the knee while staying sub-pixel on typical masks
    /// at print resolution (0.3% × 72 pt ≈ 0.9 px at 300 dpi).
    const HARD_STOP_EPSILON: f32 = 3.0e-3;

    let mut out = stops.to_vec();
    let len = out.len();
    let mut i = 0;
    while i < len {
        let offset = out[i].offset.get();
        let mut j = i + 1;
        while j < len && out[j].offset.get() == offset {
            j += 1;
        }
        if j - i > 1 && should_spread_run(&out[i..j], use_opacities) {
            let run = (j - i) as f32;
            for (k, stop) in out[i..j].iter_mut().enumerate() {
                let centred = k as f32 - (run - 1.0) / 2.0;
                let nudged = (offset + centred * HARD_STOP_EPSILON).clamp(0.0, 1.0);
                if let Some(value) = NormalizedF32::new(nudged) {
                    stop.offset = value;
                }
            }
        }
        i = j;
    }
    out
}

/// Whether a run of coincident-offset stops should be widened into a ramp
/// rather than left as an exact step. See [`spread_coincident_offsets`].
fn should_spread_run(run: &[Stop], use_opacities: bool) -> bool {
    if use_opacities {
        // Any opacity step inside a soft-mask group is the pathological case.
        return true;
    }
    // A colour step is widened only when it carries no actual colour change, so
    // a genuine hard colour stop stays exact.
    run.iter().all(|stop| stop.color == run[0].color)
}

/// The `/Range` array — `[min, max]` for each output component — of a shading
/// function whose output is a colour in `color`'s colour space. A function's
/// output is clipped to `/Range` (ISO 32000-2 Table 38), and for a Type 4
/// PostScript function `/Range` is required and fixes the output-component
/// count (§7.10.5.3).
///
/// Every device, ICC and calibrated space krilla emits normalises each
/// component to `[0, 1]`. Lab is the exception (ISO 32000-2 §8.6.5.4, Table 64):
/// its `L*` component spans `[0, 100]` and its `a*`/`b*` components span the
/// colour space's `Range` (default `[-100, 100]`). A flat `[0, 1]` range would
/// clip a Lab function's output to the nearest boundary, collapsing every Lab
/// stop to near-black.
fn color_output_range(color: &Color) -> Vec<f32> {
    if let Color::Regular(RegularColor::Lab { params, .. }) = color {
        let [a_min, a_max, b_min, b_max] = params.range.unwrap_or([-100.0, 100.0, -100.0, 100.0]);
        vec![0.0, 100.0, a_min, a_max, b_min, b_max]
    } else {
        [0.0, 1.0].repeat(color.to_pdf_color().len())
    }
}

fn serialize_exponential(
    c0: Vec<f32>,
    c1: Vec<f32>,
    range: Vec<f32>,
    chunk: &mut Chunk,
    sc: &mut SerializeContext,
) -> Ref {
    let root_ref = sc.new_ref();
    assert_eq!(
        c0.len(),
        c1.len(),
        "cannot create gradient with stops from different color spaces"
    );

    let mut exp = chunk.exponential_function(root_ref);

    exp.range(range);
    exp.c0(c0);
    exp.c1(c1);
    exp.domain([0.0, 1.0]);
    exp.n(1.0);
    exp.finish();
    root_ref
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::color::rgb;

    fn stop(offset: f32) -> Stop {
        stop_rgb(offset, 0, 0, 0)
    }

    fn stop_rgb(offset: f32, r: u8, g: u8, b: u8) -> Stop {
        Stop {
            offset: NormalizedF32::new(offset).unwrap(),
            color: rgb::Color::new(r, g, b).into(),
            opacity: NormalizedF32::ONE,
        }
    }

    fn offsets(stops: &[Stop]) -> Vec<f32> {
        stops.iter().map(|s| s.offset.get()).collect()
    }

    fn assert_strictly_increasing(stops: &[Stop]) {
        for pair in stops.windows(2) {
            assert!(
                pair[0].offset.get() < pair[1].offset.get(),
                "offsets must be strictly increasing, got {:?}",
                offsets(stops)
            );
        }
    }

    fn assert_non_decreasing(stops: &[Stop]) {
        for pair in stops.windows(2) {
            assert!(
                pair[0].offset.get() <= pair[1].offset.get(),
                "offsets must be non-decreasing, got {:?}",
                offsets(stops)
            );
        }
    }

    #[test]
    fn opacity_spread_separates_a_coincident_pair() {
        // A CSS hard stop (`transparent 0 50%, black 50% 100%`) yields two stops
        // at 0.5; without spreading they produce a duplicate Bounds entry.
        let spread = spread_coincident_offsets(&[stop(0.0), stop(0.5), stop(0.5), stop(1.0)], true);
        assert_eq!(spread.len(), 4);
        assert_strictly_increasing(&spread);
        // The ramp stays centred on the original offset and narrow.
        assert!((spread[1].offset.get() - 0.5).abs() < 1.0e-2);
        assert!((spread[2].offset.get() - 0.5).abs() < 1.0e-2);
    }

    #[test]
    fn spread_leaves_distinct_offsets_untouched() {
        let input = [stop(0.0), stop(0.49), stop(0.51), stop(1.0)];
        assert_eq!(
            offsets(&spread_coincident_offsets(&input, true)),
            offsets(&input)
        );
    }

    #[test]
    fn opacity_spread_keeps_boundary_runs_in_range_and_ordered() {
        // Coincident stops at both 0 and 1 must stay within [0, 1] yet ordered.
        let spread = spread_coincident_offsets(&[stop(0.0), stop(0.0), stop(1.0), stop(1.0)], true);
        assert_strictly_increasing(&spread);
        assert!(spread.first().unwrap().offset.get() >= 0.0);
        assert!(spread.last().unwrap().offset.get() <= 1.0);
    }

    #[test]
    fn colour_spread_widens_a_degenerate_step() {
        // An alpha-only gradient is constant black; its coincident colour stops
        // carry no colour change, so the colour shading is widened too (a step
        // there is needless and PDFium subdivides it inside a soft-mask group).
        let spread =
            spread_coincident_offsets(&[stop(0.0), stop(0.5), stop(0.5), stop(1.0)], false);
        assert_strictly_increasing(&spread);
    }

    #[test]
    fn colour_spread_keeps_a_genuine_hard_stop_exact() {
        // `red 50%, blue 50%` is a real colour discontinuity: it must stay an
        // exact step (cheap for PDFium as a fill), so the offsets are untouched.
        let input = [
            stop_rgb(0.0, 255, 0, 0),
            stop_rgb(0.5, 255, 0, 0),
            stop_rgb(0.5, 0, 0, 255),
            stop_rgb(1.0, 0, 0, 255),
        ];
        assert_eq!(
            offsets(&spread_coincident_offsets(&input, false)),
            offsets(&input)
        );
    }

    #[test]
    fn opacity_spread_boundary_run_of_three_stays_non_decreasing_in_range() {
        // A run of three coincident stops at a boundary cannot stay strictly
        // increasing after clamping: the centred nudges reach the boundary and
        // collapse onto it (three at 1.0 -> [0.997, 1.0, 1.0]). The result must
        // still be non-decreasing and within [0, 1]; the caller's `>=` guard
        // drops the collapsed segment.
        let at_zero = [stop(0.0), stop(0.0), stop(0.0), stop(1.0)];
        let at_one = [stop(0.0), stop(1.0), stop(1.0), stop(1.0)];
        for run in [at_zero.as_slice(), at_one.as_slice()] {
            let spread = spread_coincident_offsets(run, true);
            assert_non_decreasing(&spread);
            assert!(spread.first().unwrap().offset.get() >= 0.0);
            assert!(spread.last().unwrap().offset.get() <= 1.0);
        }
    }

    #[test]
    fn sanitize_keeps_uniform_rgb_gradient_with_black_endpoint() {
        // preserve_black resolves a pure-black RGB stop to DeviceRGB while its
        // non-black neighbours resolve to sRGB under no_device_cs. That is one
        // colour model, not a mix: the white stop must keep its colour rather
        // than be coerced to the first (black) stop's colour.
        let mut sc = SerializeContext::new(crate::SerializeSettings {
            preserve_black: true,
            no_device_cs: true,
            ..crate::SerializeSettings::default()
        });
        let stops = [stop_rgb(0.0, 0, 0, 0), stop_rgb(1.0, 255, 255, 255)];
        let sanitized = sanitize_gradient_stops(&stops, &mut sc);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[0].color, stops[0].color);
        assert_eq!(sanitized[1].color, stops[1].color);
    }
}
