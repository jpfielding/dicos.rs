/// Encode data using the PackBits algorithm.
///
/// PackBits is a simple run-length encoding scheme:
/// - Header byte n >= 0: literal run of (n + 1) bytes follows
/// - Header byte n < 0 (and n != -128): repeat next byte (-n + 1) times
/// - Header byte -128: no-op (reserved)
pub fn encode_packbits(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        // Try to find a run of identical bytes
        let mut run_len = 1usize;
        while i + run_len < data.len() && run_len < 128 && data[i + run_len] == data[i] {
            run_len += 1;
        }

        if run_len > 1 {
            // Write run: header = -(run_len - 1), then the repeated byte
            // Cast to i8 after arithmetic to avoid overflow when run_len == 128
            let header = -((run_len as isize) - 1) as i8;
            out.push(header as u8);
            out.push(data[i]);
            i += run_len;
        } else {
            // Literal run: consume bytes until we hit a run of 3+ identical bytes
            let lit_start = i;
            let mut lit_len = 1usize;

            while i + lit_len < data.len() && lit_len < 128 {
                // Check if the next 3 bytes are identical (trigger run break)
                if i + lit_len + 2 < data.len()
                    && data[i + lit_len] == data[i + lit_len + 1]
                    && data[i + lit_len] == data[i + lit_len + 2]
                {
                    break;
                }
                lit_len += 1;
            }

            // Write literal: header = (lit_len - 1), then the literal bytes
            let header = (lit_len - 1) as u8;
            out.push(header);
            out.extend_from_slice(&data[lit_start..lit_start + lit_len]);
            i += lit_len;
        }
    }

    out
}

/// Decode PackBits compressed data.
///
/// `expected_len` is the expected decompressed size. If nonzero, decoding
/// stops once that many bytes have been produced.
pub fn decode_packbits(data: &[u8], expected_len: usize) -> Result<Vec<u8>, PackBitsError> {
    let mut out = if expected_len > 0 {
        Vec::with_capacity(expected_len)
    } else {
        Vec::new()
    };

    let mut i = 0;

    while i < data.len() {
        // Stop if we've reached the expected output length
        if expected_len > 0 && out.len() >= expected_len {
            break;
        }

        let n = data[i] as i8;
        i += 1;

        if n == -128 {
            // No-op
            continue;
        }

        if n >= 0 {
            // Literal run: read (n + 1) bytes
            let count = (n as usize) + 1;
            if i + count > data.len() {
                return Err(PackBitsError::TruncatedLiteral {
                    offset: i,
                    count,
                    remaining: data.len() - i,
                });
            }
            out.extend_from_slice(&data[i..i + count]);
            i += count;
        } else {
            // Replicate run: repeat next byte (-n + 1) times
            let count = (-n as usize) + 1;
            if i >= data.len() {
                return Err(PackBitsError::TruncatedRun { offset: i });
            }
            let val = data[i];
            i += 1;
            out.resize(out.len() + count, val);
        }
    }

    Ok(out)
}

/// Errors from PackBits decoding.
#[derive(Debug, thiserror::Error)]
pub enum PackBitsError {
    #[error("compressed data truncated in literal run at offset {offset}: need {count} bytes, have {remaining}")]
    TruncatedLiteral {
        offset: usize,
        count: usize,
        remaining: usize,
    },

