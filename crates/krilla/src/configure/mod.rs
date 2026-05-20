//! Configuring PDF version and export mode.
//!
//! See [`Validator`] for validator-specific requirements. Repository-level
//! implementation notes for PDF/X live in `PDF_X.md` next to this module.

pub mod validate;
mod version;

pub use validate::{ValidationError, Validator};
pub use version::PdfVersion;

/// A configuration of validator and PDF version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configuration {
    validator: Validator,
    version: PdfVersion,
}

impl Default for Configuration {
    fn default() -> Self {
        Self::new()
    }
}

impl Configuration {
    /// Create a new configuration from a validator and a PDF version.
    ///
    /// Returns `None` if the configuration is invalid.
    pub fn new_with(validator: Validator, version: PdfVersion) -> Option<Self> {
        if validator.compatible_with_version(version) {
            Some(Self { validator, version })
        } else {
            None
        }
    }

    /// Create a new configuration from a validator. An appropriate PDF
    /// version will be set automatically.
    pub fn new_with_validator(validator: Validator) -> Self {
        Self::new_with(validator, validator.recommended_version()).expect(
            "recommended_version is invariant-compatible with the validator — see \
             Validator::recommended_version and Validator::compatible_with_version",
        )
    }

    /// Create a new configuration from a PDF version and no validator.
    pub fn new_with_version(version: PdfVersion) -> Self {
        Self::new_with(Validator::None, version)
            .expect("Validator::None is compatible with every PDF version")
    }

    /// Create a new configuration without any validator.
    pub fn new() -> Self {
        Self::new_with_validator(Validator::None)
    }

    /// Return the validator of the configuration.
    pub fn validator(&self) -> Validator {
        self.validator
    }

    /// Return the PDF version of the configuration.
    pub fn version(&self) -> PdfVersion {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use crate::configure::{Configuration, PdfVersion, Validator};

    #[test]
    fn invalid_combination_1() {
        assert_eq!(
            Configuration::new_with(Validator::A1_B, PdfVersion::Pdf17),
            None
        );
    }

    #[test]
    fn invalid_x1a_pdf17() {
        assert_eq!(
            Configuration::new_with(Validator::X1A, PdfVersion::Pdf17),
            None
        );
    }

    #[test]
    fn invalid_x4_pdf14() {
        assert_eq!(
            Configuration::new_with(Validator::X4, PdfVersion::Pdf14),
            None
        );
    }

    #[test]
    fn valid_x4_pdf16() {
        assert!(Configuration::new_with(Validator::X4, PdfVersion::Pdf16).is_some());
    }

    #[test]
    fn invalid_x4_pdf17() {
        assert_eq!(
            Configuration::new_with(Validator::X4, PdfVersion::Pdf17),
            None
        );
    }

    #[test]
    fn invalid_x6_pdf17() {
        assert_eq!(
            Configuration::new_with(Validator::X6, PdfVersion::Pdf17),
            None
        );
    }

    #[test]
    fn valid_x6_pdf20() {
        assert!(Configuration::new_with(Validator::X6, PdfVersion::Pdf20).is_some());
    }

    #[test]
    fn valid_x6p_pdf20() {
        assert!(Configuration::new_with(Validator::X6P, PdfVersion::Pdf20).is_some());
    }

    #[test]
    fn invalid_x6p_pdf17() {
        assert_eq!(
            Configuration::new_with(Validator::X6P, PdfVersion::Pdf17),
            None
        );
    }

    #[test]
    fn invalid_a2b_x4_pdf14() {
        assert_eq!(
            Configuration::new_with(Validator::A2B_X4, PdfVersion::Pdf14),
            None
        );
    }

    #[test]
    fn recommended_versions() {
        assert_eq!(Validator::X1A.recommended_version(), PdfVersion::Pdf14);
        assert_eq!(Validator::X3.recommended_version(), PdfVersion::Pdf14);
        assert_eq!(Validator::X4.recommended_version(), PdfVersion::Pdf16);
        assert_eq!(Validator::X6.recommended_version(), PdfVersion::Pdf20);
        assert_eq!(Validator::X6P.recommended_version(), PdfVersion::Pdf20);
        assert_eq!(Validator::A1B_X1A.recommended_version(), PdfVersion::Pdf14);
        assert_eq!(Validator::A2B_X4.recommended_version(), PdfVersion::Pdf16);
        assert_eq!(Validator::UA1.recommended_version(), PdfVersion::Pdf17);
        assert_eq!(Validator::UA2.recommended_version(), PdfVersion::Pdf20);
        assert_eq!(Validator::WTPDF.recommended_version(), PdfVersion::Pdf20);
    }

    #[test]
    fn valid_ua2_pdf20() {
        assert!(Configuration::new_with(Validator::UA2, PdfVersion::Pdf20).is_some());
    }

    #[test]
    fn invalid_ua2_pdf17() {
        assert_eq!(
            Configuration::new_with(Validator::UA2, PdfVersion::Pdf17),
            None
        );
    }

    #[test]
    fn valid_wtpdf_pdf20() {
        assert!(Configuration::new_with(Validator::WTPDF, PdfVersion::Pdf20).is_some());
    }

    #[test]
    fn invalid_wtpdf_pdf17() {
        assert_eq!(
            Configuration::new_with(Validator::WTPDF, PdfVersion::Pdf17),
            None
        );
    }
}
