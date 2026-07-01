//! Demonstrates optional content groups (PDF "layers"), per ISO
//! 32000-1 §8.11 and ISO 32000-2 §8.11.
//!
//! Writes a single page with two layers — a base "Map" and a "Notes"
//! overlay — that the viewer can toggle independently from its layer
//! panel. The base layer is on by default; the notes layer is off.
//! Open the resulting `target/optional_content.pdf` in a PDF viewer
//! that exposes optional content (Acrobat, Foxit, modern Preview).

use std::path;

use krilla::geom::Point;
use krilla::optional_content::Layer;
use krilla::page::PageSettings;
use krilla::text::{Font, TextDirection};
use krilla::Document;

fn main() {
    let mut document = Document::new();
    // Register the layers up front. The returned `LayerHandle`s are
    // passed to `Surface::push_layer` to bracket drawing operations.
    let map = document.add_layer(Layer::new("Map"));
    let notes = document.add_layer(Layer::new("Notes").with_default_visible(false));

    let font = {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fonts/NotoSans-Regular.ttf");
        let data = std::fs::read(&path).unwrap();
        Font::new(data.into(), 0).unwrap()
    };

    let mut page = document.start_page_with(PageSettings::from_wh(300.0, 100.0).unwrap());
    let mut surface = page.surface();

    // Anything drawn between push_layer / pop is gated on the
    // layer's visibility. Use Surface::pop to close the
    // marked-content sequence — it understands the matching
    // PushInstruction::Layer just like push_transform / push_clip.
    surface.push_layer(map);
    surface.draw_text(
        Point::from_xy(10.0, 30.0),
        font.clone(),
        14.0,
        "Map layer (on by default)",
        false,
        TextDirection::Auto,
    );
    surface.pop();

    surface.push_layer(notes);
    surface.draw_text(
        Point::from_xy(10.0, 70.0),
        font,
        12.0,
        "Notes layer (off by default — toggle in viewer)",
        false,
        TextDirection::Auto,
    );
    surface.pop();

    surface.finish();
    page.finish();

    let pdf = document.finish().unwrap();
    let path = path::absolute("optional_content.pdf").unwrap();
    eprintln!("Saved layered PDF to '{}'.", path.display());
    std::fs::write(path, &pdf).unwrap();
}
