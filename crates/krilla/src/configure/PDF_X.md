# Description
PDF/X is a family of standards for graphic arts and prepress exchange.
krilla supports the following variants, from most restrictive to least:
- PDF/X-1a:2003 (ISO 15930-4) -- CMYK + spot only, no transparency. PDF 1.4.
- PDF/X-3:2003 (ISO 15930-6) -- ICC-based RGB allowed, no transparency. PDF 1.4.
- PDF/X-4 (ISO 15930-7) -- transparency allowed. PDF 1.6.
- PDF/X-4p (ISO 15930-7) -- like X-4 with required external ICC profile reference. PDF 1.6.
- PDF/X-6 (ISO 15930-9) -- based on PDF 2.0.
- PDF/X-6p (ISO 15930-9) -- like X-6 with required external ICC profile reference. PDF 2.0.

PDF/X-1a is a subset of PDF/X-3, which is a subset of PDF/X-4.

krilla also supports combined PDF/A + PDF/X validators:
- `A1B_X1A`: PDF/A-1b + PDF/X-1a (PDF 1.4).
- `A2B_X4`: PDF/A-2b + PDF/X-4 (PDF 1.6).
- `A3B_X4`: PDF/A-3b + PDF/X-4 (PDF 1.6).

See `README.md` for the meaning of each color.

## PDF/X-1a:2003 (ISO 15930-4)

See `crates/krilla/examples/pdf_x1a.rs` for a minimal end-to-end example.
The example uses a bundled compact synthetic CMYK output profile to keep the
generated PDF small; production callers should use a real press/output ICC
profile instead.

### 6.1 File structure

6.1.1: krilla writes the PDF version header as 1.4 (the minimum version krilla supports). `pdf-writer` always writes a binary header comment. 🟢

6.1.2:
- krilla always sets the file ID in the trailer. 🟢
- krilla does not support encryption. 🔵

6.1.3: krilla does not use `LZWDecode`; only `FlateDecode` and `DCTDecode` are used. 🔵

6.1.4: krilla does not use `JBIG2Decode` or `JPXDecode`. 🔵

### 6.2 Graphics

6.2.1: krilla does not write PostScript XObjects. 🔵 krilla only uses PostScript functions (for some gradient types); to be on the safe side, krilla fails export when a PostScript function is used via `ContainsPostScript`. 🟢

6.2.2:
- krilla does not write halftone dictionaries. 🔵
- krilla does not write the `HTP` key. 🔵

6.2.3: krilla does not write transfer functions (`TR`, `TR2` keys). 🔵

6.2.4: krilla does not write overprint settings (`OP`, `op`, `OPM` keys). 🔵

6.2.5: krilla only accepts DeviceCMYK, DeviceGray, and Separation (with CMYK/gray alternate) for page content. Validated via `ContainsRgb` in `RegularColor::color_space()` and in image colour space handling, including gradient stop validation. Plain CMYK fills and strokes serialize as `DeviceCMYK`; the ICC profile remains in the output intent. 🟢

6.2.6: Only TrapNet and PrinterMark annotations are allowed. krilla only supports Link annotations, which are forbidden. Validated via `ContainsAnnotation`. 🟢

6.2.7: krilla forbids transparency via `Transparency`. 🟢

6.2.8: krilla does not write alternate images (`Alternates` key). 🔵

6.2.9: krilla does not write OPI dictionaries. 🔵

6.2.10: krilla does not write reference XObjects. 🔵

### 6.3 Fonts

6.3.1: krilla always embeds all fonts. 🟢

6.3.2: krilla validates that `.notdef` glyphs are not used via `ContainsNotDefGlyph`. 🟢

### 6.4 Metadata

6.4.1:
- krilla writes `/GTS_PDFXVersion` = `"PDF/X-1a:2003"` to the Info dictionary. 🟢
- krilla requires `/Title` via `NoDocumentTitle`. 🟢
- krilla requires `/CreationDate` and `/ModDate` via `MissingDocumentDate`. 🟢
- krilla writes `/Trapped` as `/True` or `/False` (never `/Unknown`), defaulting to `/False`. 🟢

6.4.2:
- krilla writes XMP `pdfxid:GTS_PDFXVersion`. 🟢
- krilla writes XMP `pdf:Trapped` consistent with the Info dict value. 🟢

### 6.5 Output intent

6.5.1:
- krilla writes exactly one output intent with `/S /GTS_PDFX`. 🟢
- krilla always writes `OutputConditionIdentifier` (value "Custom"). 🟢
- krilla references an embedded CMYK ICC profile in `DestOutputProfile`. 🟢
- krilla requires a CMYK ICC profile via `cmyk_profile` and validates via `MissingCMYKProfile`. 🟢

### 6.6 Actions

6.6.1: All actions are forbidden. krilla supports GoTo and URI actions within Link annotations, which are already forbidden via `ContainsAnnotation`. 🟢

