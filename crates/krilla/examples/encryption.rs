//! Produces an AES-256 encrypted PDF (Standard Security Handler V=5,
//! R=6 — ISO 32000-2 §7.6.4 "AESV3").
//!
//! The user password is `alice`, the owner password is `bob`.
//! Permissions allow printing and content copy; everything else is
//! denied. Open the resulting `target/encryption.pdf` in a PDF viewer
//! — it should prompt for one of the passwords before rendering.

use std::path;

use krilla::encryption::{Encryption, Permissions};
use krilla::geom::Point;
use krilla::metadata::Metadata;
use krilla::page::PageSettings;
use krilla::text::{Font, TextDirection};
use krilla::{Document, SerializeSettings};

fn main() {
    // Encryption is configured on `SerializeSettings::encryption`.
    // krilla applies it to the underlying `Pdf` immediately after
    // the header is written, so every string and stream emitted
    // afterwards — including the content streams below — is wrapped
    // with a per-object IV and encrypted under the file key.
    let settings = SerializeSettings {
        encryption: Some(
            Encryption::new("alice", "bob")
                .with_permissions(Permissions::PRINT | Permissions::COPY),
        ),
        ..Default::default()
    };

    let mut document = Document::new_with(settings);
    document.set_metadata(Metadata::new().title("Confidential Report".into()));

    let font = {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fonts/NotoSans-Regular.ttf");
        let data = std::fs::read(&path).unwrap();
        Font::new(data.into(), 0).unwrap()
    };

    let mut page = document.start_page_with(PageSettings::from_wh(200.0, 50.0).unwrap());
    let mut surface = page.surface();
    surface.draw_text(
        Point::from_xy(0.0, 25.0),
        font,
        14.0,
        "This text is encrypted at rest.",
        false,
        TextDirection::Auto,
    );
    surface.finish();
    page.finish();

    let pdf = document.finish().unwrap();
    let path = path::absolute("encryption.pdf").unwrap();
    eprintln!(
        "Saved encrypted PDF to '{}'. User password: alice; owner: bob.",
        path.display(),
    );
    std::fs::write(path, &pdf).unwrap();
}