    #[error("compressed data truncated in replicate run at offset {offset}")]
    TruncatedRun { offset: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_data() {
        assert!(encode_packbits(&[]).is_empty());
        assert!(decode_packbits(&[], 0).unwrap().is_empty());
    }

    #[test]
    fn single_byte() {
        let encoded = encode_packbits(&[42]);
        let decoded = decode_packbits(&encoded, 1).unwrap();
        assert_eq!(decoded, &[42]);
    }

    #[test]
    fn all_identical() {
        let data = vec![0xAB; 50];
        let encoded = encode_packbits(&data);
        let decoded = decode_packbits(&encoded, data.len()).unwrap();
        assert_eq!(decoded, data);
        // A run of 50 should compress significantly
        assert!(encoded.len() < data.len());
    }

    #[test]
    fn all_different() {
        let data: Vec<u8> = (0..50).collect();
        let encoded = encode_packbits(&data);
        let decoded = decode_packbits(&encoded, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn mixed_runs_and_literals() {
        // Pattern: 3 identical, 5 different, 4 identical
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xFF, 0xFF]); // run of 3
        data.extend_from_slice(&[1, 2, 3, 4, 5]); // literal
        data.extend_from_slice(&[0x42, 0x42, 0x42, 0x42]); // run of 4

        let encoded = encode_packbits(&data);
        let decoded = decode_packbits(&encoded, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn max_run_length() {
        // 128 is the max run length in PackBits
        let data = vec![0xBB; 128];
        let encoded = encode_packbits(&data);
        let decoded = decode_packbits(&encoded, data.len()).unwrap();
        assert_eq!(decoded, data);
        // Should be encoded as a single run: 2 bytes
        assert_eq!(encoded.len(), 2);
    }

    #[test]
    fn run_exceeds_max() {
        // 200 identical bytes should split into 128 + 72
        let data = vec![0xCC; 200];
        let encoded = encode_packbits(&data);
        let decoded = decode_packbits(&encoded, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn max_literal_length() {
        // 128 different bytes at max literal
        let data: Vec<u8> = (0..128).map(|i| (i * 7 + 13) as u8).collect();
        let encoded = encode_packbits(&data);
        let decoded = decode_packbits(&encoded, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn truncated_literal_error() {
        // header byte 0x02 means 3 literal bytes follow, but we only provide 1
        let bad_data = &[0x02, 0xFF];
        let result = decode_packbits(bad_data, 0);
        assert!(result.is_err());
    }

    #[test]
    fn truncated_run_error() {
        // header byte 0xFE (-2) means repeat 3 times, but no data byte follows
        let bad_data = &[0xFE_u8];
        let result = decode_packbits(bad_data, 0);
        assert!(result.is_err());
    }

    #[test]
    fn noop_byte_skipped() {
        // -128 (0x80) is a no-op in PackBits
        let encoded = &[0x80, 0x00, 0x42]; // noop, then literal 1 byte (0x42)
        let decoded = decode_packbits(encoded, 0).unwrap();
        assert_eq!(decoded, &[0x42]);
    }

    #[test]
    fn roundtrip_random_patterns() {
        let patterns: Vec<Vec<u8>> = vec![
            vec![0],
            vec![0, 0],
            vec![0, 1],
            vec![0, 0, 0],
            vec![0, 0, 1, 1, 1],
            vec![1, 2, 3, 3, 3, 4, 5],
            (0..255).collect(),
            vec![0; 1000],
            {
                // Alternating runs and literals
                let mut v = Vec::new();
                for i in 0..20u8 {
                    if i % 2 == 0 {
                        v.extend(std::iter::repeat(i).take(10));
                    } else {
                        v.extend((0..10).map(|j| i.wrapping_mul(10).wrapping_add(j)));
                    }
                }
                v
            },
        ];

        for (idx, data) in patterns.iter().enumerate() {
            let encoded = encode_packbits(data);
            let decoded = decode_packbits(&encoded, data.len())
                .unwrap_or_else(|e| panic!("pattern {idx}: decode failed: {e}"));
            assert_eq!(&decoded, data, "pattern {idx}: roundtrip mismatch");
        }
    }

    #[test]
    fn expected_len_stops_early() {
        let data = vec![0xAA; 100];
        let encoded = encode_packbits(&data);
        // Decode but request only 50 bytes
        let decoded = decode_packbits(&encoded, 50).unwrap();
        assert!(decoded.len() >= 50);
    }
}
