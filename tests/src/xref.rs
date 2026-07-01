//! Cross-reference layout selection (classic `xref` table vs
//! `/Type /XRef` stream) and trailer `/Size` balance.

use krilla::configure::{ConfigurationBuilder, PdfVersion};
use krilla::geom::{Point, Size};
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::optional_content::Layer;
use krilla::page::PageSettings;
use krilla::paint::{Fill, LinearGradient, SpreadMethod, Stop};
use krilla::tagging::{Tag, TagGroup, TagTree};
use krilla::text::{Font, TextDirection};
use krilla::{Document, FontEmbedding, SerializeSettings};

use crate::{
    load_png_image, settings_1, settings_17, settings_25, stops_with_2_solid_1, NOTO_SANS,
};

fn build_one_page_pdf(settings: SerializeSettings) -> Vec<u8> {
    let mut doc = Document::new_with(settings);
    doc.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
    doc.finish().unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn xref_table_emitted_when_xref_streams_flag_unset() {
    let pdf = build_one_page_pdf(settings_1());

    // Classic layout: a literal `xref` keyword on its own line and a
    // `trailer` dictionary marker. Neither appears in the xref-stream
    // variant.
    assert!(
        contains(&pdf, b"\nxref\n"),
        "classic /xref table keyword missing from default layout"
    );
    assert!(
        contains(&pdf, b"trailer\n"),
        "classic trailer keyword missing from default layout"
    );
    assert!(
        !contains(&pdf, b"/Type /XRef"),
        "/Type /XRef stream emitted when xref_streams flag was false"
    );
}

#[test]
fn xref_stream_emitted_when_xref_streams_flag_set() {
    let mut settings = settings_1();
    settings.xref_streams = true;
    let pdf = build_one_page_pdf(settings);

    // Stream layout: the xref information lives in an indirect
    // `/Type /XRef` object — there is no `xref` / `trailer` keyword
    // pair.
    assert!(
        contains(&pdf, b"/Type /XRef"),
        "/Type /XRef stream missing when xref_streams flag was true"
    );
    assert!(
        !contains(&pdf, b"\nxref\n"),
        "classic /xref keyword still emitted when xref_streams flag was true"
    );
    assert!(
        !contains(&pdf, b"\ntrailer\n"),
        "classic trailer keyword still emitted when xref_streams flag was true"
    );
}

/// Extract the trailer `/Size` value from a classic-xref PDF. Walks
/// the trailer dictionary that follows the `trailer\n<<` marker and
/// returns the integer that follows `/Size`. Returns `None` if the
/// PDF uses an xref stream (no classic trailer keyword).
fn parse_trailer_size(pdf: &[u8]) -> Option<u32> {
    let trailer_pos = pdf
        .windows(b"trailer\n".len())
        .position(|w| w == b"trailer\n")?;
    let after = &pdf[trailer_pos + b"trailer\n".len()..];
    let key = b"/Size";
    let key_pos = after.windows(key.len()).position(|w| w == key)?;
    let mut cursor = &after[key_pos + key.len()..];
    while cursor.first().is_some_and(u8::is_ascii_whitespace) {
        cursor = &cursor[1..];
    }
    let digits_end = cursor
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(cursor.len());
    let digits = std::str::from_utf8(&cursor[..digits_end]).ok()?;
    digits.parse().ok()
}

/// Count the number of `N G obj` markers in a PDF. These delimit each
/// indirect-object body that krilla actually emits. The trailer
/// `/Size` must equal `max_object_id + 1`, i.e. one more than the
/// highest `N` that appears here (because object 0 is the free-list
/// head and is counted in `/Size` but is never written as `0 0 obj`).
fn count_indirect_objects(pdf: &[u8]) -> usize {
    // Scan for ASCII `<digits> <digits> obj\n` byte sequences. PDFs
    // produced by `pdf-writer` always serialise these on their own
    // line, so a simple windowed search over the linewise prefix is
    // enough — no full tokeniser required.
    let needle = b" obj\n";
    pdf.windows(needle.len())
        .enumerate()
        .filter(|(idx, w)| {
            if *w != needle {
                return false;
            }
            // Look back for `<id> <gen>` — both ASCII digit runs
            // separated by a single space, preceded by either start
            // of file or a newline.
            let mut cursor = &pdf[..*idx];
            // Strip the generation digits.
            let gen_end = cursor.len();
            while cursor.last().is_some_and(|b| b.is_ascii_digit()) {
                cursor = &cursor[..cursor.len() - 1];
            }
            if gen_end == cursor.len() || cursor.last().is_none_or(|b| *b != b' ') {
                return false;
            }
            cursor = &cursor[..cursor.len() - 1];
            // Strip the id digits.
            let id_end = cursor.len();
            while cursor.last().is_some_and(|b| b.is_ascii_digit()) {
                cursor = &cursor[..cursor.len() - 1];
            }
            if id_end == cursor.len() {
                return false;
            }
            cursor.last().is_none_or(|b| *b == b'\n' || *b == b'\r')
        })
        .count()
}

fn assert_trailer_balanced(pdf: &[u8], label: &str) {
    let size =
        parse_trailer_size(pdf).unwrap_or_else(|| panic!("{label}: could not parse trailer /Size"));
    let objects = count_indirect_objects(pdf);
    // trailer.Size = max_object_id + 1; the count of emitted objects
    // is exactly max_object_id (because ids are dense from 1 up). The
    // canonical balanced invariant is therefore `size == objects + 1`.
    assert_eq!(
        size as usize,
        objects + 1,
        "{label}: trailer /Size {size} does not match {objects} emitted objects (+1 for the free-list head)"
    );
}

#[test]
fn trailer_size_balanced_pdf_17_untagged() {
    // Default config is PDF 1.7. `settings_1` has `enable_tagging:
    // true` but no `set_tag_tree` call means no struct tree is ever
    // emitted — exercising the path where the PDF 2.0 namespace refs
    // would have been bumped but never written.
    let pdf = build_one_page_pdf(settings_1());
    assert_trailer_balanced(&pdf, "PDF 1.7 untagged");
}

#[test]
fn trailer_size_balanced_pdf_14_untagged() {
    let pdf = build_one_page_pdf(settings_17());
    assert_trailer_balanced(&pdf, "PDF 1.4 untagged");
}

#[test]
fn trailer_size_balanced_pdf_20_untagged() {
    // PDF 2.0 without a tag tree: even the version that the
    // namespaces are meant to serve must not bump unused refs.
    let pdf = build_one_page_pdf(settings_25());
    assert_trailer_balanced(&pdf, "PDF 2.0 untagged");
}

#[test]
fn trailer_size_balanced_pdf_20_tagged() {
    // PDF 2.0 with a struct tree — the path that legitimately
    // allocates and serialises the namespace dicts. The trailer must
    // remain balanced here too, proving the lazy allocator pairs
    // every bump with an emitted object.
    let mut doc = Document::new_with(settings_25());
    doc.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());

    let mut tag_tree = TagTree::new();
    tag_tree.push(TagGroup::new(Tag::P));
    doc.set_tag_tree(tag_tree);

    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "PDF 2.0 tagged");
}

