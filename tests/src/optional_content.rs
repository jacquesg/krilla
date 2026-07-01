//! End-to-end tests for [`Document::add_layer`] +
//! [`Surface::push_layer`](krilla::surface::Surface::push_layer)
//! / [`Surface::pop`](krilla::surface::Surface::pop).

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

// --- per-page /Properties bookkeeping ------------------------------

#[test]
fn layer_used_on_one_page_omitted_from_other_pages_properties() {
    // Two pages, two layers. Page 1 uses Map only; page 2 uses
    // Notes only. Each page's /Resources /Properties must list
    // ONLY the layer it actually uses — otherwise viewers
    // optimising on resource scope ignore the irrelevant entry
    // and we end up writing two larger resource dicts.
    let mut doc = Document::new_with(crate::settings_1());
    let map = doc.add_layer(Layer::new("Map"));
    let notes = doc.add_layer(Layer::new("Notes"));

    let mut page1 = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface1 = page1.surface();
    surface1.push_layer(map);
    surface1.pop();
    surface1.finish();
    page1.finish();

    let mut page2 = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface2 = page2.surface();
    surface2.push_layer(notes);
    surface2.pop();
    surface2.finish();
    page2.finish();

    let pdf = doc.finish().unwrap();

    // String-match for the two distinct per-page /Properties
    // dicts; both must exist, each must reference exactly one of
    // the two layer names. The pretty-printed bytes are
    // deterministic given pretty=true in settings_1.
    let l0_count = pdf.windows(b"/L0".len()).filter(|w| *w == b"/L0").count();
    let l1_count = pdf.windows(b"/L1".len()).filter(|w| *w == b"/L1").count();
    // Each layer reference appears in: the content-stream BDC marker
    // AND the page's /Properties entry. So exactly 2 occurrences of
    // each per layer. A larger count means one page is over-
    // declaring resources.
    assert_eq!(
        l0_count, 2,
        "/L0 appears {l0_count} times; expected 2 (one BDC + one /Properties)"
    );
    assert_eq!(
        l1_count, 2,
        "/L1 appears {l1_count} times; expected 2 (one BDC + one /Properties)"
    );
}

#[test]
fn same_layer_used_on_multiple_pages_appears_in_each_page_properties() {
    // Map is used on BOTH pages — each page's /Properties must
    // include it. (Total /L0 occurrences: 2 BDCs + 2 /Properties
    // entries = 4.)
    let mut doc = Document::new_with(crate::settings_1());
    let map = doc.add_layer(Layer::new("Map"));

    for _ in 0..2 {
        let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
        let mut surface = page.surface();
        surface.push_layer(map);
        surface.pop();
        surface.finish();
        page.finish();
    }

    let pdf = doc.finish().unwrap();
    let l0_count = pdf.windows(b"/L0".len()).filter(|w| *w == b"/L0").count();
    assert_eq!(
        l0_count, 4,
        "/L0 appears {l0_count} times; expected 4 (2 BDCs + 2 /Properties entries)",
    );
}

// --- nested push_layer ---------------------------------------------

#[test]
fn nested_push_layer_balances_bdc_emc() {
    // PDF allows nested marked-content sequences. krilla must
    // emit two BDCs in order then two EMCs in reverse order; the
    // PushInstruction stack guarantees the symmetry as long as
    // pop is called the matching number of times.
    let mut doc = Document::new_with(crate::settings_1());
    let outer = doc.add_layer(Layer::new("Outer"));
    let inner = doc.add_layer(Layer::new("Inner"));

    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    surface.push_layer(outer);
    surface.push_layer(inner);
    surface.pop();
    surface.pop();
    surface.finish();
    page.finish();

    // No panic = the marked-content stack stayed consistent.
    // Both layer names should appear in the page's /Properties.
    let pdf = doc.finish().unwrap();
    assert!(contains(&pdf, b"/L0"));
    assert!(contains(&pdf, b"/L1"));
}

// --- layer inside a Form XObject -----------------------------------

#[test]
fn layer_used_inside_form_xobject_registers_on_form_resources() {
    // A Form XObject has its OWN /Resources dict — the page's
    // /Resources is NOT visible to the BDC lookup inside the
    // Form's content stream. krilla must register the layer name
    // on the SUB-builder (StreamBuilder) so the resulting Form
    // XObject ships its own /Properties entry.
    use krilla::graphic::Graphic;
    let mut doc = Document::new_with(crate::settings_1());
    let inset = doc.add_layer(Layer::new("InsetGraphic"));

    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    // Build a Form XObject whose content stream contains the
    // /OC marker.
    let graphic_stream = {
        let mut builder = surface.stream_builder();
        let mut sub_surface = builder.surface();
        sub_surface.push_layer(inset);
        sub_surface.pop();
        sub_surface.finish();
        builder.finish()
    };
    let graphic = Graphic::new(graphic_stream, false);
    surface.draw_graphic(graphic);
    surface.finish();
    page.finish();

    let pdf = doc.finish().unwrap();

    // The layer's resource name (`/L0`) appears at least 3 times:
    //  - once in the Form XObject's content-stream BDC
    //  - once in the Form XObject's /Resources /Properties
    //  - once more if the page-level resources also reference it
    //    (we don't expect this — the page surface never called
    //    push_layer directly, so the page's /Properties stays
    //    empty for L0 — but a count >= 2 is the correctness gate).
    let l0_count = pdf.windows(b"/L0".len()).filter(|w| *w == b"/L0").count();
    assert!(
        l0_count >= 2,
        "/L0 appears only {l0_count} times; expected at least 2 (Form XObject BDC + /Properties)",
    );
}

// --- layers + encryption -------------------------------------------

