//! Unit tests for the `/PrintField` attribute owner on `Tag<kind::Form>`
//! introduced for ISO 32000-2 §14.7.4.4 Table 359 (PDF/UA-1 §7.18 /
//! PDF/UA-2 §8.13). The set/get round-trip is checked here so a
//! regression in the codegen `OrdinalSet` insertion logic is caught
//! before any downstream end-to-end tests run.

#[cfg(test)]
mod tests {
    use crate::page::PageSettings;
    use crate::tagging::{
        ContentTag, FormFieldRole, FormFieldState, SpanTag, Tag, TagGroup, TagTree,
    };
    use crate::{Document, SerializeSettings};

    #[test]
    fn form_set_role_then_set_checked_keeps_both() {
        let mut form = Tag::Form;
        form.set_role(Some(FormFieldRole::CheckBox));
        form.set_checked_state(Some(FormFieldState::On));
        assert_eq!(form.role(), Some(FormFieldRole::CheckBox));
        assert_eq!(form.checked_state(), Some(FormFieldState::On));
    }

    #[test]
    fn form_set_role_then_set_checked_then_set_name_keeps_all_three() {
        let mut form = Tag::Form;
        form.set_role(Some(FormFieldRole::CheckBox));
        form.set_checked_state(Some(FormFieldState::Off));
        form.set_name(Some("agree-to-terms".to_string()));
        assert_eq!(form.role(), Some(FormFieldRole::CheckBox));
        assert_eq!(form.checked_state(), Some(FormFieldState::Off));
        assert_eq!(form.name(), Some("agree-to-terms"));
    }

    /// Pin the PDF serialisation when the Form's id is assigned via
    /// `set_id` AFTER the /Role and /Checked attributes have already
    /// been populated, so `set_id` adds `StructAttr::Id` to the same
    /// `OrdinalSet`. Reproduces the mutation order where the id is
    /// added last, on top of an already-populated attribute set.
    #[test]
    fn form_serialises_when_id_added_after_role_and_checked() {
        let settings = SerializeSettings {
            enable_tagging: true,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);

        let mut tag_tree = TagTree::new();
        let mut form_tag = Tag::Form;
        form_tag.set_role(Some(FormFieldRole::CheckBox));
        form_tag.set_checked_state(Some(FormFieldState::On));
        // After role + checked, the caller sets an id on the
        // TagKind via the universal `as_any_mut` forwarder, which
        // adds StructAttr::Id(0) to the same OrdinalSet.
        let mut kind: crate::tagging::TagKind = form_tag.into();
        kind.set_id(Some(crate::tagging::TagId::from(b"probe".iter().copied())));
        let mut form_group = TagGroup::new(kind);

        let mut page = document.start_page_with(PageSettings::default());
        let mut surface = page.surface();
        let id = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
        surface.end_tagged();
        surface.finish();
        page.finish();
        form_group.push(id);
        tag_tree.push(form_group);
        document.set_tag_tree(tag_tree);

        let pdf = document.finish().unwrap();
        let pdf_str = std::str::from_utf8(&pdf).unwrap_or("");
        assert!(
            pdf.windows(b"/PrintField".len())
                .any(|w| w == b"/PrintField"),
            "expected /PrintField after set_id. PDF dump:\n{pdf_str}"
        );
        assert!(
            pdf.windows(b"/Role".len()).any(|w| w == b"/Role"),
            "expected /Role attribute on the /PrintField dict (regression: set_id wiped Role). PDF dump:\n{pdf_str}"
        );
        assert!(
            pdf.windows(b"/cb".len()).any(|w| w == b"/cb"),
            "expected /cb (checkbox) value on /Role. PDF dump:\n{pdf_str}"
        );
    }

    /// Pin the PDF serialisation of `Tag<kind::Form>` carrying all
    /// three `/PrintField` attribute owner entries (Role, Checked,
    /// Name). The byte stream must contain `/O /PrintField`,
    /// `/Role /cb`, `/checked /on`, and `/Desc (newsletter)` in the
    /// resulting structure-attribute dictionary so PDF/UA-1 §7.18
    /// / PDF/UA-2 §8.13 consumers receive the metadata.
    #[test]
    fn form_serialises_all_three_print_field_attrs() {
        let settings = SerializeSettings {
            enable_tagging: true,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);

        let mut tag_tree = TagTree::new();
        let form_tag = Tag::Form
            .with_role(Some(FormFieldRole::CheckBox))
            .with_checked_state(Some(FormFieldState::On))
            .with_name(Some("newsletter".to_string()));
        let mut form_group = TagGroup::new(form_tag);

        let mut page = document.start_page_with(PageSettings::default());
        let mut surface = page.surface();
        let id = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
        surface.end_tagged();
        surface.finish();
        page.finish();
        form_group.push(id);
        tag_tree.push(form_group);
        document.set_tag_tree(tag_tree);

        let pdf = document.finish().unwrap();
        let pdf_str = std::str::from_utf8(&pdf).unwrap_or("");
        assert!(
            pdf.windows(b"/PrintField".len())
                .any(|w| w == b"/PrintField"),
            "expected /PrintField attribute-owner marker. PDF dump:\n{pdf_str}"
        );
        assert!(
            pdf.windows(b"/Role".len()).any(|w| w == b"/Role"),
            "expected /Role attribute on the /PrintField dict. PDF dump:\n{pdf_str}"
        );
        assert!(
            pdf.windows(b"/cb".len()).any(|w| w == b"/cb"),
            "expected /cb (checkbox) value on /Role. PDF dump:\n{pdf_str}"
        );
        let has_checked = pdf.windows(b"/checked".len()).any(|w| w == b"/checked")
            || pdf.windows(b"/Checked".len()).any(|w| w == b"/Checked");
        assert!(
            has_checked,
            "expected /checked (or PDF 2.0 /Checked) attribute"
        );
        assert!(
            pdf.windows(b"/Desc".len()).any(|w| w == b"/Desc"),
            "expected /Desc attribute on the /PrintField dict"
        );
        assert!(
            pdf.windows(b"newsletter".len()).any(|w| w == b"newsletter"),
            "expected /Desc value literal 'newsletter' in PDF byte stream"
        );
    }
}
