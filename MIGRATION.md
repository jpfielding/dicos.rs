# Migration Guide: dicos.rs 1.x → 2.0

This document tracks the breaking API changes across the 2.0 release. It is
built up per-PR; PR A6 completes it with the release-wide summary.

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
| Root re-exports | `Codec`, `CodecError`, `GrayImage` | adds `Dataset`, `Element`, `Value`, `PixelData`, `Tag`, `Vr`, `parse`, `parse_with_limit`, `parse_with_warnings`, `parse_with_warnings_and_limit`, `ParseWarning`, `DicosError`, `write` | Prefer `dicos::Dataset` etc. over the fully-qualified module paths. |

### PR A4 — Codec registry

| Area | 1.x | 2.0 | How to migrate |
| --- | --- | --- | --- |
| `decode_frame(data, w, h, ts_uid)` | fell back to sniffing when the transfer syntax was unknown | unknown/unsupported transfer syntax **always** returns `DicosError::UnsupportedTransferSyntax`; no sniff fallback | If you relied on content detection, call the new `decode_frame_sniffed(data, w, h)` explicitly. |
| `sniff_codec(data)` | guessed RLE from a leading segment-count; matched the JP2 box magic; scanned up to 4 KB for JPEG SOF markers | RLE guess removed (RLE has no signature — select via transfer syntax); JP2-box magic removed (decoder is codestream-only, keeps only `FF 4F` SOC); JPEG detection is now a bounded, structured marker walk to the first SOF (`FF F7` → JPEG-LS, `FF C3` → JPEG Lossless) | For RLE, pass the transfer syntax UID to `decode_frame`/`codec_for_transfer_syntax`. JP2-boxed streams are no longer sniffed. |
| Codec error mapping | backend errors flattened to `CodecError::InvalidData(e.to_string())` | backend errors map to `CodecError::Backend { codec, source }`, preserving the original via `std::error::Error::source` | Match on `CodecError::Backend { codec, source }` (or `#[non_exhaustive]` fallthrough) instead of parsing strings. |
