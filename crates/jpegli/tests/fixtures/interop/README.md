# jpegli interop fixtures

Third-party JPEG Lossless (ITU-T T.81 Annex H, Process 14 SV1) streams produced
by **libjpeg-turbo `cjpeg`** and consumed by `../interop.rs`. They prove our
decoder is byte-interoperable with an independent conformant encoder, and
(env-gated) that `djpeg` accepts our encoder's output.

## Tool versions used to generate these fixtures

- `cjpeg` / `djpeg`: **libjpeg-turbo 3.1.4** (lossless requires **≥ 3.0**).

`cjpeg -lossless` and `-precision N` (2..16) are only available in
libjpeg-turbo 3.0+. Ubuntu's `libjpeg-turbo-progs` is still 2.x on 22.04/24.04
and **lacks** lossless — see `.github/workflows/ci.yml` for how CI obtains a 3.x
binary.

## Source images

Generated deterministically by this Python snippet (no external inputs):

```python
import struct
def wpgm(path, w, h, px, mv=65535):
    with open(path, 'wb') as f:
        f.write(f"P5\n{w} {h}\n{mv}\n".encode())
        fmt = '>H' if mv > 255 else 'B'
        f.write(b''.join(struct.pack(fmt, s & (0xffff if mv > 255 else 0xff)) for s in px))

# 16-bit gradient
wpgm("g16.pgm", 16, 16, [(x*4096 + y*63) & 0xffff for y in range(16) for x in range(16)])

# 16-bit pseudo-random (LCG) — exercises large modular differences incl. SSSS=16
st = 0x1234567
def rnd():
    global st
    st = (st*6364136223846793005 + 1442695040888963407) & 0xffffffffffffffff
    return (st >> 33) & 0xffff
wpgm("r16.pgm", 16, 16, [rnd() for _ in range(256)])

# 8-bit gradient
wpgm("g8.pgm", 16, 16, [(x*16 + y) & 0xff for y in range(16) for x in range(16)], mv=255)
```

## Stream generation

```sh
# Predictors (selection value / Ss) 1..7, 16-bit
for psv in 1 2 3 4 5 6 7; do
  cjpeg -precision 16 -lossless $psv -outfile g16_psv$psv.jpg g16.pgm
done

# Point transform Pt=1 (stores (sample >> 1) << 1)
cjpeg -precision 16 -lossless 1,1 -outfile g16_psv1_pt1.jpg g16.pgm

# Restart every MCU row (width=16 MCUs = 1 row)
cjpeg -precision 16 -lossless 1 -restart 16B -outfile g16_restart.jpg g16.pgm

# Random 16-bit, predictor 1 (large diffs / SSSS=16)
cjpeg -precision 16 -lossless 1 -outfile r16_psv1.jpg r16.pgm

# 8-bit precision
cjpeg -precision 8 -lossless 1 -outfile g8_psv1.jpg g8.pgm
```

## What the test asserts

- `decode_cjpeg_fixtures` (always runs): `jpegli::decode(fixture)` equals the
  source PGM samples. For `g16_psv1_pt1.jpg` the expected value is
  `(sample >> 1) << 1` (reduced-domain point transform).
- `encode_roundtrip_via_djpeg` (only when `DICOS_INTEROP` is set and `djpeg` is
  on `PATH`): our encoder output is decoded by `djpeg` back to the source pixels
  across several predictors and a restart interval.
