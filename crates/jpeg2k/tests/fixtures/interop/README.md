# jpeg2k interop fixtures

Third-party JPEG 2000 codestreams (ITU-T T.800 Part 1, lossless) produced by
**OpenJPEG `opj_compress`** and consumed by `../interop.rs`.

## OpenJPEG-conformant (both directions pass)

These fixtures round-trip against OpenJPEG in both directions: we decode
`opj_compress` streams to the exact source pixels, and `opj_decompress` exactly
reproduces our encoder's output. Reaching this required fixing two
self-consistent conformance defects (our own round-trip masked both):

1. **MQ arithmetic coder** — was using the `C += A` interval convention (MPS
   sub-interval at the base) rather than the normative T.800 Annex C / OpenJPEG
   `C += Qe` convention (LPS at the base). Non-interoperable codewords; symptom
   was every coefficient decoding to 0 (samples ≈ 2^15).
2. **Forward 2-D 5/3 DWT** — transformed rows before columns; the reference
   transforms columns before rows, and the floor-rounded lifting makes the pass
   order visible in the integer coefficients (off-by-one).

Byte-diffing our 0-level (`num_decomp_levels = 0`) encoding against
`opj_compress -n 1` isolated defect 1 (SIZ/COD/QCD markers and packet-header
prefix were already byte-identical; only the MQ coefficient bytes diverged);
defect 2 surfaced once the entropy layer was conformant and any DWT level was
present.

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
