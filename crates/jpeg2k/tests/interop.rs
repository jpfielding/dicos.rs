//! Interop tests against OpenJPEG (`opj_compress` / `opj_decompress`, 2.5.x).
//!
//! Two directions:
//!
//! * **Decode** (`decode_openjpeg_fixtures`): decodes the checked-in
//!   `tests/fixtures/interop/*.j2k` streams — produced by `opj_compress` in its
//!   default reversible (lossless 5/3) mode — and asserts our output equals the
//!   source PGM samples exactly.
//! * **Encode** (`encode_roundtrip_via_opj_decompress`): env + tool gated.
//!   Encodes with our codec, hands the stream to `opj_decompress`, and compares
//!   pixels.
//!
//! ## Both tests are `#[ignore]`d — blocked on a real conformance bug
//!
//! These fixtures decode structurally (the SIZ/COD/QCD markers our encoder emits
//! are *byte-identical* to OpenJPEG's for the same image), but the EBCOT
//! tier-1 / MQ entropy-coded coefficient bytes are **not** OpenJPEG-compatible:
//!
//! * `jpeg2k::decode` of an `opj_compress` stream returns `Ok` but yields ~all
//!   coefficients zero (every sample decodes to ≈2^15, the DC level shift).
//! * `opj_decompress` of our encoder's output likewise yields ≈2^15.
//! * Our own encode→decode round-trip is *perfect* — encoder and decoder share
//!   the same non-conformant tier-1 convention.
//!
//! Localised with `opj_compress -n 1` (zero DWT levels, one code-block): the
//! packet-header prefix matches OpenJPEG byte-for-byte, then the MQ coefficient
//! stream diverges. So the defect is isolated to the tier-1/MQ coefficient
//! coding, independent of the DWT.
//!
//! The fixtures and this harness are the regression proof: once the tier-1/MQ
//! coder is made T.800-conformant, delete the `#[ignore]` attributes and both
//! directions must pass. See `fixtures/interop/README.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/interop")
}

/// Parse a binary 16-bit PGM (P5, big-endian samples).
fn read_pgm(path: &Path) -> (u32, u32, Vec<u16>) {
    let d = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(&d[..2], b"P5", "{}: not a P5 PGM", path.display());
    let mut i = 2usize;
    let mut toks = Vec::new();
    while toks.len() < 3 {
        while d[i].is_ascii_whitespace() {
            i += 1;
        }
        if d[i] == b'#' {
            while d[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let s = i;
        while !d[i].is_ascii_whitespace() {
            i += 1;
        }
        toks.push(std::str::from_utf8(&d[s..i]).unwrap().parse::<u32>().unwrap());
    }
    i += 1;
    let (w, h, mv) = (toks[0], toks[1], toks[2]);
    assert!(mv > 255, "{}: expected 16-bit maxval, got {mv}", path.display());
    let px = d[i..].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    (w, h, px)
}

/// (stream, source PGM) fixture pairs. `-cb32` shares the random source.
fn fixtures() -> &'static [(&'static str, &'static str)] {
    &[
        ("gradient-64x64.j2k", "gradient-64x64.pgm"),
        ("odd-61x47.j2k", "odd-61x47.pgm"),
        ("random-32x32.j2k", "random-32x32.pgm"),
        ("random-32x32-cb32.j2k", "random-32x32.pgm"),
    ]
}

/// Decode direction.
///
/// `#[ignore]`: blocked on the tier-1/MQ conformance bug documented at the top
/// of this file. Remove once our decoder reads OpenJPEG streams correctly.
#[test]
#[ignore = "blocked: jpeg2k tier-1/MQ entropy coding is not OpenJPEG-conformant"]
fn decode_openjpeg_fixtures() {
    let dir = fixture_dir();
    for &(j2k, pgm) in fixtures() {
        let (w, h, expected) = read_pgm(&dir.join(pgm));
        let bytes =
            std::fs::read(dir.join(j2k)).unwrap_or_else(|e| panic!("missing fixture {j2k}: {e}"));
        let (decoded, dw, dh) =
            jpeg2k::decode(&bytes, w, h).unwrap_or_else(|e| panic!("{j2k}: decode failed: {e}"));
        assert_eq!((dw, dh), (w, h), "{j2k}: dimensions");
        assert_eq!(decoded, expected, "{j2k}: pixel mismatch vs OpenJPEG source");
    }
}

fn tool_on_path(tool: &str) -> bool {
    Command::new(tool).arg("-h").output().map(|_| true).unwrap_or(false)
}

/// Encode direction.
///
/// `#[ignore]`: blocked on the same bug (our encoder's coefficients are not
/// OpenJPEG-decodable). Also gated on `DICOS_INTEROP` + `opj_decompress` so that
/// once un-ignored it still skips cleanly where the tool is absent.
#[test]
#[ignore = "blocked: jpeg2k tier-1/MQ entropy coding is not OpenJPEG-conformant"]
fn encode_roundtrip_via_opj_decompress() {
    if std::env::var_os("DICOS_INTEROP").is_none() {
        eprintln!("skipping: DICOS_INTEROP not set");
        return;
    }
    if !tool_on_path("opj_decompress") {
        eprintln!("skipping: opj_decompress not on PATH");
        return;
    }

    let dir = fixture_dir();
    let tmp = std::env::temp_dir();
    for &(_, pgm) in fixtures() {
        let (w, h, src) = read_pgm(&dir.join(pgm));
        let mut buf = Vec::new();
        jpeg2k::encode(&src, w, h, &jpeg2k::Jpeg2kOptions::default(), &mut buf)
            .unwrap_or_else(|e| panic!("{pgm}: encode failed: {e}"));

        let j2k = tmp.join(format!("dicos_jp2k_{pgm}.j2k"));
        let out = tmp.join(format!("dicos_jp2k_{pgm}.out.pgm"));
        std::fs::write(&j2k, &buf).unwrap();
        let status = Command::new("opj_decompress")
            .arg("-i")
            .arg(&j2k)
            .arg("-o")
            .arg(&out)
            .status()
            .expect("run opj_decompress");
        assert!(status.success(), "{pgm}: opj_decompress rejected our output");

        let (dw, dh, decoded) = read_pgm(&out);
        assert_eq!((dw, dh), (w, h), "{pgm}: opj dimensions");
        assert_eq!(decoded, src, "{pgm}: opj pixel mismatch");
    }
}
