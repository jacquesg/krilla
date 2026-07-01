//! Destinations in a PDF document.
//!
//! In some cases, you might want to refer to other locations within the same document, for
//! example when defining the outline, or when link to a different section in the document
//! from a link. To achieve this, you can use destinations, which are associated with a page
//! and a specific location on that page.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use pdf_writer::{Obj, Ref, Str};
use tiny_skia_path::Transform;

use crate::chunk_container::ChunkContainer;
use crate::error::{KrillaError, KrillaResult};
use crate::geom::{Point, Rect};
use crate::serialize::{PageInfo, SerializeContext};

/// The type of destination.
#[derive(Hash)]
pub enum Destination {
    /// An XYZ destination.
    Xyz(XyzDestination),
    /// A named destination.
    Named(NamedDestination),
}

impl Destination {
    pub(crate) fn serialize(&self, sc: &mut SerializeContext, buffer: Obj) -> KrillaResult<()> {
        match self {
            Destination::Xyz(xyz) => {
                let ref_ = sc.register_xyz_destination(xyz.clone());
                buffer.primitive(ref_);

                Ok(())
            }
            Destination::Named(named) => named.serialize(sc, buffer),
        }
    }
}

/// A destination associated with a name.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct NamedDestination {
    pub(crate) name: Arc<String>,
    pub(crate) xyz_dest: Arc<XyzDestination>,
}

impl From<NamedDestination> for Destination {
    fn from(val: NamedDestination) -> Self {
        Destination::Named(val)
    }
}

impl NamedDestination {
    /// Create a new named destination.
    ///
    /// When used as part of a link annotation, the destination will be automatically registered
    /// with the [`Document`](crate::Document).
    ///
    /// That said, you can also manually register a destination without linking to it by calling
    /// [`Document::register_named_destination`](crate::Document::register_named_destination).
    pub fn new(name: String, xyz_dest: XyzDestination) -> Self {
        Self {
            name: Arc::new(name),
            xyz_dest: Arc::new(xyz_dest),
        }
    }

    pub(crate) fn serialize(
        &self,
        sc: &mut SerializeContext,
        destination: Obj,
    ) -> KrillaResult<()> {
        sc.register_named_destination(self.clone())
            .ok_or_else(|| KrillaError::DuplicateNamedDestination(Arc::clone(&self.name)))?;
        destination.primitive(Str(self.name.as_bytes()));

        Ok(())
    }
}

/// Selects the PDF destination operator emitted for an
/// [`XyzDestination`] (ISO 32000-2 §12.3.2.2 Table 149).
///
/// The default for `XyzDestination` is the implicit `/XYZ <left>
/// <top> 0` (zoom unchanged) command. The other variants cover the
/// fit-window flavours used by PDFreactor's `-ro-destination-area`
/// property family and by CSS author intent expressed via
/// moegoe's `-bd-destination-area` cascade reader:
///
/// | Variant            | PDF operator              | Notes                                        |
/// |--------------------|---------------------------|----------------------------------------------|
/// | [`Xyz`]            | `/XYZ <left> <top> 0`     | Implicit default; preserves the legacy path. |
/// | [`Fit`]            | `/Fit`                    | Fit the whole page in the viewport.          |
/// | [`FitBoundingBox`] | `/FitB`                   | Fit the page content bounding box (PDF 1.1+).|
/// | [`FitHorizontal`]  | `/FitH <top>`             | Fit page width, scroll to `top`.             |
/// | [`FitVertical`]    | `/FitV <left>`            | Fit page height, scroll to `left`.           |
/// | [`FitRect`]        | `/FitR <l> <b> <r> <t>`   | Fit the supplied rectangle.                  |
///
/// `/FitH` and `/FitV` take a single offset; krilla reuses the `y`
/// (resp. `x`) component of the `XyzDestination::point` for that
/// offset so embedders need only supply the existing target point.
/// `/FitR` requires an explicit rectangle, supplied at construction
/// time via [`FitMode::Rect`].
///
/// [`Xyz`]: FitMode::Xyz
/// [`Fit`]: FitMode::Fit
/// [`FitBoundingBox`]: FitMode::FitBoundingBox
/// [`FitHorizontal`]: FitMode::FitHorizontal
/// [`FitVertical`]: FitMode::FitVertical
/// [`FitRect`]: FitMode::Rect
#[derive(Clone, Copy, Debug)]
pub enum FitMode {
    /// `/XYZ <left> <top> 0` — explicit point destination (default).
    Xyz,
    /// `/Fit` — fit the entire page in the viewport.
    Fit,
    /// `/FitB` — fit the page content bounding box (PDF 1.1+).
    FitBoundingBox,
    /// `/FitH <top>` — fit page width and scroll to the destination
    /// `y` coordinate; the `x` coordinate is ignored.
    FitHorizontal,
    /// `/FitV <left>` — fit page height and scroll to the destination
    /// `x` coordinate; the `y` coordinate is ignored.
    FitVertical,
    /// `/FitR <l> <b> <r> <t>` — fit the supplied rectangle (PDF user
    /// space, top-left origin like every other krilla API).
    Rect(Rect),
}

