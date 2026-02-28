// voxel2dicos reads a raw voxel volume and optional threats.json exported
// from Blender and writes DICOS files into an output directory:
//   - ct.dcs  — multi-frame CT Image volume (one axial slice per frame)
//   - tdr.dcs — Threat Detection Report with PTO bounding boxes (if threats present)
//
// Usage:
//
//	go run . <raw_input> <output_dir>
//	go run . tmp/voxels.raw tmp/dicos/
package main

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"log"
	"math"
	"os"
	"path/filepath"

	"github.com/jpfielding/dicos.go/pkg/dicos"
)

type rawHeader struct {
	Width, Height, Depth         uint32
	SpacingX, SpacingY, SpacingZ float64
	OriginX, OriginY, OriginZ   float64
}

type threatFile struct {
	Threats []threatEntry `json:"threats"`
}

type threatEntry struct {
	Label       string  `json:"label"`
	Category    string  `json:"category"`
	Flag        string  `json:"flag"`
	Probability float64 `json:"probability"`
	BBoxMM      struct {
		Min [3]float64 `json:"min"`
		Max [3]float64 `json:"max"`
	} `json:"bbox_mm"`
}

func main() {
	rawPath := "tmp/voxels.raw"
	outDir := "tmp/dicos"

	if len(os.Args) > 1 {
		rawPath = os.Args[1]
	}
	if len(os.Args) > 2 {
		outDir = os.Args[2]
	}

	// Create output directory
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		log.Fatalf("create output dir: %v", err)
	}

	// Derive threats.json path from raw path directory
	threatsPath := filepath.Join(filepath.Dir(rawPath), "threats.json")

	// --- Read raw volume ---
	f, err := os.Open(rawPath)
	if err != nil {
		log.Fatalf("open raw: %v", err)
	}
	defer f.Close()

	var hdr rawHeader
	if err := binary.Read(f, binary.LittleEndian, &hdr); err != nil {
		log.Fatalf("read header: %v", err)
	}

	width := int(hdr.Width)
	height := int(hdr.Height)
	depth := int(hdr.Depth)
	totalVoxels := width * height * depth

	fmt.Printf("Volume: %dx%dx%d (%d voxels)\n", width, height, depth, totalVoxels)
	fmt.Printf("Spacing: %.2f x %.2f x %.2f mm\n", hdr.SpacingX, hdr.SpacingY, hdr.SpacingZ)
	fmt.Printf("Origin:  %.1f, %.1f, %.1f mm\n", hdr.OriginX, hdr.OriginY, hdr.OriginZ)

	voxels := make([]uint16, totalVoxels)
	if err := binary.Read(f, binary.LittleEndian, voxels); err != nil {
		log.Fatalf("read voxels: %v", err)
	}

	var vmin, vmax uint16 = math.MaxUint16, 0
	var nonzero int
	for _, v := range voxels {
		if v > 0 {
			nonzero++
			if v < vmin {
				vmin = v
			}
			if v > vmax {
				vmax = v
			}
		}
	}
	fmt.Printf("Non-zero: %d (%.1f%%), range: [%d, %d]\n", nonzero, 100*float64(nonzero)/float64(totalVoxels), vmin, vmax)

	// --- Build DICOS CT Image ---
	ct := dicos.NewCTImage()

	ct.Patient.SetPatientName("BlenderSim", "Screening", "", "", "")
	ct.Patient.PatientID = "BAG-CT-001"
	ct.Series.Modality = "CT"
	ct.Series.SeriesDescription = "Simulated CT scan — airport screening tray"
	ct.Series.SeriesNumber = 1
	ct.Study.StudyDescription = "Airport Security Screening Simulation"
	ct.Equipment.Manufacturer = "dicos.go Blender Voxelizer"
	ct.Equipment.StationName = "BLENDER-SIM"
	ct.Equipment.InstitutionName = "DICOS.go Project"

	ct.CTImageMod.KVP = 140
	ct.CTImageMod.ExposureTime = 500
	ct.CTImageMod.XRayTubeCurrent = 300
	ct.CTImageMod.FilterType = "BODY"
	ct.CTImageMod.ConvolutionKernel = "STANDARD"
	ct.CTImageMod.AcquisitionType = "SPIRAL"
	ct.CTImageMod.DataCollectionDiameter = 620
	ct.CTImageMod.ReconstructionDiameter = 640
	ct.CTImageMod.ImageType = []string{"ORIGINAL", "PRIMARY", "AXIAL"}

	ct.RescaleIntercept = -1024.0
	ct.RescaleSlope = 1.0
	ct.RescaleType = "HU"

	ct.CTImageMod.WindowCenter = 1500
	ct.CTImageMod.WindowWidth = 4000

	// Security CT orientation: belt runs along X (tray long axis).
	// Native CT slices are axial = YZ plane, perpendicular to belt.
	// Frames: one per X position (along belt). Rows=Z (height), Columns=Y (tray width).
	// Slice direction: along X (SpacingX between slices).
	//
	// ImageOrientationPatient: row direction=Z(up), col direction=Y(across belt)
	// For axial slices with Z flipped (tray at bottom = last row):
	//   Row direction = [0, 0, -1] (Z decreasing = tray at bottom of image)
	//   Col direction = [0, 1,  0] (Y across belt)
	ct.ImagePlane.PixelSpacing = [2]float64{hdr.SpacingZ, hdr.SpacingY} // row=Z, col=Y
	ct.ImagePlane.SliceThickness = hdr.SpacingX                          // slices along X (belt)
	ct.ImagePlane.SpacingBetweenSlices = hdr.SpacingX
	ct.ImagePlane.ImageOrientationPatient = [6]float64{0, 0, -1, 0, 1, 0}
	ct.ImagePlane.ImagePositionPatient = [3]float64{hdr.OriginX, hdr.OriginY, hdr.OriginZ}

	ct.FrameOfReference.FrameOfReferenceUID = dicos.GenerateUID("1.2.826.0.1.3680043.8.498.")
	ct.FrameOfReference.PositionReferenceIndicator = "BB"

	// Frame dimensions: Rows=Z (depth), Columns=Y (height)
	// Number of frames = X (width) — one axial slice per belt position
	frameRows := depth    // Z dimension
	frameCols := height   // Y dimension
	numFrames := width    // X dimension (along belt)

	ct.Rows = frameRows
	ct.Columns = frameCols
	ct.BitsAllocated = 16
	ct.BitsStored = 16
	ct.HighBit = 15
	ct.PixelRepresent = 0
	ct.SamplesPerPixel = 1
	ct.PhotometricInterp = "MONOCHROME2"

	// Reorganize voxels: from [z][y][x] storage to axial frames [x][z_flipped][y]
	axialVoxels := make([]uint16, len(voxels))
	for x := 0; x < width; x++ {
		for row := 0; row < depth; row++ {
			z := depth - 1 - row // flip Z so tray (z=0) is at bottom of image
			for y := 0; y < height; y++ {
				srcIdx := z*width*height + y*width + x
				dstIdx := x*depth*height + row*height + y
				axialVoxels[dstIdx] = voxels[srcIdx]
			}
		}
	}

	ct.SetPixelData(frameRows, frameCols, axialVoxels)

	ctPath := filepath.Join(outDir, "ct.dcs")
	n, err := ct.Write(ctPath)
	if err != nil {
		log.Fatalf("write DICOS CT: %v", err)
	}

	fmt.Printf("\n%s (%d bytes, %.1f MB)\n", ctPath, n, float64(n)/1024/1024)
	fmt.Printf("  %d axial frames (along belt), %dx%d per frame (Z x Y)\n", numFrames, frameRows, frameCols)
	fmt.Printf("  Pixel spacing: %.2f x %.2f mm, slice spacing: %.2f mm\n", hdr.SpacingZ, hdr.SpacingY, hdr.SpacingX)
	fmt.Printf("  Axial=native(YZ), Sagittal=reconstructed(XZ), Coronal=reconstructed(XY)\n")

	// --- Read threats and build TDR ---
	threatData, err := os.ReadFile(threatsPath)
	if err != nil {
		fmt.Printf("\nNo threats — TDR skipped\n")
		return
	}

	var tf threatFile
	if err := json.Unmarshal(threatData, &tf); err != nil {
		log.Fatalf("parse threats.json: %v", err)
	}

	if len(tf.Threats) == 0 {
		fmt.Printf("\nNo threats — TDR skipped\n")
		return
	}

	tdr := dicos.NewThreatDetectionReport()
	tdr.AlarmDecision = "ALARM"
	tdr.Series.Modality = "TDR"
	tdr.Equipment.Manufacturer = "dicos.go Blender Voxelizer"
	tdr.ReferencedSOPClassUID = "1.2.840.10008.5.1.4.1.1.2"
	tdr.ReferencedSOPInstanceUID = ct.SOPCommon.SOPInstanceUID

	for i, t := range tf.Threats {
		bbMin := [3]float32{
			float32(t.BBoxMM.Min[0] - hdr.OriginX),
			float32(t.BBoxMM.Min[1] - hdr.OriginY),
			float32(t.BBoxMM.Min[2] - hdr.OriginZ),
		}
		bbMax := [3]float32{
			float32(t.BBoxMM.Max[0] - hdr.OriginX),
			float32(t.BBoxMM.Max[1] - hdr.OriginY),
			float32(t.BBoxMM.Max[2] - hdr.OriginZ),
		}

		tdr.PTOs = append(tdr.PTOs, dicos.PotentialThreatObject{
			ID:          i + 1,
			Label:       t.Label,
			OOIType:     t.Category,
			Probability: float32(t.Probability),
			Confidence:  float32(t.Probability),
			BoundingBox: &dicos.BoundingBox{
				TopLeft:     bbMin,
				BottomRight: bbMax,
			},
		})

		fmt.Printf("  PTO %d: %s [%s] prob=%.2f bbox=[%.0f,%.0f,%.0f]-[%.0f,%.0f,%.0f]mm\n",
			i+1, t.Label, t.Category, t.Probability,
			bbMin[0], bbMin[1], bbMin[2], bbMax[0], bbMax[1], bbMax[2])
	}

	tdrPath := filepath.Join(outDir, "tdr.dcs")
	tn, err := tdr.Write(tdrPath)
	if err != nil {
		log.Fatalf("write DICOS TDR: %v", err)
	}

	fmt.Printf("%s (%d bytes) — ALARM, %d PTOs\n", tdrPath, tn, len(tdr.PTOs))
}
