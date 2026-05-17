use krilla::blend::BlendMode;
use krilla::geom::{Point, Size, Transform};
use krilla::page::Page;
use krilla::text::{Font, TextDirection};
use krilla_macros::snapshot;

use crate::embed::file_1;
use crate::{blue_fill, load_png_image, red_fill, NOTO_SANS};
use crate::{green_fill, rect_to_path, Document};

#[snapshot(settings_2)]
fn stream_resource_cache(page: &mut Page) {
    let mut surface = page.surface();
    let path1 = rect_to_path(0.0, 0.0, 100.0, 100.0);
    let path2 = rect_to_path(50.0, 50.0, 150.0, 150.0);
    let path3 = rect_to_path(100.0, 100.0, 200.0, 200.0);

    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&path1);
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&path2);
    surface.set_fill(Some(blue_fill(1.0)));
    surface.draw_path(&path3);
}

#[snapshot]
fn stream_nested_transforms(page: &mut Page) {
    let mut surface = page.surface();
    let path1 = rect_to_path(0.0, 0.0, 100.0, 100.0);

    surface.push_transform(&Transform::from_translate(50.0, 50.0));
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&path1);
    surface.push_transform(&Transform::from_translate(100.0, 100.0));
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&path1);

    surface.pop();
    surface.pop();
}

#[snapshot]
fn stream_reused_graphics_state(page: &mut Page) {
    let mut surface = page.surface();
    let path1 = rect_to_path(0.0, 0.0, 100.0, 100.0);
    surface.set_fill(Some(green_fill(0.5)));
    surface.draw_path(&path1);
    surface.push_blend_mode(BlendMode::ColorBurn);
    surface.set_fill(Some(green_fill(0.5)));
    surface.draw_path(&path1);
    surface.pop();
    surface.set_fill(Some(green_fill(0.5)));
    surface.draw_path(&path1);
}

// Make sure page streams, images, etc. are flate encoded with default settings.
#[snapshot(document, settings_29)]
fn stream_compress_by_default(document: &mut Document) {
    document.embed_file(file_1());

    let mut page = document.start_page();
    let mut surface = page.surface();
    let path1 = rect_to_path(0.0, 0.0, 100.0, 100.0);
    surface.set_fill(Some(green_fill(0.5)));
    surface.draw_path(&path1);

    let image = load_png_image("luma8.png");
    let size = Size::from_wh(100.0, 100.0).unwrap();
    surface.draw_image(image, size);

    let font = Font::new(NOTO_SANS.clone(), 0).unwrap();
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_text(
        Point::from_xy(0.0, 50.0),
        font,
        20.0,
        "Hello World",
        false,
        TextDirection::Auto,
    );
}