6.6.2: krilla does not write JavaScript, Launch, Sound, Movie, or ResetForm actions. 🔵

### 6.7 Embedded files

6.7.1: krilla forbids embedded files via `EmbeddedFile(Existence)`. 🟢

### 6.8 Page boxes

6.8.1: krilla requires a TrimBox or ArtBox on every page via `MissingTrimOrArtBox`. 🟢

6.8.2: The BleedBox, if present, should encompass the TrimBox/ArtBox. 🟣

### Structural limits

krilla enforces PDF 1.4 structural limits: maximum string length (32767), name length (127), array length (8191), dictionary length (4095), float value (32767), indirect objects (8388607), and q/Q nesting (28). 🟢

## PDF/X-3:2003 (ISO 15930-6)

Differences from PDF/X-1a:

### 6.2.5 Colour spaces

krilla allows CalRGB, Lab, and ICCBased 3-component (RGB); DeviceRGB remains forbidden, so all RGB must be ICC-managed. Enforced by `no_device_cs`. krilla uses the embedded `cmyk_profile` as the required printer output intent for PDF/X-3 documents. 🟢

### 6.2.6 Annotations

krilla no longer forbids annotations (PDF/X-3 permits any annotation type with the PRINT flag set). krilla only supports Link annotations, and sets `AnnotationFlags::PRINT` on every annotation via `requires_annotation_flags`. 🟢

### 6.2.4 Separation consistency

krilla requires the same Separation colorant to always use the same tint transform. Validated via `InconsistentSeparationFallback`. 🟢

## PDF/X-4 (ISO 15930-7)

See `crates/krilla/examples/pdf_x4.rs` for a minimal end-to-end example.
The example uses a bundled compact synthetic CMYK output profile to keep the
generated PDF small; production callers should use a real press/output ICC
profile instead.

### File structure

krilla writes PDF version 1.6. 🟢

### Transparency

krilla allows transparency for PDF/X-4. 🟢

### Colour spaces

krilla conservatively enforces `no_device_cs` for page content, which is stricter than the standard but always compliant. The embedded PDF/X output intent uses the caller-provided `cmyk_profile`, which must be a printer/output profile. 🟢

### Identification

- krilla writes XMP `pdfxid:GTS_PDFXVersion` = `"PDF/X-4"` as the authoritative identifier. 🟢
- krilla additionally writes `/GTS_PDFXVersion` in the Document Info dictionary. ISO 15930-7 only requires the XMP form, but the Info-dict entry is permitted and improves downstream-tool compatibility. 🟢
- krilla does not require `/Title`. 🟢
- krilla relaxes PDF 1.4 structural limits (string length, array length, etc.). 🟢

### Separation consistency

Same requirement as PDF/X-3. 🟢

## PDF/X-4p (ISO 15930-7)

Same as PDF/X-4 except:

See `crates/krilla/examples/pdf_x4p.rs` for a minimal end-to-end example.

### Output intent

- krilla references the ICC profile externally via `DestOutputProfileRef` instead of embedding it. `external_output_profile` in `SerializeSettings` is required for `X4P`; otherwise krilla reports `MissingExternalOutputProfile`. 🟢
- When provided, krilla writes a `DestOutputProfileRef` dictionary with profile metadata and URL file specifications, and does not write an embedded `/DestOutputProfile` for the PDF/X output intent. 🟢
- `ExternalOutputProfile::rgb/luma/cmyk` reject malformed input (empty URLs / identifier / info) eagerly at construction time, returning `Err(ExternalOutputProfileError::*)`. 🟢
- krilla rejects external profile settings for validators other than `X4P` and `X6P` via `ExternalOutputProfileUnsupportedByValidator`. 🟢
- Generic PDF 1.x validators may report version warnings for `DestOutputProfileRef` because they validate the base PDF model rather than the PDF/X-4p conformance level. This is expected and does not by itself indicate a broken PDF/X-4p file. 🟣

## PDF/X-6 (ISO 15930-9)

krilla writes PDF version 2.0. Requirements are similar to PDF/X-4 but based on PDF 2.0 features. Separation consistency is enforced. 🟢

### Identification and trapping (PDF 2.0 interaction)

PDF 2.0 deprecated most entries in the Document Information dictionary in favour of XMP metadata. ISO 15930-9 (PDF/X-6/-6p) nevertheless **still mandates `/Trapped` in the Info dictionary**, and permits `/GTS_PDFXVersion` there. krilla therefore always writes:

- `/Trapped` (`/True` or `/False`, never `/Unknown`; defaults to `/False` when unset) in the Info dict. 🟢
- `/GTS_PDFXVersion` = `"PDF/X-6"` / `"PDF/X-6p"` in the Info dict. 🟢
- `pdf:Trapped` and `pdfxid:GTS_PDFXVersion` in XMP, kept consistent with the Info-dict values. 🟢

