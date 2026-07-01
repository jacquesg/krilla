//! API-shape tests for the
//! [`ShapeOptimization`] enum and its
//! plumbing through [`SerializeSettings`].
//!
//! Krilla does not currently apply any path simplification, so the
//! field has no observable effect on the PDF content stream. These
//! tests cover the parse-and-store contract only: the default is
//! `Auto`, and each of the three variants can be supplied via
//! struct-update syntax without disturbing the rest of the settings.
//! Full semantic coverage is deferred until a real simplification
//! pass lands.

use krilla::{SerializeSettings, ShapeOptimization};

#[test]
fn shape_optimization_default_is_auto() {
    let settings = SerializeSettings::default();
    assert_eq!(settings.shape_optimization, ShapeOptimization::Auto);
}

#[test]
fn shape_optimization_can_be_set_to_none() {
    let settings = SerializeSettings {
        shape_optimization: ShapeOptimization::None,
        ..Default::default()
    };
    assert_eq!(settings.shape_optimization, ShapeOptimization::None);
}

#[test]
fn shape_optimization_can_be_set_to_full() {
    let settings = SerializeSettings {
        shape_optimization: ShapeOptimization::Full,
        ..Default::default()
    };
    assert_eq!(settings.shape_optimization, ShapeOptimization::Full);
}
