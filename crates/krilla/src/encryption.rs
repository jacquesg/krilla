//! AES-256 PDF encryption (Standard Security Handler V=5, R=6 — ISO
//! 32000-2 §7.6.4 "AESV3").
//!
//! Apply to a document by setting
//! [`SerializeSettings::encryption`](crate::SerializeSettings::encryption):
//!
//! ```ignore
//! use krilla::encryption::{Encryption, Permissions};
//! use krilla::SerializeSettings;
//!
//! let settings = SerializeSettings {
//!     encryption: Some(
//!         Encryption::new("alice", "bob")
//!             .with_permissions(Permissions::PRINT | Permissions::COPY),
//!     ),
//!     ..Default::default()
//! };
//! ```
//!
//! Every string and stream emitted into the document is then encrypted
//! transparently with the file encryption key. The `/Encrypt` dict, the
//! trailer `/ID` strings, and (when `encrypt_metadata` is `false`) the
//! metadata stream are written in clear per the spec.

/// Re-exported from `pdf-writer` — the user-facing bit flags for
/// `/Encrypt /P` (ISO 32000-2 §7.6.4.2 Table 22).
///
/// Construct using the associated constants and `|`:
///
/// ```ignore
/// use krilla::encryption::Permissions;
/// let p = Permissions::PRINT | Permissions::COPY | Permissions::FILL_FORM;
/// ```
pub use pdf_writer::Permissions;

/// AES-256 PDF encryption configuration (Standard Security Handler
/// V=5, R=6).
///
/// Constructed via [`Encryption::new`] and the fluent setters. Stored
/// on [`SerializeSettings::encryption`](crate::SerializeSettings::encryption);
/// krilla applies it to the underlying `Pdf` immediately after the
/// header is written, before any indirect object body emission, so
/// every subsequent string and stream is encrypted with the document's
/// file key.
///
/// Passwords are byte slices, not `&str`, because PDF encryption keys
/// are derived from arbitrary octet sequences. For Unicode passwords
/// the caller is responsible for SASLprep normalisation (RFC 4013)
/// before constructing the value. Passwords longer than 127 bytes are
/// silently truncated per ISO 32000-2 §7.6.4.3.2.
#[derive(Debug, Clone)]
pub struct Encryption {
    pub(crate) user_password: Vec<u8>,
    pub(crate) owner_password: Vec<u8>,
    pub(crate) permissions: Permissions,
    pub(crate) encrypt_metadata: bool,
}

impl Encryption {
    /// Create an encryption configuration with the given user and
    /// owner passwords, no permissions, and metadata encryption
    /// enabled.
    pub fn new(
        user_password: impl Into<Vec<u8>>,
        owner_password: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            user_password: user_password.into(),
            owner_password: owner_password.into(),
            permissions: Permissions::NONE,
            encrypt_metadata: true,
        }
    }

    /// Set the permission flags applied when the user password (as
    /// distinct from the owner password) is used to open the document.
    /// Default is [`Permissions::NONE`] (the most restrictive set —
    /// no print, no copy, no annotation, no form fill).
    pub fn with_permissions(mut self, permissions: Permissions) -> Self {
        self.permissions = permissions;
        self
    }

    /// Set whether to encrypt the document's metadata stream. When
    /// `false`, indexers and other tools can read the catalog's
    /// `/Metadata` stream without the password. Default is `true`.
    pub fn with_encrypt_metadata(mut self, encrypt: bool) -> Self {
        self.encrypt_metadata = encrypt;
        self
    }

    /// Translate to the underlying pdf-writer parameter type.
    pub(crate) fn to_pdf_writer(&self) -> pdf_writer::EncryptionParameters {
        pdf_writer::EncryptionParameters::new(
            self.user_password.clone(),
            self.owner_password.clone(),
        )
        .with_permissions(self.permissions)
        .with_encrypt_metadata(self.encrypt_metadata)
    }
}
