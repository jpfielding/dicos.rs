//! Set DICOS_CHARLS_INTEROP=1 with a C++ compiler and libcharls-dev installed.
use std::{fs, path::PathBuf, process::Command};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn charls_decodes_our_planar_rgb_output() {
    if std::env::var_os("DICOS_CHARLS_INTEROP").is_none() {
        eprintln!("skipping CharLS encode interop: set DICOS_CHARLS_INTEROP=1");
        return;
    }
    let dir = TempDir(std::env::temp_dir().join(format!("dicos-charls-{}", std::process::id())));
    fs::create_dir(&dir.0).unwrap();
    let executable = dir.0.join("charls-decode");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/charls_decode.cpp");
    assert!(Command::new("c++")
        .args(["-std=c++14", "-Wall", "-Wextra", "-Werror"])
        .arg(source)
        .arg("-lcharls")
        .arg("-o")
        .arg(&executable)
        .status()
        .unwrap()
        .success());
    for precision in [8, 12, 16] {
        for near in [0u8, 3] {
            let max = (1u32 << precision) - 1;
            let pixels: Vec<u16> = (0..3 * 35)
                .map(|i| ((i * 619 + i / 35 * 71) & max) as u16)
                .collect();
            let mut opts = jpegls::EncodeOptions::default();
            opts.precision = Some(precision);
            opts.near = near;
            let mut stream = Vec::new();
            jpegls::encode_image(&pixels, 7, 5, 3, &opts, &mut stream).unwrap();
            let input = dir.0.join("input.jls");
            let output = dir.0.join("output.raw");
            fs::write(&input, &stream).unwrap();
            assert!(
                Command::new(&executable)
                    .arg(input)
                    .arg(&output)
                    .status()
                    .unwrap()
                    .success(),
                "precision {precision}, near {near}"
            );
            let bytes = fs::read(output).unwrap();
            let decoded: Vec<u16> = if precision == 8 {
                bytes.into_iter().map(u16::from).collect()
            } else {
                bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_ne_bytes([b[0], b[1]]))
                    .collect()
            };
            assert_eq!(decoded.len(), pixels.len());
            for (&actual, &expected) in decoded.iter().zip(&pixels) {
                assert!(actual.abs_diff(expected) <= u16::from(near));
            }
            assert_eq!(decoded, jpegls::decode_image(&stream).unwrap().samples);
        }
    }
}
