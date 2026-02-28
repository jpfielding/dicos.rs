# DICOS Export

Convert voxelized Blender scene to DICOS CT Image and TDR files.

## Overview

The export produces up to two DICOS files:

1. **CT Image** (`.dcs`) - Multi-frame volumetric image, one axial slice per frame
2. **TDR** (`.dcs`) - Threat Detection Report with bounding boxes, references the CT Image

## Step 1: Write Raw Voxels from Blender

The Blender voxelizer writes a raw binary file (see [voxelization.md](voxelization.md) for format). It also writes a JSON sidecar with threat metadata if threats are present.

Save both to the project `tmp/` directory:
```python
# In Blender Python, at end of voxelization:
import json

# Save raw volume
with open('tmp/voxels.raw', 'wb') as f:
    f.write(struct.pack('<III', RES_X, RES_Y, RES_Z))
    f.write(struct.pack('<ddd', spacing_x_mm, spacing_y_mm, spacing_z_mm))
    f.write(struct.pack('<ddd', origin_x_mm, origin_y_mm, origin_z_mm))
    volume.tofile(f)

# Save threat metadata (if threats present)
threats = []
for name, info in threat_objects.items():
    obj_parts = info['objects']
    all_corners = []
    for obj_name in obj_parts:
        obj = bpy.data.objects[obj_name]
        corners = [obj.matrix_world @ Vector(v) for v in obj.bound_box]
        all_corners.extend(corners)
    threats.append({
        'label': info['label'],
        'category': info['category'],       # DICOS Threat Category
        'flag': info['flag'],               # Assessment Flag
        'probability': info['probability'],
        'bbox_mm': {
            'min': [min(c[i] for c in all_corners) * 1000 for i in range(3)],
            'max': [max(c[i] for c in all_corners) * 1000 for i in range(3)],
        }
    })

with open('tmp/threats.json', 'w') as f:
    json.dump({'threats': threats}, f, indent=2)
```

## Step 2: Build CT Image with Go

Use `cmd/voxel2dicos` or write directly with the `pkg/dicos` library.

### Using cmd/voxel2dicos

```bash
go run ./cmd/voxel2dicos/ tmp/voxels.raw tmp/bag_ct.dcs
```

### Using pkg/dicos Directly

```go
import (
    "encoding/binary"
    "os"
    "github.com/jpfielding/dicos.go/pkg/dicos"
)

// 1. Read raw volume header
f, _ := os.Open("tmp/voxels.raw")
var width, height, depth uint32
binary.Read(f, binary.LittleEndian, &width)
binary.Read(f, binary.LittleEndian, &height)
binary.Read(f, binary.LittleEndian, &depth)

var spacingX, spacingY, spacingZ float64
binary.Read(f, binary.LittleEndian, &spacingX)
binary.Read(f, binary.LittleEndian, &spacingY)
binary.Read(f, binary.LittleEndian, &spacingZ)

var originX, originY, originZ float64
binary.Read(f, binary.LittleEndian, &originX)
binary.Read(f, binary.LittleEndian, &originY)
binary.Read(f, binary.LittleEndian, &originZ)

voxels := make([]uint16, width*height*depth)
binary.Read(f, binary.LittleEndian, voxels)

// 2. Build CT Image
ct := dicos.NewCTImage()
ct.Patient.SetPatientName("Simulated", "Bag", "", "", "")
ct.Patient.PatientID = "SIM-001"
ct.Series.Modality = "CT"
ct.Series.SeriesDescription = "Simulated CT scan"
ct.Equipment.Manufacturer = "dicos.go Blender Voxelizer"

// Scanner parameters (simulated Smiths HI-SCAN 6040 CTiX)
ct.CTImageMod.KVP = 140
ct.CTImageMod.ExposureTime = 500
ct.CTImageMod.XRayTubeCurrent = 300
ct.CTImageMod.ConvolutionKernel = "STANDARD"
ct.CTImageMod.AcquisitionType = "SPIRAL"
ct.CTImageMod.DataCollectionDiameter = 620
ct.CTImageMod.ReconstructionDiameter = 640
ct.CTImageMod.ImageType = []string{"ORIGINAL", "PRIMARY", "AXIAL"}

// Rescale to approximate Hounsfield Units
ct.RescaleIntercept = -1024.0
ct.RescaleSlope = 1.0
ct.RescaleType = "HU"

// Window/Level for viewing
ct.CTImageMod.WindowCenter = 2000
ct.CTImageMod.WindowWidth = 6000

// Image Plane geometry
ct.ImagePlane.PixelSpacing = [2]float64{spacingY, spacingX}
ct.ImagePlane.SliceThickness = spacingZ
ct.ImagePlane.SpacingBetweenSlices = spacingZ
ct.ImagePlane.ImageOrientationPatient = [6]float64{1, 0, 0, 0, 1, 0}
ct.ImagePlane.ImagePositionPatient = [3]float64{originX, originY, originZ}

// Frame of Reference
ct.FrameOfReference.FrameOfReferenceUID = dicos.GenerateUID("1.2.826.0.1.3680043.8.498.")

// Image dimensions
ct.Rows = int(height)
ct.Columns = int(width)
ct.BitsAllocated = 16
ct.BitsStored = 16
ct.HighBit = 15
ct.PixelRepresent = 0 // unsigned
ct.SamplesPerPixel = 1
ct.PhotometricInterp = "MONOCHROME2"

// Set pixel data: all Z slices as frames
ct.SetPixelData(int(height), int(width), voxels)

// Write
ct.Write("tmp/bag_ct.dcs")
```

