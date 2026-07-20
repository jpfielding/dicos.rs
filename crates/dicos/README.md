# dicos

A Rust library and CLI toolkit for working with **DICOS** (Digital Imaging and Communications in Security) files. Provides reading, writing, codec support, and inspection of DICOS/DICOM files used in baggage scanning, cargo inspection, and personnel screening equipment.

Implements the **NEMA IIC 1 v04-2023** standard.

## What is DICOS?

DICOS (Digital Imaging and Communications in Security) is a file format and communication standard for security screening equipment. It is based on the medical imaging standard DICOM (Digital Imaging and Communications in Medicine) but extended with security-specific modules for threat detection, automated decision-making, and object-of-interest classification.

DICOS is maintained by NEMA (National Electrical Manufacturers Association) and is used by TSA, customs agencies, and security screening vendors worldwide. Equipment such as X-ray baggage scanners, CT scanners, and personnel screening portals produce DICOS files.

Key differences from DICOM:

- **Group 4010** tags for Automated Threat Detection (ATD), threat ROIs, alarm decisions, and object-of-interest classification
- **Group 6100** tags for energy-discriminating detector data
- Security-specific SOP Classes (CT, DX, TDR, AIT)
- Screening workflow modules (itinerary, OOI owner, transport classification)

## File Structure

A DICOS file follows the DICOM Part 10 binary layout:

```
+--------------------------------------------+
|  128-byte preamble (unused, all zeros)      |
+--------------------------------------------+
|  4 bytes: "DICM" magic number              |
+--------------------------------------------+
|  File Meta Information (Group 0002)         |
|    Always Explicit VR Little Endian         |
|    Contains: Transfer Syntax UID,           |
|              SOP Class UID, etc.            |
+--------------------------------------------+
|  Dataset Elements (Groups 0008-7FDF)        |
|    Encoded per the Transfer Syntax          |
|    Patient, Study, Series, Equipment,       |
|    Image Pixel, CT/DX params, ATD data...   |
+--------------------------------------------+
|  Pixel Data (7FE0,0010)                     |
|    Native: raw u16 pixel values             |
|    Encapsulated: compressed frame fragments  |
+--------------------------------------------+
```

Each data element is encoded as:

```
+---------+---------+--------+---------+
|  Group  | Element |   VR   |  Value  |
| (2 bytes)| (2 bytes)| (2 bytes)| (variable)|
+---------+---------+--------+---------+
```

## Supported Modalities

DICOS defines four primary imaging modalities, each with its own SOP Class:

| Modality | Description | SOP Class UID |
|----------|-------------|---------------|
| **CT** | Computed Tomography | `1.2.840.10008.5.1.4.1.1.501.1` |
| **DX** | Digital X-Ray (Projection) | `1.2.840.10008.5.1.4.1.1.501.2.1` |
| **TDR** | Threat Detection Report | `1.2.840.10008.5.1.4.1.1.501.3` |
| **AIT** | Advanced Imaging Technology (Personnel) | `1.2.840.10008.5.1.4.1.1.501.4` |

**CT** -- 3D volume data from baggage/cargo CT scanners. Multi-frame images with slice position and orientation metadata. Typical bit depth is 16-bit unsigned.

```rust
use dicos::{reader, tag};

let ds = reader::parse(std::fs::File::open("ct_volume.dcs")?)?;
assert_eq!(ds.modality(), "CT");
println!("Slices: {}", ds.number_of_frames());
println!("Dimensions: {}x{}", ds.columns(), ds.rows());
```

**DX** -- 2D projection images from X-ray line scanners. May include multi-energy views (low/high energy) for material discrimination.

```rust
let ds = reader::parse(std::fs::File::open("xray_scan.dcs")?)?;
assert_eq!(ds.modality(), "DX");
println!("Image: {}x{} @ {} bits",
    ds.columns(), ds.rows(), ds.bits_allocated());
```

**TDR** -- Threat Detection Reports containing automated threat detection results. No pixel data; instead contains sequences of threat ROIs, alarm decisions, and confidence scores.

```rust
use dicos::{reader, tag, types::Value};

let ds = reader::parse(std::fs::File::open("threat_report.dcs")?)?;
if let Some(decision) = ds.get_string(tag::ALARM_DECISION) {
    println!("Alarm: {decision}");
}
if let Some(elem) = ds.get(tag::THREAT_ROI_SEQUENCE) {
    if let Value::Sequence(items) = &elem.value {
        println!("Threats detected: {}", items.len());
    }
}
```

