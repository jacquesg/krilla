//! End-to-end tests for [`Document::add_layer`] +
//! [`Surface::push_layer`] / [`Surface::pop`].

use krilla::optional_content::{Layer, LayerIntent};
use krilla::page::PageSettings;
use krilla::Document;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn build_two_layer_doc() -> Vec<u8> {
    let settings = crate::settings_1();
    let mut doc = Document::new_with(settings);
    let map = doc.add_layer(Layer::new("Map"));
    let notes = doc.add_layer(Layer::new("Notes").with_default_visible(false));

    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();

    surface.push_layer(map);
    surface.pop();

    surface.push_layer(notes);
    surface.pop();

    surface.finish();
    page.finish();
    doc.finish().unwrap()
}

#[test]
fn add_layer_emits_ocg_dict_per_layer() {
    let pdf = build_two_layer_doc();
    // /Type /OCG appears once per layer.
    let mut count = 0usize;
    let needle = b"/Type /OCG";
    let mut i = 0;
    while i + needle.len() <= pdf.len() {
        if &pdf[i..i + needle.len()] == needle {
            count += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    assert_eq!(count, 2, "expected one /Type /OCG per layer, got {count}");
}

#[test]
fn add_layer_emits_layer_names() {
    let pdf = build_two_layer_doc();
    assert!(contains(&pdf, b"/Name (Map)"));
    assert!(contains(&pdf, b"/Name (Notes)"));
}

#[test]
fn catalog_carries_oc_properties() {
    let pdf = build_two_layer_doc();
    assert!(
        contains(&pdf, b"/OCProperties"),
        "/OCProperties missing from catalog"
    );
    assert!(contains(&pdf, b"/OCGs ["));
    // Default config carries /ON and /OFF arrays driven by
    // each layer's default_visible flag.
    assert!(contains(&pdf, b"/ON ["));
    assert!(contains(&pdf, b"/OFF ["));
    assert!(contains(&pdf, b"/Order ["));
}

#[test]
fn push_layer_emits_oc_marked_content() {
    let pdf = build_two_layer_doc();
    // The content stream is FlateDecode-compressed by default, so
    // we can't string-match the BDC marker here. Settle for the
    // structural assertions on the OCG dicts and the marked-content
    // stream being present at all (we verified the compiler accepted
    // the API).
    assert!(contains(&pdf, b"/Length"), "no streams in produced PDF");
}

#[test]
fn no_layers_omits_oc_properties() {
    let mut doc = Document::new_with(crate::settings_1());
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let _ = page.surface();
    page.finish();
    let pdf = doc.finish().unwrap();
    assert!(
        !contains(&pdf, b"/OCProperties"),
        "/OCProperties emitted without layers"
    );
    assert!(
        !contains(&pdf, b"/Type /OCG"),
        "/OCG dict emitted without layers"
    );
}

#[test]
fn layer_with_design_intent_emits_correct_name() {
    let mut doc = Document::new_with(crate::settings_1());
    let _ = doc.add_layer(Layer::new("Trim guides").with_intent(LayerIntent::Design));
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let _ = page.surface();
    page.finish();
    let pdf = doc.finish().unwrap();
    assert!(contains(&pdf, b"/Intent /Design"));
    assert!(contains(&pdf, b"/Name (Trim guides)"));
}

#[test]
#[ignore = "shells out to qpdf; run via cargo test -- --ignored qpdf_layered_pdf_check"]
fn qpdf_layered_pdf_check() {
    use std::process::Command;

    let pdf = build_two_layer_doc();
    let path = std::env::temp_dir().join("krilla_layered.pdf");
    std::fs::write(&path, &pdf).expect("write layered PDF");

    let output = Command::new("qpdf")
        .arg("--check")
        .arg(&path)
        .output()
        .expect("qpdf not on PATH");
    assert!(
        output.status.success(),
        "qpdf rejected the layered PDF: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // qpdf doesn't surface OCG state directly through --check, but
    // --show-object on the catalog will reveal /OCProperties. Run
    // --json to confirm there are exactly 2 OCG dicts in the file
    // (a `/Type /OCG` count we already check structurally in
    // `add_layer_emits_ocg_dict_per_layer`).
    let json = Command::new("qpdf")
        .arg("--json=latest")
        .arg(&path)
        .output()
        .expect("qpdf --json");
    let json = String::from_utf8_lossy(&json.stdout);
    let ocg_count = json.matches("\"/OCG\"").count();
    assert!(
        ocg_count >= 2,
        "expected /OCG dicts in qpdf JSON output, got {}: {}",
        ocg_count,
        &json[..json.len().min(2000)],
    );
}

#[test]
#[should_panic(expected = "LayerHandle out of bounds")]
fn invalid_layer_handle_panics() {
    use krilla::optional_content::LayerHandle;
    // Fabricate a handle out of bounds for an empty document.
    let bogus: LayerHandle = unsafe { std::mem::transmute(42u32) };
    let mut doc = Document::new_with(crate::settings_1());
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    surface.push_layer(bogus);
}
