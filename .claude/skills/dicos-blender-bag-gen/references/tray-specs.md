# TSA Screening Tray Specifications

## Default Tray: Standard Flat

Use the **standard flat tray** as the default unless the user requests otherwise.

| Type | Outer (L x W x H) | Interior (L x W x H) | Wall Thick | Use |
|------|-------------------|----------------------|------------|-----|
| **Standard flat** (default) | **660 x 420 x 75mm** | **640 x 400 x 65mm** | **~10mm** | **Most common checkpoint tray** |
| Deep bin | 640 x 420 x 150mm | 620 x 400 x 140mm | ~10mm | Carry-on rollers, shoes |
| Large deep bin | 700 x 480 x 150mm | 680 x 460 x 140mm | ~10mm | Oversize bags |

## Scanner Tunnel Constraints

Smiths HI-SCAN 6040 CTiX (common checkpoint scanner):
- Tunnel opening: 620mm W x 420mm H
- Belt width: 600mm
- Tray must fit through tunnel, so max tray width ~700mm

## Blender Geometry

All dimensions in meters (Blender default units). Place tray at origin, bottom at Z=0.

```python
# Standard flat tray (DEFAULT)
TRAY_L = 0.660    # length (X)
TRAY_W = 0.420    # width (Y)
TRAY_H = 0.075    # height (Z)
WALL   = 0.010    # wall thickness
FLOOR  = 0.004    # floor thickness

# Interior bounds (for placement validation)
INTERIOR_X = (-0.320, 0.320)   # ±320mm
INTERIOR_Y = (-0.200, 0.200)   # ±200mm
FLOOR_Z    = 0.005              # items sit here (floor + 1mm gap)
```

### Construction Steps

1. **Outer shell**: `primitive_cube_add(size=1)` at `(0, 0, TRAY_H/2)`, scale to `(TRAY_L, TRAY_W, TRAY_H)`, apply scale
2. **Interior cavity**: `primitive_cube_add` at `(0, 0, FLOOR + inner_h/2)`, scale to `(TRAY_L - 2*WALL, TRAY_W - 2*WALL, TRAY_H - FLOOR)`, apply scale. Boolean DIFFERENCE from outer shell, delete cavity.
3. **Edge bevel**: Bevel modifier, width=0.003, segments=2, apply.
4. **Handle cutouts**: On both short ends (±TRAY_L/2 in X), add a cylinder `r=0.025, depth=WALL+0.004`, rotated 90° around Y, scaled `(1, 2.5, 1)` to make an oval slot. Located at `z=TRAY_H - 0.015`. Boolean DIFFERENCE from tray, delete cylinder. Repeat for other end.
5. **Rolled rim lip**: Create a thin cube `(TRAY_L+0.006, TRAY_W+0.006, 0.003)` at `z=TRAY_H`. Boolean subtract a slightly smaller cube to leave only the 3mm edge flange. Join to tray.

```python
# Handle cutouts (both short ends)
for x_sign in [-1, 1]:
    bpy.ops.mesh.primitive_cylinder_add(
        radius=0.025, depth=WALL + 0.004,
        location=(x_sign * TRAY_L/2, 0, TRAY_H - 0.015),
        rotation=(0, math.radians(90), 0))
    slot = bpy.context.active_object
    slot.scale = (1, 2.5, 1)  # oval
    bpy.ops.object.transform_apply(scale=True)
    # Boolean DIFFERENCE from tray, then delete slot

# Rolled rim lip (3mm outward flange at top edge)
# Outer rim cube (TRAY_L+6mm, TRAY_W+6mm, 3mm) at z=TRAY_H
# Subtract inner cube (TRAY_L-2mm, TRAY_W-2mm, 6mm) to leave edge only
# Join to tray
```

## The Tray is Solid — Non-Penetration Rules

**The tray is a rigid, solid object. Nothing may overlap or intersect with it.**

1. **No object may extend below Z=0.** The tray bottom is the absolute floor.
2. **No object may extend outside the tray outer walls in XY.** Even above the rim, items must stay within the tray's XY footprint so they don't fall off during belt transport.
3. **Items sit ON the tray floor at Z=0.005** (4mm floor + 1mm clearance). No mesh vertices below this Z.
4. **Items must be at least 5mm inside the tray interior walls** in XY — no touching or overlapping the wall mesh.
5. **No mesh-to-mesh overlap between any objects.** Bags don't penetrate the tray. Bags don't penetrate each other. Use bounding box checks after placement.
6. **During voxelization**, tray mask is built first and is authoritative — tray voxels are never overwritten by any other object.

## Sizing Containers to Fit

The standard flat tray interior is only 640 x 400mm. **Do not stack bags on top of each other.** When multiple containers don't fit side-by-side:
- **Downsize the containers** until they fit within the tray interior with gaps
- Items extending above the tray rim is normal and expected
- Leave at least 5mm gap between containers
- All containers sit directly on the tray floor

## Gravity Rule

Every object must obey gravity and rest on a supporting surface:
- Containers sit on the **tray floor**
- Items inside bags sit on the **bag interior floor** or on **other items below them**
- Nothing floats in mid-air — pack bottom-up, tracking running Z height
- Loose tray items (watches, phones) sit flat on the tray floor

## Validation Check

After placing all containers and loose items, run this validation:

```python
tray_interior_x = (-0.320, 0.320)
tray_interior_y = (-0.200, 0.200)
margin = 0.005  # 5mm wall clearance

for obj in items:
    bb = world_bounding_box(obj)
    assert bb.x_min >= tray_interior_x[0] + margin
    assert bb.x_max <= tray_interior_x[1] - margin
    assert bb.y_min >= tray_interior_y[0] + margin
    assert bb.y_max <= tray_interior_y[1] - margin
    assert bb.z_min >= 0.004  # above tray floor
```

## Tray Material

- Injection-molded polypropylene
- CT density: stored 2524 (+1500 HU apparent) for simulation visibility
- Color in Blender: opaque dark blue-gray
- Typically gray or dark gray in reality

## Real Scan Reference

Analysis of a real checkpoint CT image (312x312x375 voxels):

```
Air baseline:   ~32751 (stored uint16)
Tray floor:     z=0-10, density +500 to +900 above air
Tray walls:     z=6-20, thin (12-15 voxels), density +800 to +900
Bag contents:   z=20+ onward, density up to +14000 for metal
Tray position:  not centered — sits near one edge of the FOV
Tray X extent:  ~180 voxels at floor level
```

The real tray is FAINT in CT (~900 HU above air = polypropylene at ~1.1 g/cm³). Our simulation uses +1500 HU for the tray to ensure visibility — this simulates the partial-volume and beam-hardening effects that make thin plastic more visible at the shell boundary in real scans.