**AIT** -- Personnel screening images from full-body scanners (e.g., millimeter wave). Typically single-frame grayscale images.

## Transfer Syntaxes

The library supports all standard DICOM transfer syntaxes used in DICOS:

| Transfer Syntax | UID | Compressed | Codec Crate |
|-----------------|-----|------------|-------------|
| Implicit VR Little Endian | `1.2.840.10008.1.2` | No | -- |
| Explicit VR Little Endian | `1.2.840.10008.1.2.1` | No | -- |
| Explicit VR Little Endian Extended | `1.2.840.10008.1.2.1.64` | No | -- |
| Explicit VR Big Endian (Retired) | `1.2.840.10008.1.2.2` | No | -- |
| RLE Lossless | `1.2.840.10008.1.2.5` | Yes | `jpegrle` |
| JPEG Lossless (Process 14) | `1.2.840.10008.1.2.4.57` | Yes | `jpegli` |
| JPEG Lossless First-Order (Process 14, SV1) | `1.2.840.10008.1.2.4.70` | Yes | `jpegli` |
| JPEG-LS Lossless | `1.2.840.10008.1.2.4.80` | Yes | `jpegls` |
| JPEG-LS Near-Lossless | `1.2.840.10008.1.2.4.81` | Yes | `jpegls` |
| JPEG 2000 Lossless | `1.2.840.10008.1.2.4.90` | Yes | `jpeg2k` |
| JPEG 2000 | `1.2.840.10008.1.2.4.91` | Yes | `jpeg2k` |
| JPEG Baseline (Process 1) | `1.2.840.10008.1.2.4.50` | Yes | -- |
| JPEG Extended (Process 2 & 4) | `1.2.840.10008.1.2.4.51` | Yes | -- |
| Deflated Explicit VR Little Endian | `1.2.840.10008.1.2.1.99` | Yes | -- |

```rust
use dicos::transfer::{self, TransferSyntax};

let ts = TransferSyntax::new(transfer::JPEG_LS_LOSSLESS);
assert!(ts.is_encapsulated());
assert!(ts.is_jpeg_ls());
assert!(ts.is_explicit_vr());
println!("{}", ts.name()); // "JPEG-LS Lossless"
```

## Library Usage

Add `dicos` to your `Cargo.toml`:

```toml
[dependencies]
dicos = "1.0"

# Enable compression codecs as needed:
# dicos = { version = "2.0", features = ["all-codecs"] }
```

### Reading a DICOS File

```rust
use std::fs::File;
use std::io::BufReader;
use dicos::reader;

let file = File::open("scan.dcs")?;
let ds = reader::parse(BufReader::new(file))?;

println!("Modality: {}", ds.modality());
println!("Dimensions: {}x{}", ds.columns(), ds.rows());
println!("Frames: {}", ds.number_of_frames());
println!("Bits Allocated: {}", ds.bits_allocated());
println!("Transfer Syntax: {}", ds.transfer_syntax());
```

### Accessing Data Elements

```rust
use dicos::{reader, tag, types::Value, vr::Vr};

let ds = reader::parse(std::fs::File::open("scan.dcs")?)?;

// String elements
if let Some(name) = ds.get_string(tag::PATIENT_NAME) {
    println!("Patient: {name}");
}
if let Some(manufacturer) = ds.get_string(tag::MANUFACTURER) {
    println!("Manufacturer: {manufacturer}");
}

// Numeric elements
if let Some(rows) = ds.get_u16(tag::ROWS) {
    println!("Rows: {rows}");
}

// Iterate all elements in tag order
for (tag, elem) in ds.iter() {
    println!("{tag} {}: {:?}", elem.vr, elem.value);
}
```

### Working with Pixel Data

```rust
use dicos::{reader, tag, types::Value};

let ds = reader::parse(std::fs::File::open("scan.dcs")?)?;

if let Some(pd) = ds.pixel_data() {
    println!("Frames: {}", pd.num_frames());
    println!("Compressed: {}", pd.is_compressed());

    if !pd.is_compressed() {
        // Native pixel data -- access raw u16 values
        let pixels = pd.flat_data().unwrap();
        println!("Total pixels: {}", pixels.len());

        // Access individual frames
        let frame0 = pd.frame(0).unwrap();
        println!("Frame 0 pixels: {}", frame0.data.len());
    }
}
```