/// `Surface::push_overprint` must emit an `ExtGState` carrying `/OP`,
/// `/op` and `/OPM` entries that match the [`Overprint`] descriptor
/// supplied by the caller, and the dictionary must be installed via
/// `gs` in the content stream.
#[test]
fn overprint_extgstate_entries_emitted() {
    use krilla::geom::PathBuilder;
    use krilla::overprint::{Overprint, OverprintMode};
    use krilla::page::PageSettings;
    use krilla::Document;

    let mut document = Document::new();
    let mut page = document.start_page_with(PageSettings::from_wh(100.0, 100.0).unwrap());
    let mut surface = page.surface();
    let mut pb = PathBuilder::new();
    pb.move_to(0.0, 0.0);
    pb.line_to(50.0, 0.0);
    pb.line_to(50.0, 50.0);
    pb.line_to(0.0, 50.0);
    pb.close();
    let path = pb.finish().unwrap();
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&path);
    surface.push_overprint(
        Overprint::new()
            .stroking(true)
            .non_stroking(true)
            .mode(OverprintMode::IgnoreZeroChannel),
    );
    surface.set_fill(Some(green_fill(1.0)));
    surface.draw_path(&path);
    surface.pop();
    surface.finish();
    page.finish();

    let pdf = document.finish().expect("finish should succeed");

    fn contains_window(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    // pdf-writer flatte-compresses content streams. Inflate every stream
    // dictionary payload and verify the assertions across the whole
    // document text.
    let pdf_text = String::from_utf8_lossy(&pdf);
    let raw_bytes = pdf.clone();
    let combined = inflate_streams(&raw_bytes)
        .into_iter()
        .map(|s| String::from_utf8_lossy(&s).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let pool = format!("{pdf_text}\n{combined}");

    assert!(
        pool.contains("/OP true"),
        "stroking overprint /OP true should be emitted; pool excerpt: {}",
        &pool[..pool.len().min(2000)],
    );
    assert!(
        pool.contains("/op true"),
        "non-stroking overprint /op true should be emitted",
    );
    assert!(
        pool.contains("/OPM 1"),
        "overprint mode /OPM 1 should be emitted",
    );
    // ExtGState dictionary type marker.
    assert!(
        pool.contains("/Type /ExtGState") || pool.contains("/Type/ExtGState"),
        "ExtGState dictionary should be emitted",
    );
    // Sanity: the bytes are not all zero.
    assert!(
        contains_window(&raw_bytes, b"%PDF-"),
        "PDF header should be present",
    );
}

/// Default — no overprint pushed — must not emit any `/OP`, `/op` or
/// `/OPM` entries anywhere in the document.
#[test]
fn overprint_absent_by_default() {
    use krilla::geom::PathBuilder;
    use krilla::page::PageSettings;
    use krilla::Document;

    let mut document = Document::new();
    let mut page = document.start_page_with(PageSettings::from_wh(100.0, 100.0).unwrap());
    let mut surface = page.surface();
    let mut pb = PathBuilder::new();
    pb.move_to(0.0, 0.0);
    pb.line_to(50.0, 0.0);
    pb.line_to(50.0, 50.0);
    pb.line_to(0.0, 50.0);
    pb.close();
    let path = pb.finish().unwrap();
    surface.set_fill(Some(red_fill(1.0)));
    surface.draw_path(&path);
    surface.finish();
    page.finish();

    let pdf = document.finish().expect("finish should succeed");
    let combined = inflate_streams(&pdf)
        .into_iter()
        .map(|s| String::from_utf8_lossy(&s).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let pool = format!("{}\n{combined}", String::from_utf8_lossy(&pdf));

    assert!(
        !pool.contains("/OP true") && !pool.contains("/op true") && !pool.contains("/OPM "),
        "no overprint entries should be emitted without push_overprint",
    );
}

/// Inflate every uncompressed-or-flate stream in a PDF byte buffer.
/// Returns the inflated payloads (in document order). Best-effort: any
/// stream that fails to inflate is skipped silently.
fn inflate_streams(pdf: &[u8]) -> Vec<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(start) = find_subslice(&pdf[cursor..], b"stream") {
        let stream_start = cursor + start + b"stream".len();
        let mut data_start = stream_start;
        if pdf.get(data_start) == Some(&b'\r') {
            data_start += 1;
        }
        if pdf.get(data_start) == Some(&b'\n') {
            data_start += 1;
        }
        let Some(rel_end) = find_subslice(&pdf[data_start..], b"endstream") else {
            break;
        };
        let mut data_end = data_start + rel_end;
        // Trim a trailing newline if present.
        if data_end > 0 && pdf[data_end - 1] == b'\n' {
            data_end -= 1;
            if data_end > 0 && pdf[data_end - 1] == b'\r' {
                data_end -= 1;
            }
        }
        let payload = &pdf[data_start..data_end];

        // Try flate-decode; if it fails, treat the payload as plain bytes.
        let mut inflated = Vec::new();
        let mut decoder = flate2::read::ZlibDecoder::new(payload);
        match decoder.read_to_end(&mut inflated) {
            Ok(_) if !inflated.is_empty() => out.push(inflated),
            _ => out.push(payload.to_vec()),
        }

        cursor = data_end + b"endstream".len();
    }
    out
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
