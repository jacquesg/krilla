//! Optional content (PDF "layers"), per ISO 32000-1 §8.11 and
//! ISO 32000-2 §8.11.
//!
//! A `Layer` declares a section of content that the PDF viewer can show
//! or hide interactively. Register a layer at the document level with
//! [`Document::add_layer`](crate::Document::add_layer); it returns a
//! [`LayerHandle`] that can then be passed to
//! [`Surface::push_layer`](crate::surface::Surface::push_layer) to
//! bracket drawing operations that belong to that layer:
//!
//! ```ignore
//! use krilla::optional_content::Layer;
//!
//! let map = document.add_layer(Layer::new("Map"));
//! let notes = document.add_layer(
//!     Layer::new("Notes").with_default_visible(false),
//! );
//!
//! surface.push_layer(map);
//! // ...draw the base map...
//! surface.pop();
//!
//! surface.push_layer(notes);
//! // ...draw the overlay annotations...
//! surface.pop();
//! ```
//!
//! krilla automatically emits `/OCProperties` on the catalog at
//! finalise time, listing every registered layer in `/OCGs` and
//! recording the default visibility configuration in `/D`. The
//! marked-content sequence carries `/OC` pointing at the relevant
//! indirect OCG dictionary so consumers can suppress hidden layers
//! without parsing the resource tree.

/// A single optional content group (PDF layer).
///
/// Created via [`Layer::new`] and registered with
/// [`Document::add_layer`](crate::Document::add_layer). The returned
/// [`LayerHandle`] is then passed to
/// [`Surface::push_layer`](crate::surface::Surface::push_layer) /
/// [`Surface::pop`](crate::surface::Surface::pop) to bracket content
/// that should appear under this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub(crate) name: String,
    pub(crate) default_visible: bool,
    pub(crate) intent: LayerIntent,
}

impl Layer {
    /// Create a layer with the given user-visible name.
    ///
    /// Defaults: visible (the viewer shows the layer on first open),
    /// intent [`LayerIntent::View`] (the layer is honoured during
    /// normal rendering).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            default_visible: true,
            intent: LayerIntent::View,
        }
    }

    /// Set whether the layer is on by default. When `false`, the
    /// layer's ref appears in the `/OFF` array of the default
    /// configuration so the viewer opens the document with the
    /// layer hidden.
    pub fn with_default_visible(mut self, visible: bool) -> Self {
        self.default_visible = visible;
        self
    }

    /// Set the layer's `/Intent` per ISO 32000-2 §8.11.2.1. The
    /// default is [`LayerIntent::View`].
    pub fn with_intent(mut self, intent: LayerIntent) -> Self {
        self.intent = intent;
        self
    }
}

/// The intent of an optional content group, per ISO 32000-2
/// §8.11.2.1. Distinguishes layers a viewer should honour when
/// rendering (`View`) from layers that exist only for design-time
/// tooling (`Design`).
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum LayerIntent {
    /// The layer represents a user-visible variant of the document.
    /// Default if unset.
    View,
    /// The layer carries design-time metadata. Conforming readers
    /// ignore these layers during ordinary rendering.
    Design,
}

impl LayerIntent {
    pub(crate) fn to_pdf_writer(self) -> pdf_writer::types::OptionalContentIntent {
        match self {
            Self::View => pdf_writer::types::OptionalContentIntent::View,
            Self::Design => pdf_writer::types::OptionalContentIntent::Design,
        }
    }
}

/// An opaque handle returned by
/// [`Document::add_layer`](crate::Document::add_layer). Pass it to
/// [`Surface::push_layer`](crate::surface::Surface::push_layer) to
/// bracket content with the layer.
///
/// Layer handles are stable for the lifetime of the document; they
/// are indexes into the document's internal layer registry, not raw
/// indirect refs.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct LayerHandle(pub(crate) u32);