### Building and Writing Datasets

```rust
use dicos::types::{Dataset, Element, Value};
use dicos::{tag, vr::Vr, writer};

let mut ds = Dataset::new();
ds.put_string(tag::PATIENT_NAME, Vr::PN, "DOE^JOHN");
ds.put_string(tag::MODALITY, Vr::CS, "CT");
ds.put_u16(tag::ROWS, Vr::US, 512);
ds.put_u16(tag::COLUMNS, Vr::US, 512);
ds.put_u16(tag::BITS_ALLOCATED, Vr::US, 16);

let mut output = std::fs::File::create("output.dcs")?;
let bytes_written = writer::write(&mut output, &ds)?;
println!("Wrote {bytes_written} bytes");
```

### Using Codecs for Compressed Pixel Data

```rust
use dicos::codec_registry;

// Look up a codec by name
if let Some(codec) = codec_registry::codec_by_name("jpeg-ls") {
    println!("Codec: {}", codec.name());
    println!("TS UID: {}", codec.transfer_syntax_uid());
}

// Look up a codec by transfer syntax UID
use dicos::transfer;
if let Some(codec) = codec_registry::codec_for_transfer_syntax(transfer::RLE_LOSSLESS) {
    println!("Found codec: {}", codec.name());
}

// Decode a compressed frame (auto-selects codec from transfer syntax)
let decoded_pixels = codec_registry::decode_frame(
    &compressed_data,
    width,
    height,
    transfer::JPEG_2000_LOSSLESS,
)?;

// Encode using the GrayImage type
use dicos::GrayImage;
let img = GrayImage::from_data(512, 512, pixel_vec).unwrap();
let mut buf = Vec::new();
codec.encode(&img, &mut buf)?;
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `rle` | Enable RLE PackBits codec via `pure_jpegrle` package, imported as `jpegrle` |
| `jpegls` | Enable JPEG-LS codec via `pure_jpegls` package, imported as `jpegls` |
| `jpegli` | Enable JPEG Lossless codec via `pure_jpegli` package, imported as `jpegli` |
| `jpeg2k` | Enable JPEG 2000 codec via `pure_jpeg2k` package, imported as `jpeg2k` |
| `all-codecs` | Enable all of the above |
| `cli` | Build the `dicosctl` binary (includes all codecs + clap + serde_json) |

## CLI Usage (dicosctl)

Build the CLI with the `cli` feature:

```sh
cargo build -p dicos --features cli --release
```

### dump -- Serialize metadata as JSON

```sh
$ dicosctl dump scan.dcs
{
  "FileMetaInformationGroupLength": {
    "vr": "UL",
    "Value": 186
  },
  "TransferSyntaxUID": {
    "vr": "UI",
    "Value": "1.2.840.10008.1.2.4.80"
  },
  "Modality": {
    "vr": "CS",
    "Value": "CT"
  },
  "Rows": {
    "vr": "US",
    "Value": 512
  },
  "Columns": {
    "vr": "US",
    "Value": 512
  },
  "BitsAllocated": {
    "vr": "US",
    "Value": 16
  },
  "PixelData": {
    "vr": "OW",
    "Encapsulated": true,
    "Frames": 300
  }
}
```

Pipe through `jq` for selective queries:

```sh
# Extract the modality
dicosctl dump scan.dcs | jq '.Modality.Value'

# List all tag names
dicosctl dump scan.dcs | jq 'keys'
```

### info -- Human-readable summary

```sh
$ dicosctl info scan.dcs
File: scan.dcs
Elements: 47
Modality: CT
Dimensions: 512x512
Frames: 300
Bits Allocated: 16
Transfer Syntax: JPEG-LS Lossless (1.2.840.10008.1.2.4.80)
SOP Class: 1.2.840.10008.5.1.4.1.1.501.1
SOP Instance: 1.2.276.0.7230010.3.1.4.1234567890.1234.1234567890
Manufacturer: Smiths Detection
Model: HI-SCAN 10080 XCT
```

### Generate Synthetic Luggage + Fish Tank Example

Generate a public-safe DICOS CT sample (no vendor/private branding values):

```sh
cargo run -p dicos --bin dicos-gen-luggage-fishtank -- \
  --out testdata/synthetic/luggage_fishtank_ct.dcs
