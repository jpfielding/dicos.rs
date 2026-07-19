# Migration Guide: dicos.rs 1.x → 2.0

This document tracks the breaking API changes across the 2.0 release. All five
workspace crates move to **2.0.0** together (single coordinated publish).

The headline of 2.0 is **honest standards conformance**: the 1.0.0 codecs
overstated what they emitted. 2.0 makes the codec output conformant within an
explicit, documented scope and rejects everything legal-but-unsupported loudly.
Files written by 1.0.0 remain decodable (see the jpeg2k / jpegls rows below).

## `dicos` core (Workstream 3)

### PR A3 — Types API

| Area | 1.x | 2.0 | How to migrate |
| --- | --- | --- | --- |
| `GrayImage` fields | `img.width`, `img.height`, `img.data` were public | fields are private; use accessors | `img.width()`, `img.height()`, `img.data()` (`&[T]`), `img.data_mut()` (`&mut [T]`), or `img.into_data()` (owned `Vec<T>`). Construct with `GrayImage::new` / `GrayImage::from_data` as before. |
| `Dataset::iter()` | `impl Iterator<Item = (&Tag, &Element)>` | `impl Iterator<Item = &Element>` | Replace `for (tag, elem) in ds.iter()` with `for elem in ds.iter()` and read the tag from `elem.tag`. For tags only, use the new `ds.tags() -> impl Iterator<Item = Tag>`. |
| `Dataset::rows()` | `u16`, defaulted to `0` when absent | `Option<u16>` (no invented default) | `ds.rows().unwrap_or(0)` to preserve old behavior, or map `None` to a `DicosError::MissingAttribute`. |
| `Dataset::columns()` | `u16`, defaulted to `0` | `Option<u16>` | `ds.columns().unwrap_or(0)`, or raise `MissingAttribute`. |
| `Dataset::bits_allocated()` | `u16`, defaulted to `16` | `Option<u16>` | `ds.bits_allocated().unwrap_or(16)` to keep the old fallback. |
| `Dataset::pixel_representation()` | `u16`, defaulted to `0` | `Option<u16>` | `ds.pixel_representation().unwrap_or(0)` to keep the old fallback. |
| `Dataset::number_of_frames()` | `u32`, default `1` | **unchanged** — `u32`, default `1` | No change; the `1` default is DICOM-defined (absent = single frame). |
| `Element` fields | public | **unchanged** — still public | No change. `vr`/`value` coherence is now documented as the caller's responsibility. |
| `Value` / `PixelData` enums | exhaustive | `#[non_exhaustive]` | Add a wildcard `_ => …` arm to any `match` on these outside the `dicos` crate so future variants don't break compilation. |
| Root re-exports | `Codec`, `CodecError`, `GrayImage` | adds `Dataset`, `Element`, `Value`, `PixelData`, `Tag`, `Vr`, `parse`, `parse_with_limit`, `parse_with_warnings`, `parse_with_warnings_and_limit`, `ParseWarning`, `DicosError`, `write` | Prefer `dicos::Dataset` etc. over the fully-qualified module paths. |

### PR A4 — Codec registry

| Area | 1.x | 2.0 | How to migrate |
| --- | --- | --- | --- |
| `decode_frame(data, w, h, ts_uid)` | fell back to sniffing when the transfer syntax was unknown | unknown/unsupported transfer syntax **always** returns `DicosError::UnsupportedTransferSyntax`; no sniff fallback | If you relied on content detection, call the new `decode_frame_sniffed(data, w, h)` explicitly. |
| `sniff_codec(data)` | guessed RLE from a leading segment-count; matched the JP2 box magic; scanned up to 4 KB for JPEG SOF markers | RLE guess removed (RLE has no signature — select via transfer syntax); JP2-box magic removed (decoder is codestream-only, keeps only `FF 4F` SOC); JPEG detection is now a bounded, structured marker walk to the first SOF (`FF F7` → JPEG-LS, `FF C3` → JPEG Lossless) | For RLE, pass the transfer syntax UID to `decode_frame`/`codec_for_transfer_syntax`. JP2-boxed streams are no longer sniffed. |
| Codec error mapping | backend errors flattened to `CodecError::InvalidData(e.to_string())` | backend errors map to `CodecError::Backend { codec, source }`, preserving the original via `std::error::Error::source` | Match on `CodecError::Backend { codec, source }` (or `#[non_exhaustive]` fallthrough) instead of parsing strings. |

### PR A1/A2 — Typed errors, `#[non_exhaustive]`, ParseWarning

| Area | 1.x | 2.0 | How to migrate |
| --- | --- | --- | --- |
| Error enums | string-ish variants; exhaustive | `DicosError` and every codec `CodecError`/error enum are `#[non_exhaustive]`, with typed variants (`Backend { codec, source }`, `BadPreamble`, `NestingTooDeep`, `UnexpectedTag`, `LengthExceedsLimit`, `LengthExceedsBuffer`, `UndefinedLengthInFixedSq`, `Truncated { offset, context }`, `InvalidParameter { name, value, allowed }`; `InvalidFile` is the documented last resort) | Add a wildcard `_ => …` arm to every `match` on these enums or it will not compile. Prefer matching the specific typed variants above instead of parsing message strings. |
| Truncation semantics | partial element headers were silently tolerated | clean EOF at an element boundary is `Ok`; a partial header (1–3 bytes) is `Truncated { offset, context }`; `skip_bytes` verifies the copied count | If you fed deliberately-truncated data and relied on a lenient parse, handle `Truncated` explicitly. |
| Parse warnings | none | `ParseWarning { OddPixelDataLength, PixelCountMismatch }`; `parse` logs them, `parse_with_warnings` / `parse_with_warnings_and_limit` return them alongside the `Dataset`. Warnings are **not** stored on `Dataset`. | Use `parse_with_warnings` if you need to inspect non-fatal anomalies; otherwise no change (`parse` still returns the `Dataset`). |

