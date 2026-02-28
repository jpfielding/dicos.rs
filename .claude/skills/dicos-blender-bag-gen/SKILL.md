---
name: dicos-blender-bag-gen
description: >
  Generate 3D CT scan visualizations of bags and personal items in airport
  screening trays using Blender MCP. Supports varied container types (carry-on
  suitcases, backpacks, purses, laptop bags, duffel bags) and loose tray items
  (watches, phones, laptops, belts, shoes). Creates realistic randomized packing
  with CT density-based materials, and optionally voxelizes to raw volume data
  for DICOS export. Use when the user asks to: create a bag scan, generate a CT
  bag, build a screening scene, make a bag in Blender, simulate an airport
  X-ray/CT scan, add items to a bag, voxelize a Blender scene, or generate
  screening training data. Requires Blender MCP connection.
---

# CT Bag Generator

Generate realistic CT scan visualizations of bags and personal items in TSA screening trays via Blender MCP.

## Defaults

Use these unless the user requests otherwise. Confirm before changing.

| Parameter | Default | Notes |
|-----------|---------|-------|
| Tray type | Standard flat | 660 x 420 x 75mm |
| Voxel grid | **256 x 256 x 128** | Medium quality (default) |
| Blender units | Meters | 1 unit = 1 meter |
| Output format | DICOS CT + TDR | In a named directory under `tmp/` |
| Bag shell density | 2024 stored (+1000 HU) | Apparent density for visibility |
| Tray density | 2524 stored (+1500 HU) | Apparent density for visibility |
| Rescale Intercept | -1024 | stored 0 = -1024 HU (air) |

### Voxel Grid Sizes

The grid determines slice dimensions (Width x Height) and slice count (Depth). Larger grids produce finer detail but take longer to voxelize and create bigger files.

| Preset | Width x Height x Depth | Slice Size | Spacing (mm) | File Size | Voxelize Time | Use |
|--------|----------------------|------------|--------------|-----------|---------------|-----|
| **Draft** | 128 x 128 x 64 | 128x128 | ~5.2 x 3.4 x 3.1 | ~2 MB | ~3s | Quick preview |
| **Medium** (default) | 256 x 256 x 128 | 256x256 | ~2.6 x 1.7 x 1.6 | ~16 MB | ~15s | Standard quality |
| **High** | 512 x 512 x 256 | 512x512 | ~1.3 x 0.8 x 0.8 | ~128 MB | ~2min | Detailed analysis |
| **Scanner-match** | 512 x 512 x 375 | 512x512 | ~1.3 x 0.8 x 0.5 | ~192 MB | ~3min | Matches HI-SCAN 6040 CTiX |

Spacing is calculated from: `spacing = scene_extent / grid_dimension`. The scene extent is determined by the tray size plus a small padding margin.

### TSA-Approved Checkpoint CT Scanner Reference