```

Optional flags:

```sh
--size 256,256,160
--seed 20260225
--with-noise on
--include-threat-tags on
```

## Modules

| Module | Description |
|--------|-------------|
| `tag` | Tag constants for standard DICOM/DICOS data elements (groups 0002-7FE0, 4010, 6100) |
| `vr` | Value Representation type definitions (all 31 VRs) with encoding property queries |
| `transfer` | Transfer Syntax UID constants and `TransferSyntax` type with property queries |
| `types` | Core types: `Dataset`, `Element`, `Value`, `PixelData`, `Frame` |
| `reader` | DICOS Part 10 file parser (preamble, DICM magic, meta info, dataset) |
| `writer` | DICOS Part 10 file writer (Explicit VR Little Endian) |
| `codec` | `Codec` trait for lossless 16-bit grayscale image compression |
| `codec_registry` | Codec lookup by name, transfer syntax UID, or magic-byte sniffing |
| `img` | `GrayImage<T>` row-major pixel buffer type |
| `error` | `CodecError` and `DicosError` error types |

## Crate Structure

```
dicos.rs/
  crates/
    dicos/              Core library + dicosctl CLI
      src/
        lib.rs          Crate root, re-exports Codec, CodecError, GrayImage
        tag.rs          ~120 tag constants (groups 0002, 0008, 0010, 0018,
                         0020, 0028, 4010, 6100, 7FE0, FFFE)
        vr.rs           Vr enum (31 variants), encoding queries
        transfer.rs     TransferSyntax type, 14 UID constants
        types.rs        Dataset, Element, Value, PixelData, Frame
        reader.rs       Part 10 parser (implicit/explicit VR, encapsulated)
        writer.rs       Part 10 writer (explicit VR little endian)
        codec.rs        Codec trait (encode, decode, name, transfer_syntax_uid)
        codec_registry.rs  Static codec instances, lookup functions
        img.rs          GrayImage<T> pixel buffer
        error.rs        CodecError, DicosError
        bin/
          dicosctl.rs   CLI tool (dump, info subcommands)
    jpegrle/            RLE PackBits codec (DICOM Part 5 Annex G)
    jpegls/             JPEG-LS codec (ISO/IEC 14495-1, LOCO-I)
    jpegli/             JPEG Lossless codec (ITU-T T.81 Annex H, DPCM)
    jpeg2k/             JPEG 2000 codec (ITU-T T.800, wavelet)
    roxel/              3D volume viewer (wgpu + egui)
