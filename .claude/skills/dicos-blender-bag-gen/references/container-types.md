# Container Types

Geometry and hardware details for each bag type. All dimensions in meters.

## Carry-On Roller Suitcase

The most common screening item. Hard or soft shell with telescoping handle and wheels.

| Property | Value |
|----------|-------|
| Exterior | 0.570 x 0.360 x 0.230 (22.5" x 14" x 9") |
| Shell | Cube with bevel, CT_BagShell density 3000 |
| Orientation | Lying flat in tray, wheels down |

**Hardware (all CT_Metal_High, density 15000):**
- Telescoping handle: 2 vertical cylinders r=0.008 d=0.20, cross brace r=0.006
- 4 spinner wheels: UV spheres r=0.012 at corners
- Internal frame: Cube 0.25x0.002x0.10 (aluminum stiffener)
- Top zipper: Cylinder r=0.002, length of bag
- Side zipper: Cylinder r=0.002, front pocket
- 2 zipper pulls: UV spheres r=0.004

## Backpack

Soft-sided, irregular shape. More organic packing, items shift around.

| Property | Value |
|----------|-------|
| Exterior | 0.450 x 0.300 x 0.200 (18" x 12" x 8") |
| Shell | Cube with heavy bevel (rounded), CT_BagShell density 3000 |
| Orientation | Lying on its back in tray, straps underneath |

**Hardware:**
- 2 shoulder strap buckles: Cube 0.02x0.015x0.003, CT_Metal_High
- Zipper: Cylinder r=0.002, U-shape around top
- Laptop sleeve internal divider: Cube 0.002x0.13x0.18, CT_Fabric_Low
- Metal frame stay (if present): Cube 0.15x0.002x0.002, CT_Metal_High

**Packing notes:** Items packed less neatly. Water bottle often in side pocket (cylinder pokes up). Laptop in dedicated sleeve at the back.

## Purse / Handbag

Small, dense, lots of metal hardware.

| Property | Value |
|----------|-------|
| Exterior | 0.300 x 0.200 x 0.150 (12" x 8" x 6") |
| Shell | Cube with bevel, CT_BagShell density 3000 |
| Orientation | Upright or on side |

**Hardware:**
- Metal clasp/latch: Cube 0.04x0.02x0.005, CT_Metal_High
- Chain strap (if present): series of small torus links, CT_Metal_High
- Metal feet (4): Cylinder r=0.004 d=0.003, CT_Metal_High
- Internal zipper pocket: Cylinder r=0.001

**Typical contents:** Wallet, phone, keys, lipstick/makeup (small cylinders, CT_Liquid_Medium), compact mirror (thin disk CT_Electronics), pen (cylinder CT_Metal_High), tissues (cube CT_Underwear), gum/mints (small cube CT_Food).

## Duffel Bag

Cylindrical soft bag. Large opening, loosely packed.

| Property | Value |
|----------|-------|
| Exterior | 0.550 x 0.280 x 0.280 (22" x 11" x 11") |
| Shell | Cylinder r=0.140 d=0.550, CT_BagShell density 3000 |
| Orientation | Lying on side in tray |

**Hardware:**
- End zipper: Cylinder r=0.002, circular path
- Shoulder strap hardware: 2 D-rings as small torus, CT_Metal_High
- 2 carry handles with metal rivets

**Packing notes:** Everything stuffed in chaotically. Clothes wrapped around fragile items. Shoes at one end.

## Laptop Bag / Briefcase

Thin, structured, electronics-heavy.

| Property | Value |
|----------|-------|
| Exterior | 0.400 x 0.300 x 0.080 (16" x 12" x 3") |
| Shell | Cube with slight bevel, CT_BagShell density 3000 |
| Orientation | Flat in tray |

**Hardware:**
- Metal zipper: Full perimeter
- Buckle/latch (if briefcase): Cube 0.03x0.015x0.005, CT_Metal_High
- Shoulder strap hook: Small D-ring

**Typical contents:** Laptop, charger, mouse (small UV sphere CT_Electronics), notebook (cube CT_Paper), pen, business cards (thin cube CT_Paper), phone charger cable.

## Messenger Bag

Cross-body, flap closure, moderate size.

| Property | Value |
|----------|-------|
| Exterior | 0.380 x 0.280 x 0.120 (15" x 11" x 5") |
| Shell | Cube with bevel, CT_BagShell density 3000 |
| Orientation | Flat in tray, flap side up |

**Hardware:**
- Magnetic flap closure: 2 cylinders r=0.008 d=0.003, CT_Metal_High
- Strap buckle: Cube 0.03x0.02x0.004, CT_Metal_High
- Internal organizer pockets with zippers

## Randomizing Container Choice

When generating varied training data, pick container type randomly:

```python
import random
containers = ['carry_on', 'backpack', 'purse', 'duffel', 'laptop_bag', 'messenger']
weights    = [0.30,       0.25,       0.15,    0.10,     0.10,         0.10]
choice = random.choices(containers, weights=weights, k=1)[0]
```

Carry-on rollers and backpacks are the most common checkpoint items.
