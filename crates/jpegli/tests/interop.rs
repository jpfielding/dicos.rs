//! Interop tests against libjpeg-turbo (`cjpeg`/`djpeg`, ≥ 3.0 for lossless).
//!
//! Two directions:
//!
//! * **Decode** (`decode_cjpeg_fixtures`): always runs. Decodes the checked-in
//!   `tests/fixtures/interop/*.jpg` streams — produced by `cjpeg -lossless` —
//!   and asserts our output equals the source PGM samples (with the point
//!   transform applied for the `_pt1` fixture). See `fixtures/interop/README.md`
//!   for exact generation commands and tool versions.
//! * **Encode** (`encode_roundtrip_via_djpeg`): env + tool gated. Encodes with
//!   our codec, hands the stream to `djpeg`, and compares pixels. Skips with a
//!   note unless `DICOS_INTEROP` is set *and* `djpeg` is on `PATH`, so a plain
//!   `cargo test` passes everywhere while CI (which sets both) really runs it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/interop")
}

/// Parse a binary PGM (P5), 8- or 16-bit (big-endian per the netpbm spec).
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
    i += 1; // single whitespace after maxval
    let (w, h, mv) = (toks[0], toks[1], toks[2]);
    let px: Vec<u16> = if mv > 255 {
        d[i..].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect()
    } else {
        d[i..].iter().map(|&b| b as u16).collect()
    };
    (w, h, px)
}

/// Each fixture: stream file, its source PGM, and the point-transform shift.
struct Fixture {
    jpg: &'static str,
    src: &'static str,
    point_transform: u8,
}

fn fixtures() -> Vec<Fixture> {
    let g16 = |jpg| Fixture { jpg, src: "g16.pgm", point_transform: 0 };
    vec![
        g16("g16_psv1.jpg"),
        g16("g16_psv2.jpg"),
        g16("g16_psv3.jpg"),
        g16("g16_psv4.jpg"),
        g16("g16_psv5.jpg"),
        g16("g16_psv6.jpg"),
        g16("g16_psv7.jpg"),
        g16("g16_restart.jpg"),
        // Point transform Pt=1: cjpeg stores (sample >> 1) << 1.
        Fixture { jpg: "g16_psv1_pt1.jpg", src: "g16.pgm", point_transform: 1 },
        // Random 16-bit: statistically exercises large modular differences,
        // including the SSSS=16 (32768) case.
        Fixture { jpg: "r16_psv1.jpg", src: "r16.pgm", point_transform: 0 },
        // 8-bit precision.
        Fixture { jpg: "g8_psv1.jpg", src: "g8.pgm", point_transform: 0 },
    ]
}

/// Decode direction — always runs against checked-in fixtures.
#[test]
fn decode_cjpeg_fixtures() {
    let dir = fixture_dir();
    for f in fixtures() {
        let (w, h, mut expected) = read_pgm(&dir.join(f.src));
        if f.point_transform > 0 {
            let pt = f.point_transform;
            for s in &mut expected {
                *s = (*s >> pt) << pt;
            }
        }
        let bytes = std::fs::read(dir.join(f.jpg))
            .unwrap_or_else(|e| panic!("missing fixture {}: {e}", f.jpg));
        let (decoded, dw, dh) = jpegli::decode(&bytes, w, h)
            .unwrap_or_else(|e| panic!("{}: decode failed: {e}", f.jpg));
        assert_eq!((dw, dh), (w, h), "{}: dimensions", f.jpg);
        assert_eq!(decoded, expected, "{}: pixel mismatch vs cjpeg source", f.jpg);
    }
}

fn tool_on_path(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .map(|_| true)
        .unwrap_or(false)
}

/// Encode direction — gated on `DICOS_INTEROP` + `djpeg` presence.
#[test]
fn encode_roundtrip_via_djpeg() {
    if std::env::var_os("DICOS_INTEROP").is_none() {
        eprintln!("skipping encode_roundtrip_via_djpeg: DICOS_INTEROP not set");
        return;
    }
    if !tool_on_path("djpeg") {
        eprintln!("skipping encode_roundtrip_via_djpeg: djpeg not on PATH");
        return;
    }

    let tmp = std::env::temp_dir();
    // A handful of predictors and a restart-bearing config, all 16-bit.
    let cases: &[(u8, u16)] = &[(1, 0), (4, 0), (7, 0), (1, 2)];
    let (w, h) = (32u32, 24u32);
    let src: Vec<u16> = (0..(w * h))
        .map(|i| ((i.wrapping_mul(2654435761) >> 8) & 0xffff) as u16)
        .collect();

    for &(predictor, restart_rows) in cases {
        let mut opts = jpegli::EncodeOptions::default();
        opts.predictor = predictor;
        opts.restart_interval_rows = restart_rows;
        let mut buf = Vec::new();
        jpegli::encode_with_options(&src, w, h, &opts, &mut buf)
            .unwrap_or_else(|e| panic!("encode psv{predictor}: {e}"));

        let jpg = tmp.join(format!("dicos_jpegli_p{predictor}_r{restart_rows}.jpg"));
        let out = tmp.join(format!("dicos_jpegli_p{predictor}_r{restart_rows}.pgm"));
        std::fs::write(&jpg, &buf).unwrap();
        let status = Command::new("djpeg")
            .args(["-pnm", "-outfile"])
            .arg(&out)
            .arg(&jpg)
            .status()
            .expect("run djpeg");
        assert!(status.success(), "djpeg rejected our psv{predictor} output");

        let (dw, dh, decoded) = read_pgm(&out);
        assert_eq!((dw, dh), (w, h), "psv{predictor}: djpeg dimensions");
        assert_eq!(decoded, src, "psv{predictor}: djpeg pixel mismatch");
    }
}