impl Hash for FitMode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Discriminant first so unrelated variants do not collide.
        std::mem::discriminant(self).hash(state);
        if let FitMode::Rect(rect) = self {
            let tsp = rect.to_tsp();
            tsp.left().to_bits().hash(state);
            tsp.top().to_bits().hash(state);
            tsp.right().to_bits().hash(state);
            tsp.bottom().to_bits().hash(state);
        }
    }
}

impl PartialEq for FitMode {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FitMode::Xyz, FitMode::Xyz)
            | (FitMode::Fit, FitMode::Fit)
            | (FitMode::FitBoundingBox, FitMode::FitBoundingBox)
            | (FitMode::FitHorizontal, FitMode::FitHorizontal)
            | (FitMode::FitVertical, FitMode::FitVertical) => true,
            (FitMode::Rect(a), FitMode::Rect(b)) => {
                let at = a.to_tsp();
                let bt = b.to_tsp();
                at.left().to_bits() == bt.left().to_bits()
                    && at.top().to_bits() == bt.top().to_bits()
                    && at.right().to_bits() == bt.right().to_bits()
                    && at.bottom().to_bits() == bt.bottom().to_bits()
            }
            _ => false,
        }
    }
}

impl Eq for FitMode {}

#[derive(Debug)]
struct XyzDestRepr {
    page_index: usize,
    point: Point,
    fit_mode: FitMode,
}

impl Hash for XyzDestRepr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.page_index.hash(state);
        self.point.x.to_bits().hash(state);
        self.point.y.to_bits().hash(state);
        self.fit_mode.hash(state);
    }
}

impl PartialEq for XyzDestRepr {
    fn eq(&self, other: &Self) -> bool {
        self.page_index == other.page_index
            && self.point.x == other.point.x
            && self.point.y == other.point.y
            && self.fit_mode == other.fit_mode
    }
}

impl Eq for XyzDestRepr {}

/// A destination pointing to a specific location at a specific page.
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct XyzDestination(Arc<XyzDestRepr>);

impl From<XyzDestination> for Destination {
    fn from(val: XyzDestination) -> Self {
        Destination::Xyz(val)
    }
}

impl XyzDestination {
    /// Create a new XYZ destination. `page_index` should be the index (i.e. number) of the
    /// target page, and point indicates the specific location on that page that should be
    /// targeted. If the `page_index` is out of range, export will panic.
    pub fn new(page_index: usize, point: Point) -> Self {
        Self(Arc::new(XyzDestRepr {
            page_index,
            point,
            fit_mode: FitMode::Xyz,
        }))
    }

    /// Override the destination operator emitted at serialisation
    /// time (ISO 32000-2 §12.3.2.2 Table 149). Without this call the
    /// destination emits the implicit `/XYZ <left> <top> 0`
    /// command; setting a [`FitMode`] swaps the operator for the
    /// corresponding fit-window variant. See the [`FitMode`] enum for
    /// the variant-to-operator mapping.
    ///
    /// PDFreactor's `-ro-destination-area` and moegoe's
    /// `-bd-destination-area` author surface project onto this
    /// setter when the embedder requests anything other than the
    /// default `auto` area.
    pub fn with_fit_mode(self, fit_mode: FitMode) -> Self {
        let inner = XyzDestRepr {
            page_index: self.0.page_index,
            point: self.0.point,
            fit_mode,
        };
        Self(Arc::new(inner))
    }

    pub(crate) fn serialize(
        &self,
        sc: &mut SerializeContext,
        chunk_container: &mut ChunkContainer,
        root_ref: Ref,
    ) {
        let chunk = &mut chunk_container.non_stream.destinations;
        let destination = chunk.destination(root_ref);

        let page_info = sc.page_infos().get(self.0.page_index).unwrap_or_else(|| {
            panic!(
                "attempted to link to page {}, but document only has {} pages",
                self.0.page_index + 1,
                sc.page_infos().len()
            )
        });

        let (ref_, surface_size) = match page_info {
            PageInfo::Krilla {
                ref_, surface_size, ..
            } => (ref_, surface_size),
            PageInfo::Pdf { ref_, size, .. } => (ref_, size),
        };

        let page_ref = *ref_;
        let page_size = surface_size.height();

        let mut mapped_point = self.0.point.to_tsp();
        // Convert to PDF coordinates (top-left author origin -> PDF
        // bottom-left user space).
        let invert_transform = Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, page_size);
        invert_transform.map_point(&mut mapped_point);

