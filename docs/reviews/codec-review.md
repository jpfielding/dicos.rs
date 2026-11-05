# Independent codec review

Source locations refer to baseline commit `606331df3b6ccd1ada8a17bc8962bb9d4b2bc11d`; viewer line numbers may move in the UI overhaul.

Date: 2026-11-05. Scope: `jpegli`, `jpeg2k`, `jpegls`, and `jpegrle`; supported-format claims, interoperability, and malformed input handling. No codec source changes were made. Findings below were reproduced against compiled debug libraries, rather than inferred solely from source.

## Findings

### P1 — RLE encoder crosses image row boundaries

Evidence: `crates/jpegrle/src/encode.rs:37–47` splits the image into complete byte planes, then calls `encode_packbits` once per plane. The PackBits encoder has no row-width argument.

Reproduction: encode `[0x1234u16; 4]` with dimensions 2×2. Bytes after the 64-byte header are `[fd, 12, fd, 34]`: each segment contains a replicate packet spanning four samples, crossing the two-sample row boundary.

[DICOM PS3.5 G.3.1](https://dicom.nema.org/Medical/dicom/current/output/chtml/part05/sect_G.3.html) requires each row to be encoded separately. Thus normal encoder output violates its advertised DICOM conformance and can fail with row-oriented consumers. The crate's own full-plane decoder masks the discrepancy in round-trip tests. External consumer rejection was not tested.

Change: compress each row independently, concatenate its packets into the segment, then pad the complete segment to even length. Add a packet-boundary regression and independent RLE interoperability coverage.

Confidence: high; emitted bytes verified at runtime and requirement checked against the official standard.

### P2 — Malformed JPEG2000 QCD panics in overflow-checked builds

Evidence: `crates/jpeg2k/src/codestream.rs:56–58` computes `epsilon as u32 + guard_bits as u32 - 1`. The robustness check at lines 813–819 calls this function before validating its result.

Reproduction: encode an 8×8 image with default options. Locate QCD (`ff 5c`), set its style/guard byte at marker offset +4 to zero, and its first exponent byte at +5 to zero. Leave other exponents nonzero to bypass the all-zero legacy discriminator. Calling `jpeg2k::decode` panics at line 58 with `attempt to subtract with overflow`; `catch_unwind` confirms this is a panic rather than a returned codec error.

Impact: attacker-controlled input violates the fallible decode contract and can unwind the caller in debug or other overflow-checked builds. Default release arithmetic would wrap and subsequently fail the upper-bound check; a release panic was not claimed or tested.

Change: validate the lower bound before subtraction, or use checked arithmetic and return `InvalidData`. Retain the upper-bound guard and cover both bounds with malformed QCD tests.

Confidence: high; runtime verified.

### P2 — RLE decoder treats header bytes as image payload

Evidence: `crates/jpegrle/src/decode.rs:58–74` validates offsets against the input length and each other, but does not require segment starts to lie beyond the 64-byte header.

Reproduction: create a 64-byte zero buffer, set byte 0 to 1 (one segment) and byte 4 to 8 (first offset 8), then decode as 1×1. Result: `Ok(([0], 1, 1))`, although the input has no payload beyond its header.

Impact: malformed headers can fabricate a successful decoded image from metadata bytes instead of being rejected. This is not an out-of-bounds memory access.

Change: validate the complete RLE header and supported segment count before decompression, including offsets into the header and segment ordering/bounds.

Confidence: high; runtime verified.

### P2 — JPEG Lossless accepts missing image termination

Evidence: `crates/jpegli/src/scan.rs:519–532` checks a pending restart marker and then returns pixels. The enclosing `decode` function does not require EOI after decoding the scan.

Reproduction: encode `[100u16, 200, 300, 400]` as 2×2, remove the last two bytes (`ff d9`), then decode. Result: `Ok(([100, 200, 300, 400], 2, 2))`.

Impact: a truncated file is reported as a successfully completed image. This reproduction retains all entropy-coded samples; pixel corruption was not observed or claimed.

Change: validate completion of the entropy segment and the required EOI marker, accounting for marker bytes already buffered by the bit reader as well as unread bytes.

Confidence: high; runtime verified.

## Supported scope and completeness

The README files explicitly narrow the supported formats:

- JPEG2000: unsigned 16-bit grayscale, one tile and layer, reversible 5/3, LRCP, restricted coding styles; signed data, other precisions, multiple components/tiles and numerous optional markers are rejected.
- JPEG Lossless: single-component images, precision 2–16, predictors 1–7, point transforms and row-aligned restart intervals. Registry defaults must retain predictor 1 for the `.70` transfer syntax.
- JPEG-LS: grayscale and three-component planar scans; line/sample interleaving, restart markers, mapping tables and subsampling are rejected.
- RLE: grayscale, one or two byte segments on decode; the public encoder accepts `u16` and emits two segments.

These documented restrictions are meaningful product limitations, rather than evidence that those unsupported features were accidentally omitted. The RLE row-boundary violation is distinct: it affects the supported encode path.

## Validation performed

- `cargo test -p pure_jpegli -p pure_jpeg2k -p pure_jpegls -p pure_jpegrle --quiet`: **484 tests passed**, including fixture-based interoperability, mutation tests and doctests. Environment-gated external tests self-skip in this invocation.
- `DICOS_INTEROP=1 cargo test -p pure_jpeg2k -p pure_jpegli --test interop -- --nocapture`: **all four tests passed**, including live OpenJPEG and libjpeg-turbo/djpeg decode of crate-produced streams.
- `DICOS_CHARLS_INTEROP=1 cargo test -p pure_jpegls --test charls_encode_interop -- --nocapture`: **could not validate live CharLS interoperability**. Helper compilation failed because `charls/charls_jpegls_decoder.h` was unavailable. This is an environment limitation, not demonstrated codec incompatibility. Checked-in JPEG-LS interoperability fixtures passed in the main suite.
- A temporary Rust probe linked the compiled codec libraries and reproduced all four findings. Temporary files were outside the repository; no codec source fixes were made.
- No new fuzz campaign, release-mode malformed-input campaign, independent RLE consumer test, or viewer visual assessment was performed as part of this codec review.

Verdict: tested JPEG Lossless/JPEG2000 interoperability is encouraging, but RLE encoding is not conformant for general multi-row images, and malformed-input validation still has concrete gaps.
