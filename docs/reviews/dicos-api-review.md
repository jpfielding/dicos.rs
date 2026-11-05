# DICOS API and Roxel integration review

Source locations refer to baseline commit `606331df3b6ccd1ada8a17bc8962bb9d4b2bc11d`; viewer line numbers may move in the UI overhaul.

Date: 2026-11-05. Scope: public DICOS parsing/writing, pixel extraction, frame boundaries, metadata, and Roxel ingestion. These findings describe the reviewed baseline. The UI overhaul fixes finding 6 for unsigned, unit-slope data; the other API/ingestion findings remain open.

## Validation and limits

- Built the current workspace library successfully with `cargo build -p dicos --all-features --lib`.
- Compiled and ran the standalone Rust harness below against `target/debug/libdicos.rlib`. It exercises the real public writer and parser. No repository source files were changed by this review.
- The compressed-frame probe uses short sentinel payloads, not valid compressed codestreams: it proves container frame-boundary corruption, not a codec decoding failure.
- Confirmed the parsed metadata accessor behavior underlying directory ordering. Did not execute the directory loader or render test datasets in the GUI.
- Viewer intensity, missing-slice, and threat propagation findings below are source-derived. No GPU/rendering or runtime validation of those findings was performed.
- This was a focused review, not a complete DICOM/DICOS conformance audit. No full test suite or third-party interoperability suite was run by this reviewer.

## Findings

### 1. P1: Rewriting multi-fragment compressed data corrupts frame boundaries

Evidence: `crates/dicos/src/reader/mod.rs:594-596` assembles fragments into frames but preserves the original BOT; `:947-956` concatenates all fragments in each BOT interval. `crates/dicos/src/writer.rs:299-320` copies those offsets while emitting one item per assembled frame.

The original offsets count fragment item headers. Merging fragments removes intermediate headers, so subsequent frame offsets change. The public parse/write path retains the obsolete offsets.

Confirmed reproduction: three fragments `[[1,2],[3,4],[5,6]]` with BOT `[0,20]` first parse into frames `[[1,2,3,4],[5,6]]`. Writing and parsing that result produces `[[1,2,3,4,5,6],[]]`. Data crosses a frame boundary and the second frame becomes empty.

Change: recompute BOT offsets from the actual emitted items, including any padding, or preserve the original fragment structure. Add a parse/write/parse regression containing a frame split across multiple fragments. Confidence: high, runtime confirmed at the public API boundary.

### 2. P1: Writer preserves an implicit-VR transfer syntax while emitting explicit VR

Evidence: `crates/dicos/src/writer.rs:44-64` preserves an existing TransferSyntaxUID; `:86-105` writes every body element using explicit VR regardless of that UID. The reader obeys the advertised syntax.

Confirmed reproduction: a dataset tagged Implicit VR Little Endian with one Rows element writes successfully, then reparsing returns `Truncated { offset: 178, context: "element value" }`. This also affects editing and saving a parsed implicit-VR dataset.

Change: implement body encoding matching the advertised syntax, normalize native output to Explicit VR Little Endian, or reject unsupported output syntax before writing. Compressed transfer syntax UIDs must continue to describe the actual compressed pixels. Confidence: high, runtime confirmed.

### 3. P1: Valid 8-bit data corrupts on parse/write roundtrip

Evidence: `crates/dicos/src/reader/mod.rs:1003-1005` widens 8-bit samples into `u16`; `crates/dicos/src/writer.rs:223` and `:269-286` always emit native samples as 16-bit bytes. The dataset still advertises BitsAllocated=8.

Confirmed reproduction: a 1x2 image with BitsAllocated=8 and pixel bytes `[10,20]` parses correctly. Writing and parsing that dataset yields `[10,0,20,0]`, doubling the sample count.

Change: pass dataset pixel-format metadata into native serialization and encode the supported sample width correctly, validating sample ranges. Add separate 8-bit and 16-bit roundtrip regressions. Confidence: high, runtime confirmed.

### 4. P1: Directory spatial ordering does not consume ImagePositionPatient from parsed files

Evidence: `crates/roxel/src/volume/dataset.rs:185-191` calls `get_string(IMAGE_POSITION_PATIENT)` and expects a backslash-separated triple. `crates/dicos/src/reader/mod.rs:739-741` splits multi-valued DS into `Value::Strings`; `crates/dicos/src/types.rs:49-53` returns only the first value from that representation.