        // ISO 32000-2 §12.3.2.2 Table 149 — pick the destination
        // operator from the embedder-configured [`FitMode`]. The
        // default `FitMode::Xyz` preserves the legacy emission path.
        let destination = destination.page(page_ref);
        match self.0.fit_mode {
            FitMode::Xyz => {
                destination.xyz(mapped_point.x, mapped_point.y, None);
            }
            FitMode::Fit => {
                destination.fit();
            }
            FitMode::FitBoundingBox => {
                destination.fit_bounding_box();
            }
            FitMode::FitHorizontal => {
                destination.fit_horizontal(mapped_point.y);
            }
            FitMode::FitVertical => {
                destination.fit_vertical(mapped_point.x);
            }
            FitMode::Rect(rect) => {
                // Map the rectangle through the same author->PDF
                // y-flip applied to the destination point.
                let tsp = rect.to_tsp();
                let mut tl = tiny_skia_path::Point::from_xy(tsp.left(), tsp.top());
                let mut br = tiny_skia_path::Point::from_xy(tsp.right(), tsp.bottom());
                invert_transform.map_point(&mut tl);
                invert_transform.map_point(&mut br);
                // After the y-flip, the original top edge becomes
                // the maximum y in PDF user space; PDFreactor and
                // Acrobat both expect /FitR `[l b r t]` where
                // `b <= t`.
                let (l, r) = if tl.x <= br.x {
                    (tl.x, br.x)
                } else {
                    (br.x, tl.x)
                };
                let (b, t) = if tl.y <= br.y {
                    (tl.y, br.y)
                } else {
                    (br.y, tl.y)
                };
                let pdf_rect = pdf_writer::Rect::new(l, b, r, t);
                destination.fit_rect(pdf_rect);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::annotation::{LinkAnnotation, Target};
    use crate::error::KrillaError;
    use crate::geom::{Point, Rect};
    use crate::Document;

    use super::{NamedDestination, XyzDestination};

    #[test]
    fn named_duplicate_rejected() {
        let mut document = Document::new();
        assert_eq!(
            document.register_named_destination(NamedDestination::new(
                "same".to_string(),
                XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
            )),
            Some(())
        );
        assert_eq!(
            document.register_named_destination(NamedDestination::new(
                "same".to_string(),
                XyzDestination::new(0, Point::from_xy(100.0, 100.0)),
            )),
            None
        );
    }

    #[test]
    fn named_duplicate_same_location_allowed() {
        let mut document = Document::new();
        assert_eq!(
            document.register_named_destination(NamedDestination::new(
                "same".to_string(),
                XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
            )),
            Some(())
        );
        assert_eq!(
            document.register_named_destination(NamedDestination::new(
                "same".to_string(),
                XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
            )),
            Some(())
        );

        assert!(document.finish().is_ok());
    }

    #[test]
    fn named_duplicate_annotation_rejected() {
        let mut document = Document::new();
        let mut page = document.start_page();

        page.add_annotation(
            LinkAnnotation::new(
                Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap(),
                Target::Destination(
                    NamedDestination::new(
                        "same".to_string(),
                        XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
                    )
                    .into(),
                ),
            )
            .into(),
        );
        page.add_annotation(
            LinkAnnotation::new(
                Rect::from_xywh(0.0, 100.0, 100.0, 100.0).unwrap(),
                Target::Destination(
                    NamedDestination::new(
                        "same".to_string(),
                        XyzDestination::new(0, Point::from_xy(100.0, 100.0)),
                    )
                    .into(),
                ),
            )
            .into(),
        );
        drop(page);

        assert_eq!(
            document.finish(),
            Err(KrillaError::DuplicateNamedDestination(Arc::new(
                "same".to_string()
            )))
        );
    }

    #[test]
    fn named_duplicate_manual_then_annotation_rejected() {
        let mut document = Document::new();
        assert_eq!(
            document.register_named_destination(NamedDestination::new(
                "same".to_string(),
                XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
            )),
            Some(())
        );

        let mut page = document.start_page();
        page.add_annotation(
            LinkAnnotation::new(
                Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap(),
                Target::Destination(
                    NamedDestination::new(
                        "same".to_string(),
                        XyzDestination::new(0, Point::from_xy(100.0, 100.0)),
                    )
                    .into(),
                ),
            )
            .into(),
        );
        drop(page);

        assert_eq!(
            document.finish(),
            Err(KrillaError::DuplicateNamedDestination(Arc::new(
                "same".to_string()
            )))
        );
    }

    // -------------------------------------------------------------------
    // moegoe S12 — `XyzDestination::with_fit_mode`. Fork-only setter
    // that swaps the destination operator emitted by
    // `XyzDestination::serialize` from the implicit `/XYZ` to one of
    // the `/Fit*` variants (ISO 32000-2 §12.3.2.2 Table 149). The
    // tests below assert that each `FitMode` variant projects onto the
    // expected PDF operator in the serialised byte stream.
    // -------------------------------------------------------------------

    use super::FitMode;
    use crate::page::PageSettings;
    use crate::SerializeSettings;

    fn finish_with_destination(fit_mode: FitMode) -> Vec<u8> {
        let settings = SerializeSettings {
            pretty: true,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);
        let mut page = document.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
        page.add_annotation(
            LinkAnnotation::new(
                Rect::from_xywh(0.0, 0.0, 50.0, 50.0).unwrap(),
                Target::Destination(
                    XyzDestination::new(0, Point::from_xy(10.0, 20.0))
                        .with_fit_mode(fit_mode)
                        .into(),
                ),
            )
            .into(),
        );
        drop(page);
        document
            .finish()
            .expect("document serialisation should succeed")
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn fit_mode_horizontal_emits_fith_operator() {
        let pdf = finish_with_destination(FitMode::FitHorizontal);
        assert!(
            contains(&pdf, b"/FitH"),
            "FitMode::FitHorizontal must emit /FitH in the destination array"
        );
        assert!(
            !contains(&pdf, b"/XYZ"),
            "FitMode::FitHorizontal must replace the default /XYZ operator"
        );
    }

    #[test]
    fn fit_mode_vertical_emits_fitv_operator() {
        let pdf = finish_with_destination(FitMode::FitVertical);
        assert!(
            contains(&pdf, b"/FitV"),
            "FitMode::FitVertical must emit /FitV in the destination array"
        );
    }

    #[test]
    fn fit_mode_fit_emits_fit_operator() {
        let pdf = finish_with_destination(FitMode::Fit);
        // `/Fit` is a substring of `/FitB`/`/FitH`/`/FitV`/`/FitR`; the
        // pretty serialiser writes the operator followed by a newline
        // (the destination array closes immediately after the name).
        assert!(
            contains(&pdf, b"/Fit\n") || contains(&pdf, b"/Fit ") || contains(&pdf, b"/Fit]"),
            "FitMode::Fit must emit a bare /Fit operator"
        );
    }

    #[test]
    fn fit_mode_bounding_box_emits_fitb_operator() {
        let pdf = finish_with_destination(FitMode::FitBoundingBox);
        assert!(
            contains(&pdf, b"/FitB"),
            "FitMode::FitBoundingBox must emit /FitB"
        );
    }

    #[test]
    fn fit_mode_rect_emits_fitr_operator() {
        let pdf = finish_with_destination(FitMode::Rect(
            Rect::from_xywh(5.0, 10.0, 30.0, 40.0).unwrap(),
        ));
        assert!(
            contains(&pdf, b"/FitR"),
            "FitMode::Rect must emit /FitR with the supplied rectangle"
        );
    }

    #[test]
    fn fit_mode_default_xyz_preserves_legacy_emission() {
        // Without a `with_fit_mode` call the destination should
        // continue to emit `/XYZ <left> <top> 0` exactly as it did
        // before the FitMode extension landed.
        let settings = SerializeSettings {
            pretty: true,
            ..Default::default()
        };
        let mut document = Document::new_with(settings);
        let mut page = document.start_page_with(PageSettings::from_wh(200.0, 200.0).unwrap());
        page.add_annotation(
            LinkAnnotation::new(
                Rect::from_xywh(0.0, 0.0, 50.0, 50.0).unwrap(),
                Target::Destination(XyzDestination::new(0, Point::from_xy(10.0, 20.0)).into()),
            )
            .into(),
        );
        drop(page);
        let pdf = document.finish().expect("document should serialise");
        assert!(
            contains(&pdf, b"/XYZ"),
            "default XyzDestination must keep emitting /XYZ when no FitMode is set"
        );
    }
}
