//! Digital signatures on PDF documents (ISO 32000-2 §12.8).
//!
//! krilla itself does not embed any cryptographic stack — embedders
//! supply the PKCS#7 / CMS detached-signature bytes through the
//! [`DigitalSignature::signer`] callback. krilla owns the byte-range
//! arithmetic, the placeholder padding, and the post-finish patching
//! of `/ByteRange` and `/Contents`.
//!
//! The flow:
//!
//! 1. The caller attaches a [`SignatureField`](super::annotation::SignatureField)
//!    widget through the usual AcroForm path and configures the
//!    document with [`Document::with_digital_signature`](crate::Document::with_digital_signature).
//! 2. During serialisation krilla allocates an indirect `/Sig`
//!    dictionary, wires its ref into the widget's `/V`, sets
//!    `/AcroForm /SigFlags 3`, and emits placeholder `/ByteRange`
//!    and `/Contents <…>` values whose textual length is fixed by
//!    [`DigitalSignature::placeholder_size_bytes`].
//! 3. After `pdf-writer` has produced the final byte buffer, krilla
//!    locates the placeholder markers, fills in the correct
//!    `/ByteRange [0 a b c]` triple, concatenates the two
//!    byte-range slices, invokes the embedder's signer callback,
//!    and writes the hex-encoded result into the `/Contents`
//!    placeholder (right-padded with `0` if the signature is
//!    shorter than the reservation).
//!
//! The signer callback receives the *bytes that are about to be
//! covered by the signature* — krilla has already computed the
//! correct byte range. It returns raw DER-encoded PKCS#7 /
//! `SignedData` bytes; krilla handles the hex encoding.
//!
//! ## Compliance scope
//!
//! - Basic CMS detached signatures (`/SubFilter /adbe.pkcs7.detached`)
//!   per ISO 32000-2 §12.8.3.3 — supported.
//! - CAdES (`/SubFilter /ETSI.CAdES.detached`) per ISO 32000-2 §12.8.3.4 —
//!   the embedder is free to emit CAdES PKCS#7 bytes; krilla writes
//!   whichever `SubFilter` the caller selects.
//! - PAdES-LTV (long-term-validation with DSS / VRI dictionaries
//!   per ETSI EN 319 142) — **not in scope**. The DSS dictionary
//!   carries revocation info, OCSP responses and a `/DocTimeStamp`
//!   chain that require additional infrastructure (an OCSP /
//!   trusted-timestamp source) outside the signing callback.

use crate::error::KrillaResult;

/// Sub-filter identifying the signature format encoded in
/// `/Contents`. Per ISO 32000-2 §12.8.1, the `/Filter` is always
/// `/Adobe.PPKLite` for signatures krilla writes — `/SubFilter`
/// selects between the two interoperable CMS profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignatureSubFilter {
    /// PKCS#7 detached — the legacy Adobe profile and the default
    /// for `signPDF` output from PDFreactor, Prince, Antenna House
    /// and BFO. Bytes in `/Contents` are a DER-encoded
    /// `SignedData` structure whose `encapContentInfo.eContent`
    /// is **absent** (detached); the signed message is the
    /// document byte range described by `/ByteRange`.
    #[default]
    AdbePkcs7Detached,
    /// CAdES detached — ETSI TS 102 778 / ETSI EN 319 142 profile
    /// over the same `SignedData` container as
    /// [`AdbePkcs7Detached`](Self::AdbePkcs7Detached) but with
    /// additional signed attributes required by the CAdES
    /// envelopes. Selecting this variant does **not** make krilla
    /// produce CAdES-conformant bytes; the embedder is responsible
    /// for emitting a CAdES `SignedData` from the signer callback.
    EtsiCAdesDetached,
}

impl SignatureSubFilter {
    /// PDF name written to `/SubFilter` for this sub-filter.
    pub(crate) fn as_pdf_name(&self) -> &'static [u8] {
        match self {
            Self::AdbePkcs7Detached => b"adbe.pkcs7.detached",
            Self::EtsiCAdesDetached => b"ETSI.CAdES.detached",
        }
    }
}