The `Trapping::Unknown` variant of the public `Metadata::trapped` API is downgraded to `Trapping::NotTrapped` under any PDF/X validator (and therefore under X-6/X-6p), because PDF/X forbids the `Unknown` state. Outside PDF/X, `Unknown` is written verbatim. 🟢

## PDF/X-6p (ISO 15930-9)

Same as PDF/X-6 except:

See `crates/krilla/examples/pdf_x6p.rs` for a minimal end-to-end example.

### Output intent

- krilla references the ICC profile externally via `DestOutputProfileRef` instead of embedding it. `external_output_profile` in `SerializeSettings` is required for `X6P`; otherwise krilla reports `MissingExternalOutputProfile`. 🟢
- When provided, krilla writes a `DestOutputProfileRef` dictionary with profile metadata and URL file specifications, and does not write an embedded `/DestOutputProfile` for the PDF/X output intent. 🟢
- `ExternalOutputProfile::rgb/luma/cmyk` reject malformed input (empty URLs / identifier / info) eagerly at construction time, returning `Err(ExternalOutputProfileError::*)`. 🟢
- krilla rejects external profile settings for validators other than `X4P` and `X6P` via `ExternalOutputProfileUnsupportedByValidator`. 🟢

## Combined PDF/A + PDF/X validators

For combined validators, the most restrictive requirement from each standard applies (union of restrictions). krilla writes both `GTS_PDFA1` and `GTS_PDFX` output intents, both `pdfaid` and `pdfxid` XMP metadata, and includes PDF/A extension schemas. 🟢

krilla intentionally does not provide a combined PDF/A + PDF/X-4p or PDF/A + PDF/X-6p validator. The external output-profile reference used by the `p` variants is forbidden in PDF/X output intents by PDF/A-2 and PDF/A-3. 🟢

| Composite | PDF Version | Key inherited restrictions |
|---|---|---|
| `A1B_X1A` | 1.4 | CMYK only, no transparency, no embedded files, structural limits, TrimBox/ArtBox. |
| `A2B_X4` | 1.6 | ICC-based colours, separation consistency, image interpolation, TrimBox/ArtBox. |
| `A3B_X4` | 1.6 | ICC-based colours, separation consistency, embedded file metadata, TrimBox/ArtBox. |

## Validation tooling notes

There is no open-source PDF/X conformance validator; veraPDF covers PDF/A and PDF/UA only, and the Arlington PDF model validates against base-PDF object structures rather than any PDF/X profile. This has two practical consequences for krilla's CI (`.github/workflows/ci.yml`) and for anyone running these tools against krilla output:

### Arlington deviations

Arlington's checker reports three expected deviations on conformant PDF/X output. The CI workflow excludes these snapshots from Arlington; the comment in `ci.yml` references this section.

| Snapshot | Arlington model | Reported deviation | Reason krilla writes it |
|---|---|---|---|
| `validate_pdf_x4p_full_example.pdf` | `arlington1.7` | rejects `DestOutputProfileRef` | ISO 15930-7 §6.2.3.5 defines `DestOutputProfileRef` for PDF/X-4p; base PDF 1.7 has no equivalent. |
| `validate_pdf_x6_full_example.pdf` | `arlington2.0` | `DocInfo-Trapped-5`, `DocInfoEntry-5` | PDF 2.0 deprecates Document Info-dict entries generally; ISO 15930-9 nevertheless mandates `/Trapped` in the Info dict. |
| `validate_pdf_x6p_full_example.pdf` | `arlington2.0` | `DocInfo-Trapped-5`, `DocInfoEntry-5` | Same as X-6, plus `DestOutputProfileRef` as per X-4p. |

The unrelated `validate_pdf_a4f_full_example.pdf` exclusion tracks [pdf-association/arlington-pdf-model#132](https://github.com/pdf-association/arlington-pdf-model/issues/132) and is not PDF/X-specific.

### veraPDF scope

veraPDF is run only against `validate_pdf_a*.pdf` snapshots. Running it against PDF/X snapshots would produce a mix of irrelevant PDF/A warnings and silent passes because veraPDF has no PDF/X profile. PDF/X conformance in CI is covered by:

1. The unit/integration test suite (`crates/krilla-tests/src/validate.rs`) — exercises every `ValidationError` variant and every per-validator requirement.
2. Byte-exact snapshot tests under `refs/snapshots/validate_pdf_x*_full_example.txt` — detect any unintended change to output intents, XMP, Info dict, trim/art boxes, etc.
3. The lightweight **Verify PDF/X structural markers** CI step — `grep -aq`s each generated `validate_pdf_x*.pdf` for the presence of `GTS_PDFX`, `GTS_PDFXVersion`, and `Trapped` as a last-line structural smoke test.

For end-to-end PDF/X validation against a commercial preflight tool (Acrobat Preflight, PitStop, callas pdfaPilot, etc.), run those manually against the bundled examples under `crates/krilla/examples/pdf_x*.rs`.
