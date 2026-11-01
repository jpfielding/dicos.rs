use jpegls::{
    decode_image, decode_image_with_options, encode_image, CodecError, DecodeImageOptions,
    EncodeOptions,
};

fn rgb() -> (Vec<u8>, Vec<u16>) {
    let pixels: Vec<u16> = (0..48).map(|i| (i * 5) as u16).collect();
    let mut stream = Vec::new();
    encode_image(&pixels, 4, 4, 3, &EncodeOptions::default(), &mut stream).unwrap();
    (stream, pixels)
}

fn scans(stream: &[u8]) -> Vec<usize> {
    stream
        .windows(2)
        .enumerate()
        .filter_map(|(i, b)| (b == [0xff, 0xda]).then_some(i))
        .collect()
}

#[test]
fn rgb_selectors_are_distinct_and_unknown_or_duplicate_selectors_fail() {
    let (stream, pixels) = rgb();
    let sos = scans(&stream);
    assert_eq!(sos.len(), 3);
    for (i, &offset) in sos.iter().enumerate() {
        assert_eq!(stream[offset + 5], (i + 1) as u8);
    }
    assert_eq!(decode_image(&stream).unwrap().samples, pixels);
    for selector in [1, 99] {
        let mut bad = stream.clone();
        bad[sos[1] + 5] = selector;
        assert!(matches!(
            decode_image(&bad),
            Err(CodecError::InvalidData(_))
        ));
    }
}

#[test]
fn reordered_scans_are_returned_in_frame_component_order() {
    let (stream, pixels) = rgb();
    let sos = scans(&stream);
    let mut reordered = stream[..sos[0]].to_vec();
    reordered.extend_from_slice(&stream[sos[2]..stream.len() - 2]);
    reordered.extend_from_slice(&stream[sos[0]..sos[1]]);
    reordered.extend_from_slice(&stream[sos[1]..sos[2]]);
    reordered.extend_from_slice(&[0xff, 0xd9]);
    assert_eq!(decode_image(&reordered).unwrap().samples, pixels);
}

#[test]
fn component_ids_need_not_start_at_one_but_must_be_unique() {
    let (mut stream, pixels) = rgb();
    let sof = stream.windows(2).position(|b| b == [0xff, 0xf7]).unwrap();
    let sos = scans(&stream);
    for (i, id) in [7, 0, 42].into_iter().enumerate() {
        stream[sof + 10 + 3 * i] = id;
        stream[sos[i] + 5] = id;
    }
    assert_eq!(decode_image(&stream).unwrap().samples, pixels);
    stream[sof + 13] = 7;
    assert!(matches!(
        decode_image(&stream),
        Err(CodecError::InvalidData(_))
    ));
}

#[test]
fn output_budget_counts_every_component_and_accepts_exact_boundary() {
    let (stream, pixels) = rgb();
    for limit in [0, 32, 95] {
        let err = decode_image_with_options(
            &stream,
            &DecodeImageOptions {
                max_decoded_bytes: limit,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CodecError::InvalidData(ref message) if message.contains("limit")));
    }
    assert_eq!(
        decode_image_with_options(
            &stream,
            &DecodeImageOptions {
                max_decoded_bytes: 96
            }
        )
        .unwrap()
        .samples,
        pixels
    );
}

#[test]
fn tiny_stream_with_large_geometry_hits_budget_before_entropy_decode() {
    let (mut stream, _) = rgb();
    let sof = stream.windows(2).position(|b| b == [0xff, 0xf7]).unwrap();
    stream[sof + 5..sof + 7].copy_from_slice(&8192u16.to_be_bytes());
    stream[sof + 7..sof + 9].copy_from_slice(&8192u16.to_be_bytes());
    assert!(stream.len() < 512);
    assert!(
        matches!(decode_image(&stream), Err(CodecError::InvalidData(ref m)) if m.contains("limit"))
    );
}

#[test]
fn unsupported_component_counts_fail_before_writing() {
    for count in [0, 2, 4, 255] {
        let mut output = Vec::new();
        assert!(matches!(
            encode_image(
                &vec![0; count as usize],
                1,
                1,
                count,
                &EncodeOptions::default(),
                &mut output
            ),
            Err(CodecError::InvalidParameter {
                name: "components",
                ..
            })
        ));
        assert!(output.is_empty());
    }
}

#[test]
fn malformed_sof_and_sos_lengths_fail_in_both_entry_points() {
    let mut stream = Vec::new();
    encode_image(&[0; 16], 4, 4, 1, &EncodeOptions::default(), &mut stream).unwrap();
    for marker in [0xf7, 0xda] {
        let offset = stream.windows(2).position(|b| b == [0xff, marker]).unwrap();
        let valid = stream[offset + 3];
        for length in [2, 5, valid - 1, valid + 1] {
            let mut bad = stream.clone();
            bad[offset + 3] = length;
            assert!(matches!(
                decode_image(&bad),
                Err(CodecError::InvalidData(_))
            ));
            assert!(matches!(
                jpegls::decode(&bad, 4, 4),
                Err(CodecError::InvalidData(_))
            ));
        }
    }
}
