# Voxelization Algorithm

Convert a Blender scene to a 3D voxel grid suitable for DICOS CT export.

## Configuration

```python
RES_X, RES_Y, RES_Z = 256, 256, 128  # Grid resolution
BOUNDS_MIN = (-0.32, -0.22, -0.01)     # Scene bounds (meters)
BOUNDS_MAX = (0.32, 0.22, 0.25)
SHELL_THICKNESS = 2                     # Voxels for bag/tray shells
```

## Algorithm

### Phase 1: Build BVH Trees

For every MESH/CURVE object with a non-zero density:

```python
bm = bmesh.new()
bm.from_mesh(evaluated_mesh)
bm.transform(obj.matrix_world)
bvh = BVHTree.FromBMesh(bm)
```

### Phase 2: Point-in-Mesh Test

```python
def point_inside_mesh(bvh, point):
    direction = Vector((0, 0, 1))
    count, origin = 0, point.copy()
    for _ in range(20):
        hit, normal, idx, dist = bvh.ray_cast(origin, direction)
        if hit is None: break
        count += 1
        origin = hit + direction * 0.0001
    return count % 2 == 1
```

### Phase 3: Tray Mask (Solid)

Sample every voxel in tray bounding box. Mark inside voxels in `tray_mask` bytearray.

### Phase 4: Bag Mask (Solid, Tray-Excluded)

Sample every voxel in bag bounding box. Skip any voxel where `tray_mask[idx] == 1`. Mark inside voxels in `bag_mask`.

### Phase 5: Shell Extraction

For tray and bag, extract only surface voxels using 6-probe method:

```python
T = SHELL_THICKNESS
probes = [(x-T,y,z),(x+T,y,z),(x,y-T,z),(x,y+T,z),(x,y,z-T),(x,y,z+T)]
is_surface = any(
    p out of bounds or mask[p] == 0
    for p in probes
)
```

Write shell voxels to volume at the object's density value.

### Phase 6: Internal Objects

Sort by density descending. For each object, sample its bounding box and fill voxels where `volume[idx] < density` and point is inside mesh.

## Axis Convention (Security CT / Tray-on-Belt)

The voxel grid axes are locked to the screening tray on a conveyor belt:

```
X = belt direction / tray long side (660mm) → Width  (256 voxels) → SLICE AXIS
Y = across belt / tray short side (420mm)   → Height (256 voxels)
Z = up/down (gravity)                       → Depth  (128 voxels, Z=0 at tray floor)
```

The CT gantry rotates around the belt axis (X). Native CT slices are perpendicular to belt motion.

### Three Standard Views (Security CT Convention)

| View | Plane | Slice Through | Horizontal | Vertical | Notes |
|------|-------|--------------|-----------|----------|-------|
| **Axial** (native) | YZ at X | Along belt (X) | Tray short side (Y) | Height (Z), tray at bottom | Native CT slice, perpendicular to belt |
| **Sagittal** | XZ at Y | Across belt (Y) | Tray long side (X) | Height (Z), tray at bottom | Side view along belt |
| **Coronal** | XY at Z | Top-down (Z) | Tray long side (X) | Tray short side (Y) | Bird's eye view of tray |

**Axial is the native frame format.** Each DICOS frame is one YZ slice at a belt position X. Sagittal and coronal are reconstructed from the volume.

For axial and sagittal views, Z is flipped so row 0 = top of scene and the last row = tray floor. This puts the tray at the bottom of the displayed image.

### DICOM Orientation Tags

```
ImageOrientationPatient = [0,0,-1, 0,1,0]  — rows along -Z (tray at bottom), columns along Y
ImagePositionPatient    = [originX, originY, originZ]
PixelSpacing            = [spacingZ, spacingY]  — [row_spacing=Z, col_spacing=Y]
SliceThickness          = spacingX              — slices along belt (X)
SpacingBetweenSlices    = spacingX
NumberOfFrames          = Width (X dimension)
Rows                    = Depth (Z dimension)
Columns                 = Height (Y dimension)
```

## Output Format

Raw binary file:

| Offset | Type | Field |
|--------|------|-------|
| 0 | uint32 x3 | width (X), height (Y), depth (Z) |
| 12 | float64 x3 | spacingX, spacingY, spacingZ (mm) |
| 36 | float64 x3 | originX, originY, originZ (mm) |
| 60 | uint16[W*H*D] | voxel data (Z-major: `idx = z*W*H + y*W + x`) |

## Performance Notes

- Full 256x256x128 voxelization takes ~20s in Blender Python
- The shell extraction (Phase 5) is the bottleneck for large objects like CarryOnBag
- 6-probe method is O(6) per voxel vs O(N^3) for full neighborhood check
- Keep all phases in a single `execute_blender_code` call to preserve variables
