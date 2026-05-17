//! API-shape tests for the
//! [`ShapeOptimisation`](krilla::ShapeOptimisation) enum and its
//! plumbing through [`SerializeSettings`].
//!
//! Krilla does not currently apply any path simplification, so the
//! field has no observable effect on the PDF content stream. These
//! tests cover the parse-and-store contract only: the default is
//! `Auto`, and each of the three variants can be supplied via
//! struct-update syntax without disturbing the rest of the settings.
//! Full semantic coverage is deferred until a real simplification
//! pass lands.

use krilla::{SerializeSettings, ShapeOptimisation};

#[test]
fn shape_optimisation_default_is_auto() {
    let settings = SerializeSettings::default();
    assert_eq!(settings.shape_optimisation, ShapeOptimisation::Auto);
}

#[test]
fn shape_optimisation_can_be_set_to_none() {
    let settings = SerializeSettings {
        shape_optimisation: ShapeOptimisation::None,
        ..Default::default()
    };
    assert_eq!(settings.shape_optimisation, ShapeOptimisation::None);
}

#[test]
fn shape_optimisation_can_be_set_to_full() {
    let settings = SerializeSettings {
        shape_optimisation: ShapeOptimisation::Full,
        ..Default::default()
    };
    assert_eq!(settings.shape_optimisation, ShapeOptimisation::Full);
}
