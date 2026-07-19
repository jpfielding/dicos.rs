# jpeg2k interop fixtures

Third-party JPEG 2000 codestreams (ITU-T T.800 Part 1, lossless) produced by
**OpenJPEG `opj_compress`** and consumed by `../interop.rs`.

## ⚠️ Blocked on a real conformance bug — tests are `#[ignore]`d

Our decoder parses these fixtures structurally (returns `Ok`) but produces
wrong pixels: every sample decodes to ≈2^15 (all entropy coefficients ≈ 0). The
same failure occurs in reverse — `opj_decompress` on our encoder's output also
yields ≈2^15. Our own encode→decode round-trip is perfect, so our encoder and
decoder share a **non-conformant** EBCOT tier-1 / MQ convention.

Byte-diffing our 0-level (`num_decomp_levels = 0`) encoding against
`opj_compress -n 1` of the same image shows the SIZ/COD/QCD markers and the
packet-header prefix are **byte-identical**; only the MQ-coded coefficient bytes
diverge. The defect is isolated to tier-1/MQ coefficient coding.

These fixtures are the regression proof. Once the tier-1/MQ coder is fixed,
delete the `#[ignore]` on both tests in `../interop.rs`; both directions must
then pass.

## Tool versions

- `opj_compress` / `opj_decompress`: **OpenJPEG 2.5.4** (Ubuntu package
  `libopenjp2-tools`).

## Source images

Generated deterministically by this Python snippet:

```python
import struct
def wpgm(path, w, h, px):
    with open(path, 'wb') as f:
        f.write(f"P5\n{w} {h}\n65535\n".encode())
        f.write(b''.join(struct.pack('>H', s & 0xffff) for s in px))

# 16-bit gradient 64x64
wpgm("gradient-64x64.pgm", 64, 64,
     [(x*65535//64) ^ ((y*3) & 0xffff) for y in range(64) for x in range(64)])

# odd dims 61x47
wpgm("odd-61x47.pgm", 61, 47,
     [(x*997 + y*131) & 0xffff for y in range(47) for x in range(61)])

# random-seeded 32x32 (LCG)
st = 0xABCDEF12
def rnd():
    global st
    st = (st*6364136223846793005 + 1442695040888963407) & 0xffffffffffffffff
    return (st >> 33) & 0xffff
wpgm("random-32x32.pgm", 32, 32, [rnd() for _ in range(1024)])
```

All PGMs are true 16-bit (maxval 65535). `opj_compress` writes `Ssiz = 0x0F`
(16-bit unsigned), which is exactly the single-component precision our decoder's
support matrix requires.

## Stream generation

```sh
# Default = reversible 5/3, lossless, single tile, LRCP, 1 layer.
opj_compress -i gradient-64x64.pgm -o gradient-64x64.j2k
opj_compress -i odd-61x47.pgm      -o odd-61x47.j2k
opj_compress -i random-32x32.pgm   -o random-32x32.j2k

# Non-default 32x32 code-blocks.
opj_compress -i random-32x32.pgm   -o random-32x32-cb32.j2k -b 32,32
```

Each was verified to round-trip losslessly through `opj_decompress` at
generation time.