## Codec crates (Workstreams 1 & 2)

### `pure_jpegls` (T.87)

| Area | 1.x | 2.0 | How to migrate |
| --- | --- | --- | --- |
| Bitstream conformance | non-conformant (T.81-style `0xFF00` stuffing, no run mode, uncapped Golomb, non-spec statistics) | default output is **ITU-T T.87 conformant** single-component (`Nf = 1`, `ILV = 0`): single-bit stuffing, length-limited Golomb, run mode, LSE ID=1 presets honored, near-lossless | Files written by 1.0.0 are **not** byte-compatible with the new default. To read/write the old bytes, select `Profile::LegacyGo` via `encode_with_options` / `decode_with_options`. |
| Profiles / options | `encode(pixels, w, h)` only | `EncodeOptions { near, profile, precision }`, `DecodeOptions { profile }`; `encode`/`decode` unchanged (delegate to `Profile::T87` defaults) | For near-lossless set `EncodeOptions { near, .. }` and call `encode_with_options`. `LegacyGo` requires `near = 0`. |
| Scope guards | silently mis-decoded unsupported inputs | `DRI`/restart markers, `Nf ≠ 1`, and `ILV ≠ 0` return `CodecError::Unsupported` | Multi-component or interleaved sources must be handled outside this crate. |

### `pure_jpeg2k` (T.800)

| Area | 1.x | 2.0 | How to migrate |
| --- | --- | --- | --- |
| Codestream conformance | emitted **raw DWT coefficients**, not JPEG 2000 (no entropy coding, unreadable by any conformant decoder) | emits **real ITU-T T.800** lossless codestreams (EBCOT + MQ), single tile, single unsigned-16-bit component, LRCP, 1 layer; verified against OpenJPEG both directions | New output is a genuine `.j2c`. Downstream tools that only accepted the old raw format must be updated (they never accepted anything standard before). |
| Reading 1.0.0 / Go files | that was the only format | `decode()` uses `DecodeOptions { legacy: LegacyPolicy::Auto }`. `Auto` fingerprints the QCD first: a conformant reversible 16-bit stream can never carry all-zero quantization exponents, so that signature unambiguously identifies a legacy raw-DWT payload and only then is the legacy decoder used; everything else decodes as standard T.800. `LegacyOnly` / `StandardOnly` also available. | No change for `decode()`. To refuse any legacy payload, pass `LegacyPolicy::StandardOnly`. |
| Registry adapter | n/a | the `dicos` registry adapter decodes with `LegacyPolicy::StandardOnly` (a legacy stream under the standard transfer-syntax UID is a collision risk) | For legacy archives, decode via the codec crate directly with `LegacyPolicy::Auto`/`LegacyOnly`. |
| Support matrix | silent on unsupported features | multiple tiles/components, non-16-bit or signed precision, MCT ≠ 0, progression ≠ LRCP, layers ≠ 1, `cb_style ≠ 0`, transform ≠ 5/3, non-zero origins, and `POC/COC/QCC/PPM/PPT/PLM/PLT`, `SOP/EPH` all return `CodecError::Unsupported` | Handle these categories out of band; they were never decoded correctly before. |

### `pure_jpegli` (T.81 Process 14 SV1)

| Area | 1.x | 2.0 | How to migrate |
| --- | --- | --- | --- |
| Restart intervals | streams using restart intervals were **silently corrupted** | restart intervals supported, **row-aligned** per H.1.1 (DRI value = `rows × width`); mid-row restarts are illegal and rejected | Set `EncodeOptions { restart_interval_rows, .. }` (in rows, not samples). Decoding validates `Ri % width == 0`. |
| Options | `encode(pixels, w, h)` only | `EncodeOptions { predictor (1..=7, default 1), point_transform, restart_interval_rows, precision }` via `encode_with_options`; `encode` keeps predictor 1, no point transform, no restarts | Use `encode_with_options` for predictors 2–7, point transform, or restarts. |
| Point transform | wrong (`diff << pt` branch) | reduced-domain `P' = P − Pt`; property `decoded == (orig >> pt) << pt` | If you set a non-zero point transform, expect the low `pt` bits to be discarded (that is the definition of point transform). |
| UID truthfulness | n/a | the `.70` transfer syntax means predictor 1 specifically; registry encodes pin defaults so the emitted UID stays truthful | Non-default predictors/options are for direct codec-crate users, not the transfer-syntax-tagged registry path. |

## Workspace-wide

| Area | 1.x | 2.0 | How to migrate |
| --- | --- | --- | --- |
| Versions | all crates `1.0.0` | all crates `2.0.0` (`dicos`, `pure_jpegrle`, `pure_jpegls`, `pure_jpegli`, `pure_jpeg2k`) | Bump your dependency pins to `2.0`. |
| Fixture generator | shipped as a binary (`dicos-gen-luggage-fishtank`, run with `--bin`) | moved to an **example**: `crates/dicos/examples/gen-luggage-fishtank.rs` | Run with `cargo run -p dicos --example gen-luggage-fishtank -- --size 64,64,48 --out <path>` instead of `--bin dicos-gen-luggage-fishtank`. |
| Test data | `testdata/` was checked in (~37 MB) | `testdata/` is **gitignored and generated on demand**; tests that read it skip gracefully when absent | Generate fixtures locally with the example above (see README "Test data"). CI generates a small fixture before the test job. |