```

## DICOS-Specific Tags

All DICOS-specific tags are defined in the `tag` module. The primary DICOS groups are:

### Group 4010 -- ATD / Threat Detection

| Constant | Tag | Name |
|----------|-----|------|
| `ALARM_DECISION` | (4010,100A) | AlarmDecision |
| `OOI_TYPE` | (4010,1012) | OOIType |
| `NUMBER_OF_ALARM_OBJECTS` | (4010,1014) | NumberOfAlarmObjects |
| `ATD_ASSESSMENT_SEQUENCE` | (4010,1015) | ATDAssessmentSequence |
| `THREAT_CONFIDENCE_SCORE` | (4010,1016) | ThreatConfidenceScore |
| `ATD_ASSESSMENT_PROBABILITY` | (4010,1017) | ATDAssessmentProbability |
| `ATD_ABILITY` | (4010,1001) | ATDAbility |
| `POTENTIAL_THREAT_OBJECT_ID` | (4010,1006) | PotentialThreatObjectID |
| `THREAT_ROI_SEQUENCE` | (4010,1020) | ThreatROISequence |
| `THREAT_ROI_TYPE` | (4010,1009) | ThreatROIType |
| `BOUNDING_BOX_TOP_LEFT` | (4010,1023) | BoundingBoxTopLeft |
| `BOUNDING_BOX_BOTTOM_RIGHT` | (4010,1024) | BoundingBoxBottomRight |
| `BOUNDING_POLYGON` | (4010,101D) | BoundingPolygon |
| `THREAT_CATEGORY_DESCRIPTION` | (4010,1028) | ThreatCategoryDescription |
| `PTO_SEQUENCE` | (4010,1010) | PTOSequence |
| `PTO_REPRESENTATION_SEQUENCE` | (4010,1011) | PTORepresentationSequence |

### Group 4010 -- DX Energy Discrimination

| Constant | Tag | Name |
|----------|-----|------|
| `LOW_ENERGY_DETECTOR` | (4010,0001) | LowEnergyDetector |
| `HIGH_ENERGY_DETECTOR` | (4010,0002) | HighEnergyDetector |
| `DETECTOR_BIN_NUMBER` | (4010,0003) | DetectorBinNumber |
| `LOWER_ENERGY` | (4010,0005) | LowerEnergy |
| `ENERGY_RESOLUTION` | (4010,0006) | EnergyResolution |
| `HIGHER_ENERGY` | (4010,0007) | HigherEnergy |

### Group 4010 -- OOI / Itinerary

| Constant | Tag | Name |
|----------|-----|------|
| `OOI_OWNER_ID` | (4010,1030) | OOIOwnerID |
| `OOI_OWNER_NAME` | (4010,1031) | OOIOwnerName |
| `OOI_ID` | (4010,1034) | OOIID |
| `OOI_LABEL` | (4010,1037) | OOILabel |
| `FLIGHT_NUMBER` | (4010,1040) | FlightNumber |
| `DEPARTURE_AIRPORT` | (4010,1043) | DepartureAirport |
| `ARRIVAL_AIRPORT` | (4010,1044) | ArrivalAirport |
| `CARRIER_NAME` | (4010,1045) | CarrierName |

### Group 6100 -- Series Energy

| Constant | Tag | Name |
|----------|-----|------|
| `SERIES_ENERGY` | (6100,0030) | SeriesEnergy |
| `SERIES_ENERGY_DESCRIPTION` | (6100,0031) | SeriesEnergyDescription |

## Compression Support

All codec crates provide lossless 16-bit grayscale encode/decode through the `Codec` trait:

| Codec | Standard | Package | Imported as | Feature | Transfer Syntax UID |
|-------|----------|---------|-------------|---------|---------------------|
| RLE PackBits | DICOM Part 5 Annex G | `pure_jpegrle` | `jpegrle` | `rle` | `1.2.840.10008.1.2.5` |
| JPEG-LS | ISO/IEC 14495-1 (LOCO-I) | `pure_jpegls` | `jpegls` | `jpegls` | `1.2.840.10008.1.2.4.80` |
| JPEG Lossless | ITU-T T.81 Annex H (DPCM) | `pure_jpegli` | `jpegli` | `jpegli` | `1.2.840.10008.1.2.4.70` |
| JPEG 2000 | ITU-T T.800 (Wavelet) | `pure_jpeg2k` | `jpeg2k` | `jpeg2k` | `1.2.840.10008.1.2.4.90` |

The codec registry provides three ways to resolve a codec:

1. **By name** -- `codec_registry::codec_by_name("jpeg-ls")`
2. **By transfer syntax UID** -- `codec_registry::codec_for_transfer_syntax(uid)`
3. **By magic bytes** -- `codec_registry::sniff_codec(data)` (heuristic, checks JPEG/RLE signatures)

## Value Representations

All 31 standard DICOM Value Representations are supported:

| Category | VRs |
|----------|-----|
| **String** | AE, AS, CS, DA, DS, DT, IS, LO, LT, PN, SH, ST, TM, UC, UI, UR, UT |
| **Binary** | AT, FL, FD, OB, OD, OF, OL, OW, SL, SS, UL, UN, US |
| **Sequence** | SQ |

Long-form VRs (4-byte length field): OB, OD, OF, OL, OW, SQ, UC, UN, UR, UT.

## References

- [NEMA DICOS Standard](https://www.nema.org/standards/view/Digital-Imaging-and-Communications-in-Security) -- NEMA IIC 1 v04-2023
- [DICOM Standard](https://www.dicomstandard.org/) -- PS3.5 Data Structures and Encoding, PS3.10 Media Storage
- [DICOS/DICOM Format Overview (Stratovan)](https://www.stratovan.com/products/dicos)
- [JPEG-LS (Wikipedia)](https://en.wikipedia.org/wiki/Lossless_JPEG#JPEG-LS) -- ISO/IEC 14495-1 LOCO-I algorithm
- [JPEG 2000 (Wikipedia)](https://en.wikipedia.org/wiki/JPEG_2000) -- ITU-T T.800 wavelet compression
- [DICOM RLE Encoding](https://dicom.nema.org/medical/dicom/current/output/chtml/part05/sect_g.2.html) -- Part 5 Annex G PackBits
- [TSA DICOS Resources](https://www.tsa.gov/for-industry/dicos) -- TSA DICOS program information

## License

Licensed under MIT OR Apache-2.0.