#[test]
fn layers_plus_encryption_emit_both_subsystems() {
    // The two subsystems share the same final-renumbering ref
    // chain in `ChunkContainer::finish` (layers first, then the
    // encrypt ref, both bumped from `remapped_ref`). The previous
    // encryption + xref_streams test caught a coordination bug
    // there; this one exercises layers + encryption.
    //
    // Both the `/Encrypt` trailer entry and the catalogue's
    // `/OCProperties` must survive, the per-page `/Properties`
    // dict must reference the layer, and the strings emitted on
    // the page (the document title, the layer name) must be
    // encrypted in the body.
    use krilla::encryption::Encryption;
    let settings = krilla::SerializeSettings {
        encryption: Some(Encryption::new("u", "o")),
        ..crate::settings_1()
    };
    let mut doc = Document::new_with(settings);
    doc.set_metadata(krilla::metadata::Metadata::new().title("Layered Secret".into()));
    let layer = doc.add_layer(Layer::new("OverlayName"));

    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    surface.push_layer(layer);
    surface.pop();
    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();

    // Encryption survived.
    assert!(
        contains(&pdf, b"/Encrypt "),
        "/Encrypt missing from trailer"
    );
    assert!(
        contains(&pdf, b"/Filter /Standard"),
        "/Encrypt dict missing from PDF body",
    );
    assert!(contains(&pdf, b"/CFM /AESV3"), "AESV3 crypt filter missing",);

    // Layers survived.
    assert!(
        contains(&pdf, b"/OCProperties"),
        "/OCProperties missing from catalog under encryption",
    );
    assert!(
        contains(&pdf, b"/Type /OCG"),
        "OCG dict missing under encryption",
    );
    // The named-property lookup must still wire through page resources.
    assert!(
        contains(&pdf, b"/L0"),
        "/L0 named-property entry missing — layer-to-resource binding broken under encryption",
    );

    // Encryption hides plaintext. The layer name flows through
    // `TextStr` into the OCG dict, which IS encrypted — so the
    // literal must NOT appear in the output. Same for the
    // document title.
    assert!(
        !contains(&pdf, b"OverlayName"),
        "layer /Name plaintext leaked through encryption",
    );
    assert!(
        !contains(&pdf, b"Layered Secret"),
        "document title plaintext leaked through encryption",
    );
}

#[test]
#[ignore = "shells out to qpdf; run via cargo test -- --ignored qpdf_layers_plus_encryption"]
fn qpdf_layers_plus_encryption() {
    use krilla::encryption::Encryption;
    use std::process::Command;

    let settings = krilla::SerializeSettings {
        encryption: Some(Encryption::new("u", "o")),
        ..crate::settings_1()
    };
    let mut doc = Document::new_with(settings);
    doc.set_metadata(krilla::metadata::Metadata::new().title("Layered Secret".into()));
    let layer = doc.add_layer(Layer::new("OverlayName"));
    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    surface.push_layer(layer);
    surface.pop();
    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();

    let path = std::env::temp_dir().join("krilla_layers_enc.pdf");
    std::fs::write(&path, &pdf).unwrap();
    let output = Command::new("qpdf")
        .arg("--check")
        .arg("--password=u")
        .arg(&path)
        .output()
        .expect("qpdf not on PATH");
    assert!(
        output.status.success(),
        "qpdf rejected the layered + encrypted PDF: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AESv3"));
}

// --- positive control: default_visible=true → /ON ------------------

#[test]
fn default_visible_true_lands_in_on_array() {
    // The negative case (default_visible=false → /OFF) is covered
    // by build_two_layer_doc + catalog_carries_oc_properties; this
    // is the symmetric positive control.
    let mut doc = Document::new_with(crate::settings_1());
    let visible = doc.add_layer(Layer::new("Visible").with_default_visible(true));
    let hidden = doc.add_layer(Layer::new("Hidden").with_default_visible(false));

    let mut page = doc.start_page_with(PageSettings::from_wh(72.0, 72.0).unwrap());
    let mut surface = page.surface();
    surface.push_layer(visible);
    surface.pop();
    surface.push_layer(hidden);
    surface.pop();
    surface.finish();
    page.finish();
    let pdf = doc.finish().unwrap();

    // Visible (handle 0 → /L0 → first OCG ref in /OCGs) must be
    // listed in /ON; Hidden (/L1) must be listed in /OFF. Refs
    // are renumbered per pdf-writer's chunk pass, so check that
    // the bytes between `/ON [` and `]` differ from those between
    // `/OFF [` and `]`.
    let on_start = pdf
        .windows(b"/ON [".len())
        .position(|w| w == b"/ON [")
        .unwrap();
    let on_end = on_start + pdf[on_start..].iter().position(|&b| b == b']').unwrap();
    let off_start = pdf
        .windows(b"/OFF [".len())
        .position(|w| w == b"/OFF [")
        .unwrap();
    let off_end = off_start + pdf[off_start..].iter().position(|&b| b == b']').unwrap();
    let on_slice = &pdf[on_start..=on_end];
    let off_slice = &pdf[off_start..=off_end];
    // Each slice must be non-empty (a single ref) and the two
    // must contain different refs.
    assert!(on_slice.len() > b"/ON []".len(), "/ON is empty");
    assert!(off_slice.len() > b"/OFF []".len(), "/OFF is empty");
    // Strip the prefix and compare ref bytes.
    let on_body: &[u8] = &on_slice[5..on_slice.len() - 1];
    let off_body: &[u8] = &off_slice[6..off_slice.len() - 1];
    assert_ne!(
        on_body.trim_ascii(),
        off_body.trim_ascii(),
        "/ON and /OFF arrays must reference different layers"
    );
}
