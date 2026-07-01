//! PDF actions, allowing you to add interactivity to the document.
//!
//! PDF has the concept of "actions", which encompass things like navigating to a URL,
//! opening some file on the system, and so on. The PDF reference defines a whole bunch
//! of actions, but krilla does not expose nearly all of them. The currently supported
//! variants are:
//!
//! - [`Action::Link`] — navigate to a URI (`/S /URI`).
//! - [`Action::Goto`] — jump to a named or explicit destination
//!   inside the document (`/S /GoTo`).
//! - [`Action::JavaScript`] — execute an embedded JavaScript script
//!   (`/S /JavaScript`). The script string is written verbatim as the
//!   `/JS` entry per ISO 32000-2 §12.6.4.17. Typical use is the
//!   AcroForm `AFDate_*` / `AFTime_*` / `AFNumber_*` / `AFRange_Validate`
//!   helpers wired through a widget annotation's `/AA` dictionary so
//!   PDF viewers validate and format form input on date / time /
//!   datetime-local / month / week / number / range fields.

use pdf_writer::types::ActionType;
use pdf_writer::{Name, Str, TextStr};

use crate::configure::ValidationError;
use crate::error::KrillaResult;
use crate::interactive::destination::Destination;
use crate::serialize::SerializeContext;

/// A type of action.
#[non_exhaustive]
pub enum Action {
    /// A link action.
    Link(LinkAction),
    /// A go-to action.
    Goto(Destination),
    /// A JavaScript action — executes an embedded ECMAScript snippet
    /// when triggered (ISO 32000-2 §12.6.4.17). The script string is
    /// written into the action dictionary's `/JS` entry verbatim as a
    /// text literal; non-ASCII codepoints are encoded via
    /// `pdf_writer`'s `TextStr` (PDFDocEncoding or UTF-16BE BOM as
    /// required).
    ///
    /// Note that PDF viewers may apply a sandbox to JavaScript actions
    /// (Acrobat's `Trust Manager` settings, Foxit's similar gating);
    /// scripts referencing the AcroForm helpers
    /// (`AFDate_KeystrokeEx`, `AFDate_FormatEx`, `AFTime_Keystroke`,
    /// `AFTime_Format`, `AFNumber_Keystroke`, `AFNumber_Format`,
    /// `AFRange_Validate`, etc.) are typically permitted by default
    /// because they ship as part of the viewer's standard library.
    JavaScript(JavaScriptAction),
}

impl Action {
    pub(crate) fn serialize(
        &self,
        sc: &mut SerializeContext,
        mut action: pdf_writer::writers::Action,
    ) -> KrillaResult<()> {
        match self {
            Action::Link(link) => {
                link.serialize(action);

                Ok(())
            }
            Action::Goto(dest) => {
                let dest_entry = action.action_type(ActionType::GoTo).insert(Name(b"D"));
                dest.serialize(sc, dest_entry)
            }
            Action::JavaScript(js) => {
                // A JavaScript action is forbidden by every JS-restricting
                // validator (PDF/A-1 §6.6.1, PDF/A-2/-3 §6.5.1, PDF/X-1a/-3/-4/-4p). This
                // is the single emission point for JavaScript attached to an
                // annotation `/A` target or a widget `/AA` slot, so register
                // the conformance error here; the document-level `/Names`,
                // `/AA` and `/OpenAction` paths register separately in
                // `chunk_container`. PDF/A-4 (§6.6.2) and PDF/X-6/-6p
                // (ISO 15930-9 §6.14.2) permit JavaScript, so
                // `register_validation_error` filters it through the active
                // validators.
                sc.register_validation_error(ValidationError::ContainsJavaScriptAction(None));
                js.serialize(action);
                Ok(())
            }
        }
    }
}

/// A link action. Will open a link when clicked.
pub struct LinkAction {
    uri: String,
}

impl From<LinkAction> for Action {
    fn from(value: LinkAction) -> Self {
        Action::Link(value)
    }
}

impl LinkAction {
    /// Create a new link action that will open a URI when clicked.
    pub fn new(uri: String) -> Self {
        Self { uri }
    }
}

impl LinkAction {
    fn serialize(&self, mut action: pdf_writer::writers::Action) {
        action
            .action_type(ActionType::Uri)
            .uri(Str(self.uri.as_bytes()));
    }
}

/// A JavaScript action (`/S /JavaScript`) — executes the embedded
/// ECMAScript snippet when triggered (ISO 32000-2 §12.6.4.17).
///
/// The script is owned by the action and serialised into the `/JS`
/// entry as a text string via `pdf_writer`'s `TextStr` writer, which
/// handles PDFDocEncoding and UTF-16BE encoding transparently.
///
/// Typical use: wired through a [`crate::annotation::WidgetAnnotation`]'s
/// `/AA` dictionary (keystroke / format / validate keys) so PDF
/// viewers run the AcroForm `AFDate_*` / `AFTime_*` / `AFRange_Validate`
/// helpers when the user edits a form field.
pub struct JavaScriptAction {
    script: String,
}

impl From<JavaScriptAction> for Action {
    fn from(value: JavaScriptAction) -> Self {
        Action::JavaScript(value)
    }
}

impl JavaScriptAction {
    /// Create a new JavaScript action carrying `script`.
    ///
    /// The string is written verbatim into the PDF action dictionary's
    /// `/JS` entry — no escaping or sanitisation is performed at this
    /// layer. Callers building widget validators are expected to emit
    /// well-formed AcroForm JavaScript (e.g. quoted argument strings,
    /// balanced parentheses).
    pub fn new(script: impl Into<String>) -> Self {
        Self {
            script: script.into(),
        }
    }

    fn serialize(&self, mut action: pdf_writer::writers::Action) {
        action
            .action_type(ActionType::JavaScript)
            .js_string(TextStr(&self.script));
    }
}