#[test]
fn trailer_size_balanced_pdf_17_explicit_pdf17() {
    // Build a PDF 1.7 document via the explicit configuration to
    // avoid relying on `Configuration::default()` matching 1.7. Also
    // proves the path where `xmp_metadata` is off (no metadata bumps
    // either) cannot leak unused refs.
    let settings = SerializeSettings {
        configuration: ConfigurationBuilder::new()
            .with_version(PdfVersion::Pdf17)
            .finish()
            .unwrap(),
        ..settings_1()
    };
    let pdf = build_one_page_pdf(settings);
    assert_trailer_balanced(&pdf, "PDF 1.7 explicit");
}

/// Build a PDF that draws text using a CID (Type0) font with default
/// `FontEmbedding::Subset`. The default-subset path never emits
/// `cid_to_gid_ref` (only the `Full`+TrueType path does), so a naive
/// unconditional `sc.new_ref()` for that slot produces a leaked object.
#[test]
fn trailer_size_balanced_cid_font_subset() {
    let mut doc = Document::new_with(settings_1());
    let mut page = doc.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
    let mut surface = page.surface();

    let font = Font::new(NOTO_SANS.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(10.0, 100.0),
        font,
        16.0,
        "Hello",
        false,
        TextDirection::Auto,
    );

    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "CID font Subset embedding");
}

/// Same as above but with `FontEmbedding::None`. In this mode both
/// `data_ref` and `cid_to_gid_ref` are allocated but neither is ever
/// emitted, so two leaked objects appear unless the allocation is
/// guarded.
#[test]
fn trailer_size_balanced_cid_font_no_embedding() {
    let settings = SerializeSettings {
        font_embedding: FontEmbedding::None,
        ..settings_1()
    };
    let mut doc = Document::new_with(settings);
    let mut page = doc.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
    let mut surface = page.surface();

    let font = Font::new(NOTO_SANS.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(10.0, 100.0),
        font,
        16.0,
        "Hello",
        false,
        TextDirection::Auto,
    );

    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "CID font no embedding");
}