### Key CT Image Tags

| Tag | Value | Notes |
|-----|-------|-------|
| SOP Class UID | 1.2.840.10008.5.1.4.1.1.2 | CT Image Storage |
| Modality | CT | |
| Transfer Syntax | 1.2.840.10008.1.2.1 | Explicit VR Little Endian |
| Bits Allocated | 16 | uint16 pixel data |
| Pixel Representation | 0 | Unsigned |
| Photometric Interpretation | MONOCHROME2 | Higher values = brighter |
| Rescale Intercept | -1024 | Maps stored 0 → HU -1024 (air) |
| Rescale Slope | 1.0 | |
| Number of Frames | depth | One frame per axial slice |

### Viewing Planes

The multi-frame CT volume supports reconstruction of all three standard views:
- **Axial** (XY plane): native frame format, one per Z-level
- **Coronal** (XZ plane): reconstructed from Y slices across frames
- **Sagittal** (YZ plane): reconstructed from X slices across frames

Use `pkg/dicos.Volume.Slice(orientation, index)` to extract any plane.

## Step 3: Build TDR (If Threats Present)

Read the threat JSON sidecar and create a TDR that references the CT image.

```go
import "encoding/json"

// Read threats
data, _ := os.ReadFile("tmp/threats.json")
var meta struct {
    Threats []struct {
        Label       string  `json:"label"`
        Category    string  `json:"category"`
        Flag        string  `json:"flag"`
        Probability float64 `json:"probability"`
        BBoxMM      struct {
            Min [3]float64 `json:"min"`
            Max [3]float64 `json:"max"`
        } `json:"bbox_mm"`
    } `json:"threats"`
}
json.Unmarshal(data, &meta)

// Build TDR
tdr := dicos.NewThreatDetectionReport()
tdr.AlarmDecision = "ALARM"
tdr.Series.Modality = "TDR"
tdr.Equipment.Manufacturer = "dicos.go Blender Voxelizer"

// Link to source CT image
tdr.ReferencedSOPClassUID = "1.2.840.10008.5.1.4.1.1.2"
tdr.ReferencedSOPInstanceUID = ct.SOPCommon.SOPInstanceUID

// Add each PTO
for i, t := range meta.Threats {
    // Convert bbox from absolute mm to volume-relative mm
    bbMin := [3]float32{
        float32(t.BBoxMM.Min[0] - originX),
        float32(t.BBoxMM.Min[1] - originY),
        float32(t.BBoxMM.Min[2] - originZ),
    }
    bbMax := [3]float32{
        float32(t.BBoxMM.Max[0] - originX),
        float32(t.BBoxMM.Max[1] - originY),
        float32(t.BBoxMM.Max[2] - originZ),
    }

    tdr.PTOs = append(tdr.PTOs, dicos.PotentialThreatObject{
        ID:          i + 1,
        Label:       t.Label,
        OOIType:     t.Category,       // DICOS Threat Category
        Probability: float32(t.Probability),
        Confidence:  float32(t.Probability),
        BoundingBox: &dicos.BoundingBox{
            TopLeft:     bbMin,
            BottomRight: bbMax,
        },
    })
}

tdr.Write("tmp/bag_ct_tdr.dcs")
```

### Key TDR Tags

| Tag | Value | Notes |
|-----|-------|-------|
| SOP Class UID | 1.2.840.10008.5.1.4.1.1.501.3 | DICOS TDR Storage |
| Modality | TDR | |
| Alarm Decision (4010,1031) | ALARM / NO_ALARM | Overall scan decision |
| PTO Sequence (4010,1010) | sequence | One item per threat |
| Threat Category (4010,1012) | METAL, EXPLOSIVE, etc. | NEMA Defined Terms |
| Assessment Flag (4010,1015) | HIGH_THREAT / THREAT | Per-PTO severity |
| Bounding Box Top Left (4010,1023) | [x,y,z] float32 mm | Min corner |
| Bounding Box Bottom Right (4010,1024) | [x,y,z] float32 mm | Max corner |
| Referenced SOP Instance UID (0008,1155) | UID string | Links to source CT |

## Output Directory

The converter writes all DICOS files into a single output directory:

```bash
go run ./scripts/voxel2dicos/ tmp/voxels.raw tmp/dicos/
```

```
tmp/
├── voxels.raw         # Intermediate: raw uint16 volume + header
├── threats.json       # Intermediate: threat bounding boxes (sidecar)
└── dicos/             # Final output directory
    ├── ct.dcs         # DICOS CT Image (multi-frame volume)
    └── tdr.dcs        # DICOS TDR (only if threats present)
```

All DICOS files for a single scan belong in the same directory. The TDR references the CT by SOP Instance UID.