These are real scanners deployed at TSA checkpoints. Image parameters are approximate (vendors don't publish exact specs publicly).

| Scanner | Manufacturer | Tunnel (W x H) | Recon Matrix | Slices | Voxel (mm) | Belt Speed |
|---------|-------------|-----------------|-------------|--------|------------|------------|
| HI-SCAN 6040 CTiX | Smiths Detection | 620 x 420mm | 512 x 512 | 300-500 | ~1.2 x 1.2 x 0.5 | 0.2 m/s |
| ConneCT | Analogic | 620 x 420mm | 512 x 512 | 300-400 | ~1.0 x 1.0 x 0.5 | 0.2 m/s |
| RTT 110 | Rapiscan Systems | 620 x 420mm | 512 x 512 | 350-500 | ~1.2 x 1.2 x 0.6 | 0.2 m/s |
| ClearScan | — | 620 x 420mm | 312 x 312 | 315 | ~2.0 x 2.0 x 0.6 | 0.2 m/s |
| ICTS CT | IDSS (Integrated Defense) | 620 x 420mm | 512 x 512 | 300-400 | ~1.0 x 1.0 x 0.5 | 0.15 m/s |

**Common characteristics across all TSA checkpoint CT scanners:**
- Tunnel opening: ~620mm W x 420mm H (standardized for tray compatibility)
- Dual-energy capable: 80kV / 140kV (for material discrimination)
- Spiral/helical acquisition
- 16-bit pixel data (uint16), MONOCHROME2
- DICOS CT Image output (SOP Class 1.2.840.10008.5.1.4.1.1.2)
- DICOS TDR output for automated threat detection
- Belt speed: ~0.2 m/s (tray passes through in ~3 seconds)

When the user asks for a specific scanner model, use its parameters. When they ask for "realistic" or "production quality", use Scanner-match (512x512x375).

## Prerequisites

Requires [Blender MCP](https://github.com/ahujasid/blender-mcp) - a Model Context Protocol server that connects Claude to Blender. See [references/blender-mcp-setup.md](references/blender-mcp-setup.md) for full install instructions (download addon.py, install uv, configure `~/.claude.json`, activate in Blender).

All scene creation uses `mcp__blender__execute_blender_code` and `mcp__blender__get_viewport_screenshot`.

## Workflow

### 1. Choose a Scenario

Pick or randomize a traveler profile. Each produces different bag types and contents.

| Profile | Container | Typical Contents |
|---------|-----------|-----------------|
| Business trip | Carry-on roller + laptop bag | Suits, dress shirts, laptop, tablet, chargers, toiletries |
| Vacation | Large carry-on roller | Casual clothes, swimwear, sunscreen, snacks, camera |
| Weekend getaway | Backpack or duffel | 1-2 changes, toiletries, phone charger, book |
| Student | Backpack | Laptop, textbooks, cables, snacks, water bottle |
| Parent w/ child | Duffel or tote | Snacks, toys, diapers, wipes, change of clothes, tablet |
| Commuter | Messenger/laptop bag | Laptop, notebook, lunch, water bottle, keys, wallet |

A second tray for loose personal items is common: watch, belt, phone, wallet, shoes.

### 2. Create Screening Tray

Default: **standard flat tray** (660 x 420 x 75mm). See [references/tray-specs.md](references/tray-specs.md) for all sizes and geometry.

**The tray is solid. Nothing may overlap or intersect with it.** This is the most important constraint in the entire workflow.

- Items sit on tray floor at Z=0.005m (4mm floor + 1mm gap)
- All items must stay within tray interior XY bounds (±320mm X, ±200mm Y) with 5mm clearance
- Items may extend ABOVE the tray rim — that is normal
- **No stacking bags on bags.** If multiple containers don't fit side-by-side, downsize them until they do
- Gaps between items in the tray are fine
- During voxelization, tray voxels are never overwritten

**Gravity rule:** Every object must sit on something — the tray floor or another object's top surface. Nothing floats. Items inside bags rest on the bag floor or on items below them. Track running Z height as you pack bottom-up.

After every placement step, validate bounding boxes against the tray interior.

### 3. Create Container

See [references/container-types.md](references/container-types.md) for geometry and hardware per container type.

Containers are **hollow shells** in the CT volume (2-voxel thick). The shell density (3000) makes the bag outline visible; interior is filled with contents or air.

### 4. Pack Contents with Randomness

See [references/density-catalog.md](references/density-catalog.md) for the full item catalog.

**Randomization rules** - simulate how real people pack:
- Vary item count: use `random.randint(min, max)` for each category
- Offset positions: add `random.uniform(-0.02, 0.02)` jitter to X/Y placement
- Rotate items: add `random.uniform(-15, 15)` degree tilt to folded clothes
- Skip categories: not every bag has shoes, food, or a hoodie
- Mix folding styles: some items rolled, some folded flat, some crumpled (scaled unevenly)
- Overlap slightly: real packed clothes compress against each other
- Stuff gaps: socks and underwear get tucked into corners and shoe cavities

**Packing is bottom-up.** Track a running Z height as items stack. Heavier/flatter items at the bottom, bulky/light items on top. Electronics go in designated pockets (back of bag for laptops, front pocket for tablets/phones).

### 5. Add Loose Tray Items (Optional)

Items that go directly in the tray, NOT inside a bag:

| Item | Geometry | Density | Notes |
|------|----------|---------|-------|
| Watch (metal band) | Torus major=0.02 minor=0.004 | 15000 | Flat on tray floor |
| Watch (leather band) | Torus major=0.02 minor=0.004 | 1800 band / 7000 face | |
| Phone | Cube 0.003x0.035x0.075 | 7000 + 15000 battery | Flat on tray |
| Laptop (removed) | Cube 0.01x0.17x0.12 | 7000 + 15000 battery | TSA requires separate |
| Tablet | Cube 0.006x0.12x0.085 | 7000 + 15000 battery | |
| Belt | Cylinder r=0.008, coiled torus | 1800 leather / 15000 buckle | |
| Shoes (removed) | See catalog | 1200 + 1800 sole | Pair side by side |
| Wallet | Cube 0.015x0.055x0.045 | 1800 leather / 15000 cards | |
| Sunglasses | Curved cube | 3500 lens / 15000 hinge | |
| Baseball cap | Hemisphere r=0.07 | 1200 | With 15000 metal button |

Place loose items with random rotation on the tray floor. They must not overlap each other or the bag.

### 6. Constrain All Items

After placing everything:
1. Clamp bag contents inside bag bounds (8mm inset margin)
2. Clamp loose tray items inside tray bounds (8mm inset margin)
3. Verify NO object penetrates the tray shell from below
4. Verify bag shell does not overlap tray shell

### 7. Apply CT Materials

See density-catalog.md for the full material table. Principled BSDF + emission per tier. Bag shell and tray are **opaque** (alpha=1.0).

### 8. Add Threat Items (Optional)

See [references/threat-items.md](references/threat-items.md) for the full TSA prohibited items catalog with Blender geometry and CT signatures.

Threat items use DICOS Threat Categories per NEMA IIC 1 v04-2023: `METAL` (firearms, knives), `EXPLOSIVE` (IEDs, detonators), `CONTRABAND` (tools, weapons, sporting goods), `ANOMALY` (suspicious configurations).

Threat items are placed inside bags like any other content during Blender scene creation. No special handling is needed during the Blender sketching phase — threat bounding boxes are computed automatically at DICOS export time (step 11).

### 9. Voxelize (Optional)

See [references/voxelization.md](references/voxelization.md).

Critical voxelization rule: **tray mask is built first**, then bag and all other objects exclude tray voxels. This prevents interleaving.

Also export a `threats.json` sidecar with bounding box coordinates for any threat items placed in step 8.

### 10. Export to DICOS (Optional)

See [references/dicos-export.md](references/dicos-export.md) for the complete conversion workflow with Go code examples.

Uses [github.com/jpfielding/dicos.go](https://github.com/jpfielding/dicos.go) to write:
- **CT Image** (`.dcs`): Multi-frame volume, one axial slice per frame, with ImagePlane geometry, scanner parameters, and HU rescale
- **TDR** (`.dcs`): Threat Detection Report with PTO bounding boxes referencing the CT Image, if threats are present

A bundled converter is included at [scripts/voxel2dicos/](scripts/voxel2dicos/). Copy it to the project and run:
```bash
go run ./scripts/voxel2dicos/ tmp/voxels.raw tmp/dicos/
```

The second argument is an **output directory**. The script creates:
```
tmp/dicos/
├── ct.dcs    — multi-frame CT Image
└── tdr.dcs   — Threat Detection Report (only if threats.json exists)
```

It reads the raw binary + threats.json sidecar, builds a DICOS CTImage and TDR with proper modules, and writes both into the output directory.

### 11. Generate TDR with Threat Boxes

**Always generate a TDR when the scene contains any TSA prohibited items.** This is not optional — if banned items are present, a TDR must be written.

See [references/tdr-workflow.md](references/tdr-workflow.md) for DICOS TDR specifics.

At DICOS export time, scan the Blender scene for any `CT_Knife_*`, `CT_Gun_*`, `CT_Ammo_*`, `CT_BoxCutter*`, `CT_Scissors*`, `CT_MultiTool*`, `CT_Firearm*`, `CT_PipeBomb*`, `CT_Detonator*`, `CT_Dynamite*`, `CT_PlasticExplosive*`, or other threat-prefixed objects. For each threat item:

1. Compute its composite world-space bounding box (union of all parts)
2. Convert to volume-relative mm coordinates
3. Add a PTO entry with Threat Category, Assessment Flag (`HIGH_THREAT`), and bounding box

The TDR is a separate `.dcs` file that references the CT image. It contains one PTO per threat item, with Alarm Decision set to `ALARM`.

## Naming Convention

- Tray: `ScreeningTray`
- Container: `CarryOnBag`, `Backpack`, `Purse`, `DuffelBag`, `LaptopBag`, `MessengerBag`
- Bag contents: `CT_` prefix (e.g., `CT_Laptop`, `CT_Jeans_Folded`)
- Loose tray items: `Tray_` prefix (e.g., `Tray_Watch`, `Tray_Phone`, `Tray_Belt`)
- Hardware sub-parts: parent name + suffix (e.g., `CT_LaptopBattery`, `CT_HoodieZipper`)