Confirmed accessor behavior: serialized `0\0\-10` parses with `get_string == Some("0")`, while `get_strs == Some(["0","0","-10"])`. Thus the Z-coordinate branch does not run for normal files parsed from disk. If SliceLocation is absent, the loader falls back to InstanceNumber or filename order. Those orders need not match spatial order, so the assembled volume can be reversed or scrambled. Existing sort tests construct unsplit in-memory strings and bypass this integration boundary.

Change: consume `get_strs` or a shared numeric-vector accessor, and test sort behavior after serialization/parsing. A general spatial implementation should also account for orientation rather than assuming Z is always the slice axis. Confidence: high for the accessor mismatch; directory-loader consequence is a source trace, not an executed end-to-end test.

### 5. P2: Even-length padding becomes an extra voxel in odd-sized 8-bit images

Evidence: `crates/dicos/src/reader/mod.rs:1003-1005` maps every native byte to a pixel, without excluding the trailing padding byte justified by the image dimensions. `crates/roxel/src/volume/dataset.rs:442-450` rejects an unexpected sample count.

Confirmed reproduction: a 1x1, 8-bit dataset with padded bytes `[10,0]` parses as two pixels `[10,0]`. Roxel's exact count check consequently rejects it; that final loader rejection was source-traced rather than executed.

Change: distinguish sample bytes from element padding using the expected pixel count and supported sample format. Remove only the metadata-justified trailing pad byte, and continue rejecting genuinely excess data. Confidence: high for parser behavior.

### 6. P1: 2D and 3D apply different intensity transforms — fixed for supported unsigned, unit-slope data

Source evidence: `crates/roxel/src/slice_view.rs:141-142` applies window/level directly to raw samples. `crates/roxel/src/raycast.wgsl:110` adds `rescale_intercept` before windowing. `crates/roxel/src/volume/dataset.rs:398-401` reads that intercept from the dataset.

For a nonzero intercept and the same window settings, these paths calculate different normalized intensities for the same stored voxel. A hand example with raw=1000, intercept=-1000, WC=0 and WW=400 gives a saturated value in the raw 2D path but a mid-window normalized intensity in the 3D path, before transfer-function and lighting effects.

Change: define one stored-sample-to-display-value transform and use it consistently in slices, projection/composite paths, window calculations, and 3D rendering. Validate with a synthetic nonzero-intercept dataset. Confidence: high from source; no GUI/GPU execution of this example.

Update: slices and composites now add the same intercept as the 3D shader, and automatic percentile windows use that rescaled domain. Regression tests cover a nonzero intercept in slice/projection and automatic windowing. This does not resolve signed samples or rescale slope (finding 7).

### 7. P1: Signed pixel representation and rescale slope are not handled or rejected

Source evidence: `crates/roxel/src/volume/dataset.rs:398-440` reads intercept but copies decoded/native values as `u16`, without reading PixelRepresentation or RescaleSlope. `crates/roxel/src/volume/mod.rs:17-27` retains raw unsigned samples and intercept, with no slope field. The shader intensity expression at `crates/roxel/src/raycast.wgsl:110` likewise has no slope.

A signed stored value of -1 can therefore be treated as the large unsigned value 65535, and a slope other than 1 is ignored. Dataset acceptance does not communicate that these pixel semantics are unsupported. This can materially change apparent density and windowing.

Change: decode supported signed/sample-bit semantics and apply slope/intercept consistently, or reject unsupported combinations with an explicit error until implemented. Confidence: high from source; no signed/slope runtime fixture was executed.

### 8. P1: Directory loading can return an incomplete volume and discard later threats

Source evidence: `crates/roxel/src/volume/dataset.rs:245-258` logs and skips file/read/parse failures; `:288-304` logs and skips dimension mismatches and later extraction failures; `:317-327` returns a volume whose threat list is only `first.threats`.

A directory with otherwise loadable slices can produce an `Ok(Volume)` after slices have been omitted. The return type carries no structured completeness information. Further, threats from successfully appended later datasets are not accumulated or shifted to the combined volume's slice coordinates. This is a source-level loader contract issue; whether logs are separately visible elsewhere in the UI was not verified.