/// PDF 2.0 deprecates the CIDSet stream, so `cid_set_ref` is allocated
/// at the top of `CidFont::serialize` but its emission is gated behind
/// `!pdf_version.deprecates_cid_set()`. Under PDF 2.0 the slot leaks
/// unless allocation is also gated.
#[test]
fn trailer_size_balanced_cid_font_pdf20() {
    let mut doc = Document::new_with(settings_25());
    let mut page = doc.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
    let mut surface = page.surface();

    let font = Font::new(NOTO_SANS.clone(), 0).unwrap();
    surface.draw_text(
        Point::from_xy(10.0, 100.0),
        font,
        16.0,
        "Hello",
        false,
        TextDirection::Auto,
    );

    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "CID font PDF 2.0 (deprecated CIDSet)");
}

/// Opaque RGB image: no alpha channel, so `soft_mask_id` is `None` and
/// no extra ref is allocated. Confirm balance is preserved in the
/// straightforward path.
#[test]
fn trailer_size_balanced_image_opaque() {
    let mut doc = Document::new_with(settings_1());
    let mut page = doc.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
    let mut surface = page.surface();

    let image = load_png_image("rgb8.png");
    let (w, h) = image.size();
    surface.draw_image(image, Size::from_wh(w as f32, h as f32).unwrap());

    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "opaque image");
}

/// RGBA image: has an alpha channel, so `Image::serialize` allocates a
/// `soft_mask_id` ref via `sc.new_ref()`. That ref must be emitted as
/// the `/SMask` XObject stream or trailer balance breaks.
#[test]
fn trailer_size_balanced_image_with_alpha() {
    let mut doc = Document::new_with(settings_1());
    let mut page = doc.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
    let mut surface = page.surface();

    let image = load_png_image("rgba8.png");
    let (w, h) = image.size();
    surface.draw_image(image, Size::from_wh(w as f32, h as f32).unwrap());

    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "RGBA image (soft-mask path)");
}

/// OCG layer: `Document::add_layer` bumps a ref eagerly. Confirm the
/// chunk_container always emits a `/Type /OCG` dict for every
/// registered layer regardless of whether `Surface::push_layer` was
/// ever called.
#[test]
fn trailer_size_balanced_ocg_layer_registered_but_unused() {
    let mut doc = Document::new_with(settings_1());
    // Register a layer but never push it onto a surface.
    let _handle = doc.add_layer(Layer::new("Unused"));

    doc.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());

    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "OCG layer registered but never pushed");
}

/// PDF 2.0 with only deprecated-field metadata (producer, no
/// creation_date): under PDF 2.0 the deprecated fields are not
/// written to the Info dict, so `serialize_document_info` must not
/// bump a ref that it will then leave unwritten. Before the fix,
/// `LazyCell` was created after an unconditional `ref_.bump()`;
/// the cell was never forced for this path, leaking one ref into
/// `remapped_ref` and producing a free-entry gap that — while
/// not observed by lopdf due to pdf-writer's free-list fill —
/// is a latent correctness defect.
#[test]
fn trailer_size_balanced_pdf20_deprecated_metadata_no_creation_date() {
    let mut doc = Document::new_with(settings_25());

    // `producer` is a deprecated field in PDF 2.0 — it will not be
    // written to the Info dict. `creation_date` is left `None` so no
    // other Info-dict field forces emission either.
    doc.set_metadata(Metadata::new().producer("test-producer".to_string()));

    doc.start_page_with(PageSettings::from_wh(10.0, 10.0).unwrap());
    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "PDF 2.0 deprecated-only metadata (no creation_date)");
}

/// Linear gradient (two stops): exercises the shading-function path
/// which allocates a `root_ref` per call. Confirms balance is
/// preserved across the shading serialisation chain.
#[test]
fn trailer_size_balanced_linear_gradient() {
    let mut doc = Document::new_with(settings_1());
    let mut page = doc.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
    let mut surface = page.surface();

    let gradient = LinearGradient {
        x1: 0.0,
        y1: 0.0,
        x2: 200.0,
        y2: 0.0,
        transform: Default::default(),
        spread_method: SpreadMethod::Pad,
        stops: stops_with_2_solid_1(),
        anti_alias: false,
    };

    use krilla::geom::{Path, PathBuilder};
    let mut pb = PathBuilder::new();
    pb.move_to(0.0, 0.0);
    pb.line_to(200.0, 0.0);
    pb.line_to(200.0, 200.0);
    pb.line_to(0.0, 200.0);
    pb.close();
    let path = pb.finish().unwrap();

    surface.set_fill(Some(Fill {
        paint: gradient.into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    }));
    surface.draw_path(&path);

    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();
    assert_trailer_balanced(&pdf, "linear gradient");
}