/// Signer callback. Receives the document byte range that will be
/// covered by the signature (already excluding the `/Contents`
/// placeholder) and returns the raw DER-encoded PKCS#7 / CMS
/// `SignedData` bytes — krilla hex-encodes the result into the
/// `/Contents` slot.
///
/// The closure is `FnOnce` because every krilla `Document::finish`
/// produces exactly one signature; rerunning `finish` against a
/// fresh `Document` requires a fresh signer.
pub type SignerFn = Box<dyn FnOnce(&[u8]) -> KrillaResult<Vec<u8>> + Send>;

/// Digital signature configuration attached to a [`Document`](crate::Document).
///
/// Pairs a signer callback with the metadata fields that land in
/// the signature dictionary alongside `/ByteRange` and
/// `/Contents` (ISO 32000-2 §12.8.1, Table 252).
pub struct DigitalSignature {
    /// Signer callback. See [`SignerFn`].
    pub(crate) signer: SignerFn,
    /// Sub-filter declaring the CMS profile in `/Contents`.
    /// Default is [`SignatureSubFilter::AdbePkcs7Detached`].
    pub(crate) sub_filter: SignatureSubFilter,
    /// Maximum number of raw signature bytes (i.e. half the number
    /// of hex chars between `<` and `>` in `/Contents`) reserved
    /// in the placeholder. Must be at least as large as the
    /// PKCS#7 / CMS blob the signer will return.
    ///
    /// Default is 8192 bytes (16384 hex chars), which comfortably
    /// fits an RSA-2048 / SHA-256 signature plus the signer
    /// certificate, an intermediate, and a small set of signed
    /// attributes. CAdES-B-T or chains of more than one
    /// intermediate may require a larger reservation.
    pub(crate) placeholder_size_bytes: usize,
    /// `/Reason` — free-form text explaining why the signature
    /// was applied. `None` omits the entry.
    pub(crate) reason: Option<String>,
    /// `/Location` — the host system / geographic location at
    /// signing time. `None` omits the entry.
    pub(crate) location: Option<String>,
    /// `/ContactInfo` — a contact channel for the signer.
    /// `None` omits the entry.
    pub(crate) contact_info: Option<String>,
    /// `/Name` — the human-readable name of the signer. PDF
    /// readers display this near the signature widget. `None`
    /// omits the entry; readers fall back to the signer
    /// certificate's CN.
    pub(crate) signer_name: Option<String>,
    /// `/M` — signing time as a literal PDF date string
    /// (e.g. `D:20260527140000Z`). `None` omits the entry.
    pub(crate) signing_time: Option<String>,
}

impl DigitalSignature {
    /// Build a digital signature with the given signer callback.
    /// All other fields default to `None` / `AdbePkcs7Detached`
    /// / 8 KiB placeholder.
    pub fn new(signer: SignerFn) -> Self {
        Self {
            signer,
            sub_filter: SignatureSubFilter::default(),
            placeholder_size_bytes: 8192,
            reason: None,
            location: None,
            contact_info: None,
            signer_name: None,
            signing_time: None,
        }
    }

    /// Override the `/SubFilter`.
    pub fn with_sub_filter(mut self, sub_filter: SignatureSubFilter) -> Self {
        self.sub_filter = sub_filter;
        self
    }

    /// Override the placeholder size reserved for the raw
    /// signature bytes.
    pub fn with_placeholder_size_bytes(mut self, bytes: usize) -> Self {
        self.placeholder_size_bytes = bytes;
        self
    }

    /// Set `/Reason`.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Set `/Location`.
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Set `/ContactInfo`.
    pub fn with_contact_info(mut self, contact_info: impl Into<String>) -> Self {
        self.contact_info = Some(contact_info.into());
        self
    }

    /// Set `/Name`.
    pub fn with_signer_name(mut self, signer_name: impl Into<String>) -> Self {
        self.signer_name = Some(signer_name.into());
        self
    }

    /// Set `/M` to a literal PDF date string.
    pub fn with_signing_time(mut self, signing_time: impl Into<String>) -> Self {
        self.signing_time = Some(signing_time.into());
        self
    }
}

