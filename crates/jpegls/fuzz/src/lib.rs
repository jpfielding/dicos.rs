//! Helpers shared by the `pure_jpegls` fuzz targets: a forgiving byte cursor,
//! the fuzz-input → image description used by the round-trip targets, and the
//! assertions every accepted decode must satisfy.

/// Reads fuzz bytes front to back, yielding `0` once the input runs out so a
/// short input is still a complete (if dull) description.
pub struct Input<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Input<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn u8(&mut self) -> u8 {
        let b = self.data.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.u8(), self.u8()])
    }

    /// Everything not yet consumed.
    pub fn rest(&mut self) -> &'a [u8] {
        let start = self.pos.min(self.data.len());
        self.pos = self.data.len();
        &self.data[start..]
    }
}

/// `(1 << precision) - 1`.
pub fn max_val(precision: u8) -> u32 {
    (1u32 << precision) - 1
}

/// The largest NEAR T.87 C.2.4.1.1 allows for `max_val`: `min(255, MAXVAL/2)`.
pub fn near_max(max_val: u32) -> u8 {
    (max_val / 2).min(255) as u8
}

/// `n` samples taken as little-endian pairs from `body` (cycled to fill), each
/// masked with `mask`. An empty body yields zeros.
pub fn samples_from(body: &[u8], n: usize, mask: u32) -> Vec<u16> {
    (0..n)
        .map(|i| {
            if body.is_empty() {
                return 0;
            }
            let lo = body[(2 * i) % body.len()];
            let hi = body[(2 * i + 1) % body.len()];
            (u32::from(u16::from_le_bytes([lo, hi])) & mask) as u16
        })
        .collect()
}

/// A valid `encode_image` request described by fuzz bytes. Every input maps to
/// one, so a target built on it spends no time on rejected requests:
///
/// - byte 0: component count (bit 0: 1 or 3);
/// - byte 1: precision `2 + (b % 15)`, i.e. `2..=16`;
/// - bytes 2, 3: width and height, `1 + (b % 64)`;
/// - byte 4: NEAR — 0 unless the top bit is set, then bounded by
///   `min(255, MAXVAL / 2)`;
/// - the rest: sample bytes (little-endian pairs, masked to MAXVAL), cycled to
///   fill the image.
#[derive(Debug)]
pub struct ImageSpec {
    pub components: u8,
    pub precision: u8,
    pub width: u32,
    pub height: u32,
    pub near: u8,
    /// Component-planar, `width * height * components` long.
    pub samples: Vec<u16>,
}

impl ImageSpec {
    /// The header bytes this consumes before the sample body.
    pub const HEADER_LEN: usize = 5;

    /// `None` for inputs shorter than [`Self::HEADER_LEN`].
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::HEADER_LEN {
            return None;
        }
        let components: u8 = if data[0] & 1 == 1 { 3 } else { 1 };
        let precision: u8 = 2 + data[1] % 15;
        let width = 1 + u32::from(data[2] % 64);
        let height = 1 + u32::from(data[3] % 64);
        let max_val = max_val(precision);
        let near: u8 = if data[4] & 0x80 == 0 {
            0
        } else {
            let max_near = u32::from(near_max(max_val));
            (u32::from(data[4] & 0x7f) % (max_near + 1)) as u8
        };
        let n = width as usize * height as usize * usize::from(components);
        let samples = samples_from(&data[Self::HEADER_LEN..], n, max_val);
        Some(Self {
            components,
            precision,
            width,
            height,
            near,
            samples,
        })
    }

    pub fn encode_options(&self) -> jpegls::EncodeOptions {
        let mut opts = jpegls::EncodeOptions::default();
        opts.near = self.near;
        opts.precision = Some(self.precision);
        opts
    }

    /// Encode through `encode_image`, which must accept every spec.
    pub fn encode(&self) -> Vec<u8> {
        let mut stream = Vec::new();
        jpegls::encode_image(
            &self.samples,
            self.width,
            self.height,
            self.components,
            &self.encode_options(),
            &mut stream,
        )
        .expect("encode_image rejected a valid request");
        stream
    }
}

/// Assert the [`jpegls::DecodedImage`] contract for a stream the decoder
/// accepted: 1 or 3 components, interleave mode 0, precision `2..=16`, exactly
/// `width * height * components` samples, and no sample above `max_sample`
/// (`2^P - 1`, or the LSE MAXVAL when the caller knows a preset applies).
pub fn assert_image_contract(img: &jpegls::DecodedImage, max_sample: Option<u32>) {
    assert!(
        img.components == 1 || img.components == 3,
        "components = {}",
        img.components
    );
    assert_eq!(img.interleave_mode, 0);
    assert!(
        (2..=16).contains(&img.bits_per_sample),
        "bits_per_sample = {}",
        img.bits_per_sample
    );
    let expected = img.width as usize * img.height as usize * usize::from(img.components);
    assert_eq!(img.samples.len(), expected);
    let max = max_sample.unwrap_or_else(|| max_val(img.bits_per_sample));
    assert!(
        img.samples.iter().all(|&s| u32::from(s) <= max),
        "a sample exceeds the maximum {max}"
    );
}

/// For a single-component stream `decode_image` accepted, `decode` at the
/// stream's own dimensions must accept it too and return the same samples.
pub fn assert_decode_agrees(data: &[u8], img: &jpegls::DecodedImage) {
    if img.components != 1 {
        return;
    }
    let (samples, w, h) = jpegls::decode(data, img.width, img.height)
        .expect("decode rejected a grayscale stream decode_image accepted");
    assert_eq!((w, h), (img.width, img.height));
    assert_eq!(samples, img.samples, "decode and decode_image disagree");
}

/// Every decoded sample equals the original (`near == 0`) or is within ±NEAR.
pub fn assert_within_near(decoded: &[u16], original: &[u16], near: u8) {
    assert_eq!(decoded.len(), original.len(), "sample count changed");
    if near == 0 {
        assert_eq!(decoded, original, "lossless round-trip changed samples");
        return;
    }
    for (i, (&d, &o)) in decoded.iter().zip(original).enumerate() {
        let err = (i32::from(d) - i32::from(o)).unsigned_abs();
        assert!(
            err <= u32::from(near),
            "sample {i}: |{d} - {o}| = {err} > NEAR = {near}"
        );
    }
}
