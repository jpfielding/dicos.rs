# TDR (Threat Detection Report) Workflow

How to generate a DICOS Threat Detection Report with bounding boxes for threat items in a CT scan volume.

## DICOS TDR Overview

Per NEMA IIC 1 v04-2023, a TDR is a separate DICOS object (SOP Class 1.2.840.10008.5.1.4.1.1.501.3) that **references** the source CT Image and describes detected threats. It does NOT contain pixel data - only metadata and spatial coordinates.

### Key DICOS Terms

| Term | Tag | Description |
|------|-----|-------------|
| Alarm Decision | (4010,1031) | `ALARM`, `NO_ALARM`, `UNKNOWN` |
| TDR Type | (4010,1027) | `MACHINE` (automated/ATR), `OPERATOR` (human) |
| PTO Sequence | (4010,1010) | Sequence of Potential Threat Objects |
| Potential Threat Object (PTO) | - | A detected threat with spatial location |
| Threat Category | (4010,1012) | Defined Terms: `ANOMALY`, `CONTRABAND`, `EXPLOSIVE`, `LAPTOP`, `PHARMACEUTICAL`, `PI`, `METAL`, `NONMETAL`, `UNKNOWN`, `OTHER` |
| Threat Category Description | (4010,1013) | Free-text description of the threat |
| Assessment Flag | (4010,1015) | `HIGH_THREAT`, `THREAT`, `NO_THREAT`, `UNKNOWN` |
| Assessment Probability | (4010,1016) | 0.0-1.0 certainty |
| Bounding Box Top Left | (4010,1023) | [x, y, z] in mm, volume coordinates |
| Bounding Box Bottom Right | (4010,1024) | [x, y, z] in mm, volume coordinates |
| Referenced SOP Instance UID | (0008,1155) | Links TDR to source CT Image |

### Threat Category Mapping

Map TSA prohibited item types to DICOS Threat Categories:

| TSA Prohibited Category | DICOS Threat Category | Notes |
|------------------------|----------------------|-------|
| Firearms, ammunition, replicas | `METAL` | Dense metal signature |
| Knives, sharp objects, blades | `METAL` | Thin flat metal |
| Explosives, IED components | `EXPLOSIVE` | Organic density + wires |
| Liquid explosives | `EXPLOSIVE` | Liquid density anomaly |
| Oversized liquids (3-1-1 violation) | `CONTRABAND` | Volume exceeds limit |
| Tools (> 7"), sporting goods | `CONTRABAND` | Prohibited shape/size |
| Stun guns, pepper spray | `CONTRABAND` | Electronics + chemical |
| Suspicious electronics | `LAPTOP` | Modified/anomalous internals |
| Unknown anomaly | `ANOMALY` | Unusual density or configuration |
| Drugs, pharmaceuticals | `PHARMACEUTICAL` | Organic powder/crystal |

## Workflow: Blender Scene to TDR

### Step 1: Track Threat Objects During Scene Creation

When placing threat items in the Blender scene, record their world-space bounding boxes:

```python
threat_objects = []

# After placing a knife, for example:
knife_parts = ['CT_Knife_Blade', 'CT_Knife_Handle', 'CT_Knife_Tang']
all_corners = []
for name in knife_parts:
    obj = bpy.data.objects[name]
    corners = [obj.matrix_world @ Vector(v) for v in obj.bound_box]
    all_corners.extend(corners)

threat_objects.append({
    'label': 'Kitchen knife',
    'category': 'METAL',           # DICOS Threat Category
    'flag': 'HIGH_THREAT',         # DICOS Assessment Flag
    'probability': 0.95,
    'bb_min': [min(c[i] for c in all_corners) for i in range(3)],
    'bb_max': [max(c[i] for c in all_corners) for i in range(3)],
})
```

### Step 2: Convert Bounding Box to Volume Coordinates

After voxelization, convert Blender world-space (meters) to DICOS volume coordinates (mm):

```python
# Blender meters → DICOS mm
bb_min_mm = [b * 1000 for b in bb_min]  # x, y, z in mm
bb_max_mm = [b * 1000 for b in bb_max]

# Adjust relative to volume origin
bb_min_vol = [bb_min_mm[i] - origin_mm[i] for i in range(3)]
bb_max_vol = [bb_max_mm[i] - origin_mm[i] for i in range(3)]
```

### Step 3: Write TDR Using dicos.go Library

```go
tdr := dicos.NewThreatDetectionReport()

// Link to source CT image
tdr.ReferencedSOPClassUID = "1.2.840.10008.5.1.4.1.1.2"  // CT Image Storage
tdr.ReferencedSOPInstanceUID = ctImage.SOPCommon.SOPInstanceUID

tdr.AlarmDecision = "ALARM"
tdr.Series.Modality = "TDR"

// Add each PTO
tdr.PTOs = append(tdr.PTOs, dicos.PotentialThreatObject{
    ID:          1,
    Label:       "Kitchen knife",
    OOIType:     "METAL",           // DICOS Threat Category
    Probability: 0.95,
    Confidence:  0.92,
    BoundingBox: &dicos.BoundingBox{
        TopLeft:     [3]float32{x_min, y_min, z_min},  // mm
        BottomRight: [3]float32{x_max, y_max, z_max},  // mm
    },
})

tdr.Write("tmp/bag_ct_tdr.dcs")
```

### Step 4: Export Threat Metadata Alongside Volume

When exporting the raw voxel data, also write a JSON sidecar with threat bounding boxes. This allows the Go voxel2dicos tool to generate both the CT image and the TDR:

```json
{
  "threats": [
    {
      "label": "Kitchen knife",
      "category": "METAL",
      "flag": "HIGH_THREAT",
      "probability": 0.95,
      "blender_objects": ["CT_Knife_Blade", "CT_Knife_Handle", "CT_Knife_Tang"],
      "bbox_mm": {
        "min": [-50.0, -20.0, 130.0],
        "max": [50.0, 20.0, 145.0]
      }
    }
  ]
}
```

## Bounding Box Conventions

- Coordinates are in **millimeters** relative to the volume origin
- TopLeft = minimum corner (smallest x, y, z)
- BottomRight = maximum corner (largest x, y, z)
- The box should tightly enclose all parts of the threat item
- For multi-part items (e.g., disassembled firearm), use the composite bounding box that covers all parts
- Pad the box by 2-5mm on each side for realistic detection margins

## Multiple Threats

A single scan may contain multiple PTOs. Each gets its own entry in the PTO Sequence with a unique PTO ID. The Alarm Decision applies to the overall scan - if ANY PTO is flagged, the scan decision is `ALARM`.