impl std::fmt::Debug for DigitalSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DigitalSignature")
            .field("sub_filter", &self.sub_filter)
            .field("placeholder_size_bytes", &self.placeholder_size_bytes)
            .field("reason", &self.reason)
            .field("location", &self.location)
            .field("contact_info", &self.contact_info)
            .field("signer_name", &self.signer_name)
            .field("signing_time", &self.signing_time)
            .field("signer", &"<closure>")
            .finish()
    }
}

/// Sentinel placeholder value emitted into each of the three
/// rewritable `/ByteRange` slots before post-finish patching.
///
/// 1_000_000_000 is the smallest positive `i32` that requires
/// exactly [`BYTE_RANGE_FIELD_WIDTH`] decimal digits, so the
/// `pdf-writer` integer emission produces a slot of constant byte
/// width. The patcher locates the array by scanning for the
/// `/ByteRange` key followed by this exact byte sequence three
/// times.
pub(crate) const BYTE_RANGE_PLACEHOLDER_VALUE: i32 = 1_000_000_000;

/// Width of each placeholder field — the number of decimal digits
/// in [`BYTE_RANGE_PLACEHOLDER_VALUE`]. Post-finish patching
/// substitutes the real offset/length values, left-padded with
/// `0`s to this width, so the array byte width is preserved.
pub(crate) const BYTE_RANGE_FIELD_WIDTH: usize = 10;

/// Full ASCII byte pattern of the `/ByteRange` placeholder as
/// `pdf-writer` emits it. `pdf-writer` uses no inter-array
/// whitespace by default (only `pretty` mode injects it), and the
/// signing path emits with the production `pretty: false`
/// configuration. Matches `[0 1000000000 1000000000 1000000000]`
/// when the document was serialised pretty, or the more compact
/// `[0 1000000000 1000000000 1000000000]` when not.
///
/// We tolerate both forms by scanning for the unique key prefix
/// `/ByteRange` and then for three back-to-back occurrences of
/// the placeholder decimal value, skipping any inter-token
/// whitespace.
pub(crate) const BYTE_RANGE_KEY: &[u8] = b"/ByteRange";

/// The decimal representation of [`BYTE_RANGE_PLACEHOLDER_VALUE`]
/// — exactly [`BYTE_RANGE_FIELD_WIDTH`] bytes long.
pub(crate) const BYTE_RANGE_PLACEHOLDER_DECIMAL: &[u8] = b"1000000000";

