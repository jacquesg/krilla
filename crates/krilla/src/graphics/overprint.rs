//! Overprint control (ISO 32000-2 §8.6.7).
//!
//! Overprinting controls how a painting operation behaves when the device's
//! current colour space defines colorants (typically CMYK and DeviceN). With
//! overprint disabled (the default), painted colours replace the underlying
//! values for every colorant. With overprint enabled, the underlying values
//! are preserved on colorants that the source does not paint, allowing inks
//! to be combined on press.
//!
//! Two flags and one mode are exposed:
//!
//! - `OP` — the stroking overprint flag.
//! - `op` — the non-stroking (fill) overprint flag.
//! - `OPM` — the overprint mode: how to behave for colorants whose source
//!   value is zero.
//!
//! Defaults (when an [`Overprint`] is never installed on the graphics state)
//! match the PDF default: every flag is `false`, `OPM` is
//! [`OverprintMode::OverrideAllColorants`] (i.e. `0`).

/// How to behave when overprinting for colorants whose source value is
/// zero (ISO 32000-2 §8.6.7, the `OPM` entry).
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Hash)]
pub enum OverprintMode {
    /// `OPM 0` — an overprint operation always overrides the underlying
    /// colorant, even when the source value is zero. This is the PDF
    /// default.
    #[default]
    OverrideAllColorants,
    /// `OPM 1` — an overprint operation only overrides the underlying
    /// colorant when the source value is non-zero. Forbidden by PDF/A
    /// for ICCBased colour spaces when overprinting is enabled.
    IgnoreZeroChannel,
}

impl OverprintMode {
    pub(crate) fn to_pdf(self) -> pdf_writer::types::OverprintMode {
        match self {
            OverprintMode::OverrideAllColorants => {
                pdf_writer::types::OverprintMode::OverrideAllColorants
            }
            OverprintMode::IgnoreZeroChannel => pdf_writer::types::OverprintMode::IgnoreZeroChannel,
        }
    }
}

/// Overprint settings for a graphics-state scope (ISO 32000-2 §8.6.7).
///
/// Every field is optional. Because the graphics state accumulates only
/// the flags you set, an unset field takes the PDF default (`false` for
/// `OP`/`op`, `OverrideAllColorants` for `OPM`) rather than inheriting —
/// and krilla always writes `OP` and `op` together, so a set `OP` never
/// couples onto the non-stroking parameter (ISO 32000-2 Table 57).
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Hash)]
pub struct Overprint {
    /// `OP` — stroking-operation overprint flag.
    pub stroking: Option<bool>,
    /// `op` — non-stroking (fill) operation overprint flag.
    pub non_stroking: Option<bool>,
    /// `OPM` — overprint mode.
    pub mode: Option<OverprintMode>,
}

impl Overprint {
    /// Convenience constructor for the common case of enabling both
    /// stroking and non-stroking overprint with [`OverprintMode::IgnoreZeroChannel`]
    /// — i.e. the "preserve underlying inks for unspecified channels"
    /// behaviour matching CSS `print-color-adjust: exact` overprint
    /// semantics and PDF/X commercial-print expectations.
    pub fn preserve() -> Self {
        Self {
            stroking: Some(true),
            non_stroking: Some(true),
            mode: Some(OverprintMode::IgnoreZeroChannel),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.stroking.is_none() && self.non_stroking.is_none() && self.mode.is_none()
    }
}