Change: return structured load diagnostics and require callers to expose partial-volume status, or fail the load when completeness is required. Preserve compatible threats from all included datasets and translate their slice coordinates into the assembled volume. Validate series compatibility and metadata consistency when stacking files. Confidence: high for the inspected return/data propagation; no multi-file runtime fixture was executed.

## Reproduce the confirmed public API findings

From the repository root, build the current library:

```sh
cargo build -p dicos --all-features --lib
```

Save the following standalone probe to `/tmp/dicos_api_review.rs`. It only creates datasets in memory and writes to in-memory buffers:

```rust
use dicos::{Dataset,Element,PixelData,Value,Vr,tag,transfer};
fn bytes(ds:&Dataset)->Vec<u8>{let mut b=vec![]; dicos::write(&mut b,ds).unwrap();b}
fn main(){
 let mut ds=Dataset::new(); ds.put_string(tag::TRANSFER_SYNTAX_UID,Vr::UI,transfer::IMPLICIT_VR_LITTLE_ENDIAN); ds.put_u16(tag::ROWS,Vr::US,1);
 println!("implicit roundtrip: {:?}",dicos::parse(bytes(&ds).as_slice()));
 let mut ds=Dataset::new();ds.put_u16(tag::ROWS,Vr::US,1);ds.put_u16(tag::COLUMNS,Vr::US,2);ds.put_u16(tag::BITS_ALLOCATED,Vr::US,8);ds.insert(Element::new(tag::PIXEL_DATA,Vr::OB,Value::Bytes(vec![10,20])));
 let first=dicos::parse(bytes(&ds).as_slice()).unwrap();let second=dicos::parse(bytes(&first).as_slice()).unwrap();println!("8bit before {:?}; after {:?}",first.pixel_data(),second.pixel_data());
 let mut ds=Dataset::new();ds.put_string(tag::TRANSFER_SYNTAX_UID,Vr::UI,transfer::JPEG_LS_LOSSLESS);ds.insert(Element::new(tag::PIXEL_DATA,Vr::OB,Value::PixelData(PixelData::encapsulated(vec![vec![1,2],vec![3,4],vec![5,6]],vec![0,20]))));
 let first=dicos::parse(bytes(&ds).as_slice()).unwrap();let second=dicos::parse(bytes(&first).as_slice()).unwrap();println!("BOT before {:?}; after {:?}",first.pixel_data(),second.pixel_data());
 let mut ds=Dataset::new();ds.put_string(tag::IMAGE_POSITION_PATIENT,Vr::DS,"0\\0\\-10");let ds=dicos::parse(bytes(&ds).as_slice()).unwrap();println!("IPP string {:?}, strings {:?}",ds.get_string(tag::IMAGE_POSITION_PATIENT),ds.get_strs(tag::IMAGE_POSITION_PATIENT));
 let mut ds=Dataset::new();ds.put_u16(tag::ROWS,Vr::US,1);ds.put_u16(tag::COLUMNS,Vr::US,1);ds.put_u16(tag::BITS_ALLOCATED,Vr::US,8);ds.insert(Element::new(tag::PIXEL_DATA,Vr::OB,Value::Bytes(vec![10,0]))); let ds=dicos::parse(bytes(&ds).as_slice()).unwrap();println!("padded 8bit {:?}",ds.pixel_data());
}
```

Compile and run it:

```sh
rustc --edition=2021 /tmp/dicos_api_review.rs \
  --extern dicos=target/debug/libdicos.rlib \
  -L dependency=target/debug/deps -o /tmp/dicos_api_review
/tmp/dicos_api_review
```

Observed output:

```text
implicit roundtrip: Err(Truncated { offset: 178, context: "element value" })
8bit before Some(Native { frames: [[10, 20]] }); after Some(Native { frames: [[10, 0, 20, 0]] })
BOT before Some(Encapsulated { frames: [[1, 2, 3, 4], [5, 6]], offsets: [0, 20] }); after Some(Encapsulated { frames: [[1, 2, 3, 4, 5, 6], []], offsets: [0, 20] })
IPP string Some("0"), strings Some(["0", "0", "-10"])
padded 8bit Some(Native { frames: [[10, 0]] })
```

The probe output is a snapshot of the reviewed implementation, not an expected behavior specification. Correct fixes should change these results and replace the probes with regression assertions appropriate to the supported formats.