/// Patch the byte range and signature into the emitted PDF buffer.
///
/// Steps (ISO 32000-2 §12.8.1.1):
/// 1. Locate the unique `[0 9999999999 9999999999 9999999999]`
///    placeholder — krilla emits exactly one per document.
/// 2. Locate the `/Contents <00000…>` placeholder — its position
///    and length are known from the
///    [`SerializeContext`](crate::serialize::SerializeContext)
///    bookkeeping; we re-derive them here by searching for the
///    distinctive marker.
/// 3. Compute the byte range `[0 a b c]` where:
///    a = byte offset of the opening `<` of `/Contents`
///    b = byte offset of the byte AFTER the closing `>`
///    c = total length - b
/// 4. Overwrite the placeholder integers in `/ByteRange`.
/// 5. Concatenate the two byte-range slices (`0..a` and `b..b+c`),
///    invoke the signer, hex-encode the result, right-pad with
///    `0` to the placeholder width, write into `/Contents`.
pub(crate) fn patch_signature(
    mut buffer: Vec<u8>,
    signature: DigitalSignature,
) -> KrillaResult<Vec<u8>> {
    use crate::error::KrillaError;

    // Locate the `/ByteRange` array. We scan for the unique key
    // `/ByteRange` then walk forward over the opening `[` and the
    // literal `0` (first byte-range slot, always zero) before
    // arriving at the three rewritable placeholder slots.
    let br_key_pos = find_subsequence(&buffer, BYTE_RANGE_KEY).ok_or_else(|| {
        KrillaError::DigitalSignature("/ByteRange key not found in PDF buffer".into())
    })?;
    let array_open = skip_whitespace_to(&buffer, br_key_pos + BYTE_RANGE_KEY.len(), b'[')?;
    // The first slot is always `0`; find it and walk past it.
    let mut cursor = skip_whitespace(&buffer, array_open + 1);
    if buffer.get(cursor) != Some(&b'0') {
        return Err(KrillaError::DigitalSignature(
            "/ByteRange first slot is not the literal `0`".into(),
        ));
    }
    cursor += 1;
    // Three placeholder slots — record each opening offset.
    let mut slot_offsets = [0usize; 3];
    for (i, slot) in slot_offsets.iter_mut().enumerate() {
        cursor = skip_whitespace(&buffer, cursor);
        if buffer.get(cursor..cursor + BYTE_RANGE_FIELD_WIDTH)
            != Some(BYTE_RANGE_PLACEHOLDER_DECIMAL)
        {
            return Err(KrillaError::DigitalSignature(format!(
                "/ByteRange placeholder slot {} did not match the expected pattern",
                i
            )));
        }
        *slot = cursor;
        cursor += BYTE_RANGE_FIELD_WIDTH;
    }
    // Locate the `/Contents` literal-string placeholder. pdf-writer's
    // ASCII `Str` path writes `(0000…0000)` with N zeros (where N
    // is `placeholder_size_bytes * 2`). The sig dict's `/Contents`
    // appears AFTER `/ByteRange` within the same `/Sig` indirect
    // object; we restrict the search to bytes after the byte-range
    // array so a page's `/Contents <ref>` (which appears earlier in
    // the PDF) does not steal the match.
    let search_start = cursor;
    let contents_key_rel =
        find_subsequence(&buffer[search_start..], b"/Contents").ok_or_else(|| {
            KrillaError::DigitalSignature(
                "/Contents key not found after /ByteRange — signature dict missing or malformed"
                    .into(),
            )
        })?;
    let contents_key_pos = search_start + contents_key_rel;
    let contents_open = skip_whitespace_to(&buffer, contents_key_pos + b"/Contents".len(), b'(')?;
    let contents_close = contents_open + 1 + signature.placeholder_size_bytes * 2;
    if buffer.get(contents_close) != Some(&b')') {
        return Err(KrillaError::DigitalSignature(
            "/Contents placeholder size mismatch with reserved bytes".into(),
        ));
    }
    // Verify the placeholder body is all `'0'` ASCII — refuses to
    // patch a malformed buffer.
    for &byte in &buffer[contents_open + 1..contents_close] {
        if byte != b'0' {
            return Err(KrillaError::DigitalSignature(
                "/Contents placeholder body contains non-zero ASCII bytes".into(),
            ));
        }
    }

    let total_len = buffer.len();
    let a = contents_open; // first byte of `(`
    let b = contents_close + 1; // first byte after `)`
    let c = total_len - b;

    // Rewrite the three `/ByteRange` placeholder slots in place.
    // Real values are left-padded with `0`s to BYTE_RANGE_FIELD_WIDTH.
    write_decimal_padded(
        &mut buffer[slot_offsets[0]..slot_offsets[0] + BYTE_RANGE_FIELD_WIDTH],
        a,
    );
    write_decimal_padded(
        &mut buffer[slot_offsets[1]..slot_offsets[1] + BYTE_RANGE_FIELD_WIDTH],
        b - a,
    );
    write_decimal_padded(
        &mut buffer[slot_offsets[2]..slot_offsets[2] + BYTE_RANGE_FIELD_WIDTH],
        c,
    );

    // Replace the literal-string placeholder with a hex-string of
    // the same total byte width: `(` becomes `<`, `)` becomes `>`,
    // and the N zeros between them are overwritten with the hex
    // encoding of the signature bytes (right-padded with `0` to
    // fill the reservation).
    let mut to_sign = Vec::with_capacity(a + c);
    to_sign.extend_from_slice(&buffer[0..a]);
    to_sign.extend_from_slice(&buffer[b..b + c]);

    let sig_bytes = (signature.signer)(&to_sign)?;
    if sig_bytes.len() > signature.placeholder_size_bytes {
        return Err(KrillaError::DigitalSignature(format!(
            "signer returned {} bytes, exceeds reserved placeholder of {} bytes",
            sig_bytes.len(),
            signature.placeholder_size_bytes
        )));
    }

    buffer[a] = b'<';
    buffer[contents_close] = b'>';
    let mut hex_buf = vec![b'0'; signature.placeholder_size_bytes * 2];
    for (i, byte) in sig_bytes.iter().enumerate() {
        let pair = byte_to_hex_pair(*byte);
        hex_buf[i * 2] = pair[0];
        hex_buf[i * 2 + 1] = pair[1];
    }
    buffer[a + 1..contents_close].copy_from_slice(&hex_buf);

    Ok(buffer)
}

/// Advance past PDF whitespace bytes (space, tab, CR, LF, NUL,
/// form-feed per ISO 32000-2 §7.2.3).
fn skip_whitespace(buf: &[u8], mut idx: usize) -> usize {
    while idx < buf.len() && is_pdf_whitespace(buf[idx]) {
        idx += 1;
    }
    idx
}

/// Advance past whitespace, then assert the next byte is `expect`,
/// returning the index of `expect`. Fails with a diagnostic
/// message identifying the surrounding context.
fn skip_whitespace_to(buf: &[u8], start: usize, expect: u8) -> KrillaResult<usize> {
    use crate::error::KrillaError;
    let idx = skip_whitespace(buf, start);
    if buf.get(idx) != Some(&expect) {
        return Err(KrillaError::DigitalSignature(format!(
            "expected `{}` at offset {}, found `{}`",
            expect as char,
            idx,
            buf.get(idx).map(|b| *b as char).unwrap_or('\0'),
        )));
    }
    Ok(idx)
}

fn is_pdf_whitespace(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

fn write_decimal_padded(slot: &mut [u8], value: usize) {
    // Left-pad with `0`s so the slot width stays constant. The
    // slot itself is `BYTE_RANGE_FIELD_WIDTH` bytes long.
    let s = format!("{:0>w$}", value, w = slot.len());
    debug_assert_eq!(s.len(), slot.len());
    slot.copy_from_slice(s.as_bytes());
}

fn byte_to_hex_pair(byte: u8) -> [u8; 2] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    [HEX[(byte >> 4) as usize], HEX[(byte & 0x0F) as usize]]
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Sentinel comment emitted by `chunk_container` immediately
/// before the signature dictionary so the post-finish patcher
/// can locate the dict without parsing the whole PDF. The
/// comment itself is ignored by every spec-conformant PDF
/// consumer (ISO 32000-2 §7.2.4 "Comments").
pub(crate) const SIGNATURE_DICT_START_MARKER: &[u8] = b"%krilla-digital-signature-dict";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_decimal_has_expected_width() {
        // The decimal form of [`BYTE_RANGE_PLACEHOLDER_VALUE`] must
        // be exactly [`BYTE_RANGE_FIELD_WIDTH`] bytes long so the
        // post-finish patcher can overwrite it without shifting
        // downstream offsets.
        assert_eq!(BYTE_RANGE_PLACEHOLDER_DECIMAL.len(), BYTE_RANGE_FIELD_WIDTH);
        let parsed: i32 = std::str::from_utf8(BYTE_RANGE_PLACEHOLDER_DECIMAL)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(parsed, BYTE_RANGE_PLACEHOLDER_VALUE);
    }

    #[test]
    fn write_decimal_padded_round_trip() {
        let mut slot = [b'_'; 10];
        write_decimal_padded(&mut slot, 12345);
        assert_eq!(&slot, b"0000012345");
        write_decimal_padded(&mut slot, 0);
        assert_eq!(&slot, b"0000000000");
        write_decimal_padded(&mut slot, 9_999_999_999);
        assert_eq!(&slot, b"9999999999");
    }

    #[test]
    fn skip_whitespace_handles_pdf_separators() {
        let buf = b"  \t\r\n  X";
        assert_eq!(skip_whitespace(buf, 0), 7);
    }
}
