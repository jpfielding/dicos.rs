//! DICOS/DICOM Tag definitions.
//!
//! A Tag is a `(group, element)` pair that uniquely identifies a data element
//! within a DICOS dataset. Tags are organized by functional modules following
//! the NEMA DICOS and DICOM standards.

use std::fmt;

/// A DICOM/DICOS tag consisting of a group number and element number.
///
/// Tags are ordered first by group, then by element, matching the on-disk
/// ordering required by the DICOM file format.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag {
    pub group: u16,
    pub element: u16,
}

impl Tag {
    /// Creates a new Tag from group and element numbers.
    #[inline]
    pub const fn new(group: u16, element: u16) -> Self {
        Self { group, element }
    }

    /// Returns `true` if this is a private tag (odd group number).
    #[inline]
    pub fn is_private(self) -> bool {
        self.group % 2 == 1
    }

    /// Returns `true` if this tag is in the File Meta Information group (0002).
    #[inline]
    pub fn is_group_0002(self) -> bool {
        self.group == 0x0002
    }

    /// Returns a human-readable name for well-known tags, or an empty string
    /// for unrecognized tags.
    pub fn name(self) -> &'static str {
        match self {
            // File Meta Information
            FILE_META_INFORMATION_GROUP_LENGTH => "FileMetaInformationGroupLength",
            FILE_META_INFORMATION_VERSION => "FileMetaInformationVersion",
            MEDIA_STORAGE_SOP_CLASS_UID => "MediaStorageSOPClassUID",
            MEDIA_STORAGE_SOP_INSTANCE_UID => "MediaStorageSOPInstanceUID",
            TRANSFER_SYNTAX_UID => "TransferSyntaxUID",
            IMPLEMENTATION_CLASS_UID => "ImplementationClassUID",
            IMPLEMENTATION_VERSION_NAME => "ImplementationVersionName",
            SPECIFIC_CHARACTER_SET => "SpecificCharacterSet",

            // Patient
            PATIENT_NAME => "PatientName",
            PATIENT_ID => "PatientID",
            PATIENT_BIRTH_DATE => "PatientBirthDate",
            PATIENT_SEX => "PatientSex",
            PATIENT_AGE => "PatientAge",
            PATIENT_COMMENTS => "PatientComments",

            // Study
            STUDY_DATE => "StudyDate",
            STUDY_TIME => "StudyTime",
            ACCESSION_NUMBER => "AccessionNumber",
            STUDY_DESCRIPTION => "StudyDescription",
            STUDY_INSTANCE_UID => "StudyInstanceUID",
            STUDY_ID => "StudyID",

            // Series
            MODALITY => "Modality",
            SERIES_INSTANCE_UID => "SeriesInstanceUID",
            SERIES_NUMBER => "SeriesNumber",
            INSTANCE_NUMBER => "InstanceNumber",
            SERIES_DESCRIPTION => "SeriesDescription",
            SERIES_DATE => "SeriesDate",
            SERIES_TIME => "SeriesTime",
            PRESENTATION_INTENT_TYPE => "PresentationIntentType",

            // Equipment
            MANUFACTURER => "Manufacturer",
            INSTITUTION_NAME => "InstitutionName",
            STATION_NAME => "StationName",
            MANUFACTURER_MODEL_NAME => "ManufacturerModelName",
            DEVICE_SERIAL_NUMBER => "DeviceSerialNumber",
            SOFTWARE_VERSIONS => "SoftwareVersions",

            // SOP Common
            SOP_CLASS_UID => "SOPClassUID",
            SOP_INSTANCE_UID => "SOPInstanceUID",
            INSTANCE_CREATION_DATE => "InstanceCreationDate",
            INSTANCE_CREATION_TIME => "InstanceCreationTime",

            // Image Pixel
            SAMPLES_PER_PIXEL => "SamplesPerPixel",
            PHOTOMETRIC_INTERPRETATION => "PhotometricInterpretation",
            ROWS => "Rows",
            COLUMNS => "Columns",
            BITS_ALLOCATED => "BitsAllocated",
            BITS_STORED => "BitsStored",
            HIGH_BIT => "HighBit",
            PIXEL_REPRESENTATION => "PixelRepresentation",
            PIXEL_DATA => "PixelData",
            NUMBER_OF_FRAMES => "NumberOfFrames",

            // CT Image
            IMAGE_TYPE => "ImageType",
            RESCALE_INTERCEPT => "RescaleIntercept",
            RESCALE_SLOPE => "RescaleSlope",
            RESCALE_TYPE => "RescaleType",
            WINDOW_CENTER => "WindowCenter",
            WINDOW_WIDTH => "WindowWidth",
            WINDOW_CENTER_WIDTH_EXPLANATION => "WindowCenterWidthExplanation",
            VOI_LUT_FUNCTION => "VOILUTFunction",

            // Position/Orientation
            IMAGE_POSITION_PATIENT => "ImagePositionPatient",
            IMAGE_ORIENTATION_PATIENT => "ImageOrientationPatient",
            SLICE_THICKNESS => "SliceThickness",
            SPACING_BETWEEN_SLICES => "SpacingBetweenSlices",
            PIXEL_SPACING => "PixelSpacing",
            SLICE_LOCATION => "SliceLocation",

            // Content
            CONTENT_DATE => "ContentDate",
            CONTENT_TIME => "ContentTime",

            // Frame of Reference
            FRAME_OF_REFERENCE_UID => "FrameOfReferenceUID",
            POSITION_REFERENCE_INDICATOR => "PositionReferenceIndicator",

            // X-Ray
            KVP => "KVP",
            IMAGE_COMMENTS => "ImageComments",

            // Sequence delimiters
            ITEM => "Item",
            ITEM_DELIMITATION_ITEM => "ItemDelimitationItem",
            SEQUENCE_DELIMITATION_ITEM => "SequenceDelimitationItem",

            // DICOS ATD
            OOI_TYPE => "OOIType",
            OOI_SIZE => "OOISize", // same tag as BOUNDING_BOX_BOTTOM_RIGHT (4010,1024)
            PTO_REPRESENTATION_SEQUENCE => "PTORepresentationSequence",
            THREAT_ROI_TYPE => "ThreatROIType",
            BOUNDING_POLYGON => "BoundingPolygon",
            PTO_SEQUENCE => "PTOSequence",
            BOUNDING_BOX_TOP_LEFT => "BoundingBoxTopLeft",
            POTENTIAL_THREAT_OBJECT_ID => "PotentialThreatObjectID",
            THREAT_CATEGORY_DESCRIPTION => "ThreatCategoryDescription",
            ATD_ASSESSMENT_PROBABILITY => "ATDAssessmentProbability",
            ATD_ABILITY => "ATDAbility",
            ATD_ASSESSMENT_SEQUENCE => "ATDAssessmentSequence",
            THREAT_CONFIDENCE_SCORE => "ThreatConfidenceScore",
            ITD_TYPE => "ITDType",
            ITD_SEQUENCE => "ITDSequence",
            THREAT_ROI_SEQUENCE => "ThreatROISequence",
            ABORT_REASON => "AbortReason",
            ALARM_DECISION => "AlarmDecision",
            NUMBER_OF_ALARM_OBJECTS => "NumberOfAlarmObjects",
            ASSESSMENT_REQUEST_SEQUENCE => "AssessmentRequestSequence",
            OPERATOR_ASSESSMENT_SEQUENCE => "OperatorAssessmentSequence",

            // Reference tags
            REFERENCED_SOP_CLASS_UID => "ReferencedSOPClassUID",
            REFERENCED_SOP_INSTANCE_UID => "ReferencedSOPInstanceUID",
            REFERENCED_SERIES_SEQUENCE => "ReferencedSeriesSequence",
            REFERENCED_IMAGE_SEQUENCE => "ReferencedImageSequence",

            _ => "",
        }
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.name();
        if name.is_empty() {
            write!(f, "({:04X},{:04X})", self.group, self.element)
        } else {
            write!(f, "({:04X},{:04X}) {}", self.group, self.element, name)
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:04X},{:04X})", self.group, self.element)
    }
}

// ---------------------------------------------------------------------------
// File Meta Information (Group 0002)
// ---------------------------------------------------------------------------
pub const FILE_META_INFORMATION_GROUP_LENGTH: Tag = Tag::new(0x0002, 0x0000);
pub const FILE_META_INFORMATION_VERSION: Tag = Tag::new(0x0002, 0x0001);
pub const MEDIA_STORAGE_SOP_CLASS_UID: Tag = Tag::new(0x0002, 0x0002);
pub const MEDIA_STORAGE_SOP_INSTANCE_UID: Tag = Tag::new(0x0002, 0x0003);
pub const TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0002, 0x0010);
pub const IMPLEMENTATION_CLASS_UID: Tag = Tag::new(0x0002, 0x0012);
pub const IMPLEMENTATION_VERSION_NAME: Tag = Tag::new(0x0002, 0x0013);
pub const SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);

// ---------------------------------------------------------------------------
// Patient Module (Group 0010)
// ---------------------------------------------------------------------------
pub const PATIENT_NAME: Tag = Tag::new(0x0010, 0x0010);
pub const PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
pub const PATIENT_BIRTH_DATE: Tag = Tag::new(0x0010, 0x0030);
pub const PATIENT_SEX: Tag = Tag::new(0x0010, 0x0040);
pub const PATIENT_AGE: Tag = Tag::new(0x0010, 0x1010);
pub const PATIENT_COMMENTS: Tag = Tag::new(0x0010, 0x4000);

// ---------------------------------------------------------------------------
// General Study Module (Groups 0008, 0020)
// ---------------------------------------------------------------------------
pub const STUDY_DATE: Tag = Tag::new(0x0008, 0x0020);
pub const STUDY_TIME: Tag = Tag::new(0x0008, 0x0030);
pub const ACCESSION_NUMBER: Tag = Tag::new(0x0008, 0x0050);
pub const STUDY_DESCRIPTION: Tag = Tag::new(0x0008, 0x1030);
pub const STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
pub const STUDY_ID: Tag = Tag::new(0x0020, 0x0010);

// ---------------------------------------------------------------------------
// General Series Module
// ---------------------------------------------------------------------------
pub const MODALITY: Tag = Tag::new(0x0008, 0x0060);
pub const SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
pub const SERIES_NUMBER: Tag = Tag::new(0x0020, 0x0011);
pub const INSTANCE_NUMBER: Tag = Tag::new(0x0020, 0x0013);
pub const SERIES_DESCRIPTION: Tag = Tag::new(0x0008, 0x103E);
pub const SERIES_DATE: Tag = Tag::new(0x0008, 0x0021);
pub const SERIES_TIME: Tag = Tag::new(0x0008, 0x0031);
pub const PRESENTATION_INTENT_TYPE: Tag = Tag::new(0x0008, 0x0068);

// ---------------------------------------------------------------------------
// General Equipment Module
// ---------------------------------------------------------------------------
pub const MANUFACTURER: Tag = Tag::new(0x0008, 0x0070);
pub const INSTITUTION_NAME: Tag = Tag::new(0x0008, 0x0080);
pub const STATION_NAME: Tag = Tag::new(0x0008, 0x1010);
pub const MANUFACTURER_MODEL_NAME: Tag = Tag::new(0x0008, 0x1090);
pub const DEVICE_SERIAL_NUMBER: Tag = Tag::new(0x0018, 0x1000);
pub const SOFTWARE_VERSIONS: Tag = Tag::new(0x0018, 0x1020);

// ---------------------------------------------------------------------------
// X-Ray Acquisition Parameters
// ---------------------------------------------------------------------------
pub const KVP: Tag = Tag::new(0x0018, 0x0060);
pub const IMAGE_COMMENTS: Tag = Tag::new(0x0020, 0x4000);

// ---------------------------------------------------------------------------
// SOP Common Module
// ---------------------------------------------------------------------------
pub const SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
pub const SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub const INSTANCE_CREATION_DATE: Tag = Tag::new(0x0008, 0x0012);
pub const INSTANCE_CREATION_TIME: Tag = Tag::new(0x0008, 0x0013);

// ---------------------------------------------------------------------------
// Frame of Reference Module
// ---------------------------------------------------------------------------
pub const FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0020, 0x0052);
pub const POSITION_REFERENCE_INDICATOR: Tag = Tag::new(0x0020, 0x1040);

// ---------------------------------------------------------------------------
// Image Pixel Module (Group 0028)
// ---------------------------------------------------------------------------
pub const SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
pub const PHOTOMETRIC_INTERPRETATION: Tag = Tag::new(0x0028, 0x0004);
pub const ROWS: Tag = Tag::new(0x0028, 0x0010);
pub const COLUMNS: Tag = Tag::new(0x0028, 0x0011);
pub const BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
pub const BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
pub const HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
pub const PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
pub const PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
pub const NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);

// ---------------------------------------------------------------------------
// CT Image Module
// ---------------------------------------------------------------------------
pub const IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
pub const RESCALE_INTERCEPT: Tag = Tag::new(0x0028, 0x1052);
pub const RESCALE_SLOPE: Tag = Tag::new(0x0028, 0x1053);
pub const RESCALE_TYPE: Tag = Tag::new(0x0028, 0x1054);
pub const WINDOW_CENTER: Tag = Tag::new(0x0028, 0x1050);
pub const WINDOW_WIDTH: Tag = Tag::new(0x0028, 0x1051);
pub const WINDOW_CENTER_WIDTH_EXPLANATION: Tag = Tag::new(0x0028, 0x1055);
pub const VOI_LUT_FUNCTION: Tag = Tag::new(0x0028, 0x1056);

// ---------------------------------------------------------------------------
// Image Position / Orientation
// ---------------------------------------------------------------------------
pub const IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
pub const IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
pub const SLICE_THICKNESS: Tag = Tag::new(0x0018, 0x0050);
pub const SPACING_BETWEEN_SLICES: Tag = Tag::new(0x0018, 0x0088);
pub const PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
pub const SLICE_LOCATION: Tag = Tag::new(0x0020, 0x1041);

// ---------------------------------------------------------------------------
// Content Date/Time
// ---------------------------------------------------------------------------
pub const CONTENT_DATE: Tag = Tag::new(0x0008, 0x0023);
pub const CONTENT_TIME: Tag = Tag::new(0x0008, 0x0033);

// ---------------------------------------------------------------------------
// Sequence Delimiters
// ---------------------------------------------------------------------------
pub const ITEM: Tag = Tag::new(0xFFFE, 0xE000);
pub const ITEM_DELIMITATION_ITEM: Tag = Tag::new(0xFFFE, 0xE00D);
pub const SEQUENCE_DELIMITATION_ITEM: Tag = Tag::new(0xFFFE, 0xE0DD);

// ---------------------------------------------------------------------------
// DICOS-Specific Tags (Group 4010) - ATD/Threat Detection
// ---------------------------------------------------------------------------
pub const OOI_TYPE: Tag = Tag::new(0x4010, 0x1012);
pub const OOI_SIZE: Tag = Tag::new(0x4010, 0x1024);
pub const PTO_REPRESENTATION_SEQUENCE: Tag = Tag::new(0x4010, 0x1011);
pub const THREAT_ROI_TYPE: Tag = Tag::new(0x4010, 0x1009);
pub const BOUNDING_POLYGON: Tag = Tag::new(0x4010, 0x101D);
pub const PTO_SEQUENCE: Tag = Tag::new(0x4010, 0x1010);
pub const BOUNDING_BOX_TOP_LEFT: Tag = Tag::new(0x4010, 0x1023);
pub const BOUNDING_BOX_BOTTOM_RIGHT: Tag = Tag::new(0x4010, 0x1024);
pub const POTENTIAL_THREAT_OBJECT_ID: Tag = Tag::new(0x4010, 0x1006);
pub const THREAT_CATEGORY_DESCRIPTION: Tag = Tag::new(0x4010, 0x1028);
pub const ATD_ASSESSMENT_PROBABILITY: Tag = Tag::new(0x4010, 0x1017);

pub const ATD_ABILITY: Tag = Tag::new(0x4010, 0x1001);
pub const ATD_ASSESSMENT_SEQUENCE: Tag = Tag::new(0x4010, 0x1015);
pub const THREAT_CONFIDENCE_SCORE: Tag = Tag::new(0x4010, 0x1016);
pub const ITD_TYPE: Tag = Tag::new(0x4010, 0x1041);
pub const ITD_SEQUENCE: Tag = Tag::new(0x4010, 0x1042);
pub const THREAT_ROI_SEQUENCE: Tag = Tag::new(0x4010, 0x1020);
pub const ABORT_REASON: Tag = Tag::new(0x4010, 0x1021);
pub const ALARM_DECISION: Tag = Tag::new(0x4010, 0x100A);
pub const NUMBER_OF_ALARM_OBJECTS: Tag = Tag::new(0x4010, 0x1014);
pub const ASSESSMENT_REQUEST_SEQUENCE: Tag = Tag::new(0x4010, 0x1027);
pub const OPERATOR_ASSESSMENT_SEQUENCE: Tag = Tag::new(0x4010, 0x1029);

// Reference Tags for TDR
pub const REFERENCED_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x1150);
pub const REFERENCED_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x1155);
pub const REFERENCED_SERIES_SEQUENCE: Tag = Tag::new(0x0008, 0x1115);
pub const REFERENCED_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x1140);

// Material Classification
pub const OOI_OWNER_TYPE: Tag = Tag::new(0x4010, 0x1018);
pub const ROUTE_SEGMENT_SEQUENCE: Tag = Tag::new(0x4010, 0x1007);
pub const SCANNING_CONFIGURATION: Tag = Tag::new(0x4010, 0x100B);
pub const EXPOSURE_SEQUENCE: Tag = Tag::new(0x4010, 0x100C);
pub const PROCESSED_BIN_NUMBER_SEQUENCE: Tag = Tag::new(0x4010, 0x100D);
pub const TOTAL_PROCESSED_BIN_NUMBER: Tag = Tag::new(0x4010, 0x100E);
pub const TRANSPORT_CLASSIFICATION_SEQUENCE: Tag = Tag::new(0x4010, 0x1026);

// ---------------------------------------------------------------------------
// OOI Owner Module Tags (Group 4010)
// ---------------------------------------------------------------------------
pub const OOI_OWNER_ID: Tag = Tag::new(0x4010, 0x1030);
pub const OOI_OWNER_NAME: Tag = Tag::new(0x4010, 0x1031);
pub const OOI_OWNER_ID_TYPE: Tag = Tag::new(0x4010, 0x1032);
pub const OOI_OWNER_CATEGORY: Tag = Tag::new(0x4010, 0x1033);

// ---------------------------------------------------------------------------
// OOI Module Tags (Group 4010)
// ---------------------------------------------------------------------------
pub const OOI_ID: Tag = Tag::new(0x4010, 0x1034);
pub const OOI_TYPE_ATTR: Tag = Tag::new(0x4010, 0x1035);
pub const OOI_SIZE_ATTR: Tag = Tag::new(0x4010, 0x1036);
pub const OOI_LABEL: Tag = Tag::new(0x4010, 0x1037);

// ---------------------------------------------------------------------------
// Itinerary Module Tags (Group 4010)
// ---------------------------------------------------------------------------
pub const FLIGHT_NUMBER: Tag = Tag::new(0x4010, 0x1040);
pub const DEPARTURE_AIRPORT: Tag = Tag::new(0x4010, 0x1043);
pub const ARRIVAL_AIRPORT: Tag = Tag::new(0x4010, 0x1044);
pub const CARRIER_NAME: Tag = Tag::new(0x4010, 0x1045);
pub const CARRIER_CODE: Tag = Tag::new(0x4010, 0x1046);

// ---------------------------------------------------------------------------
// DICOS DX Detector Energy Tags (Group 4010)
// ---------------------------------------------------------------------------
pub const LOW_ENERGY_DETECTOR: Tag = Tag::new(0x4010, 0x0001);
pub const HIGH_ENERGY_DETECTOR: Tag = Tag::new(0x4010, 0x0002);
pub const DETECTOR_BIN_NUMBER: Tag = Tag::new(0x4010, 0x0003);
pub const LOWER_ENERGY: Tag = Tag::new(0x4010, 0x0005);
pub const ENERGY_RESOLUTION: Tag = Tag::new(0x4010, 0x0006);
pub const HIGHER_ENERGY: Tag = Tag::new(0x4010, 0x0007);

// ---------------------------------------------------------------------------
// DX Detector Module Tags (Group 0018)
// ---------------------------------------------------------------------------
pub const DETECTOR_TYPE: Tag = Tag::new(0x0018, 0x7004);
pub const DETECTOR_CONFIGURATION: Tag = Tag::new(0x0018, 0x7005);
pub const DETECTOR_DESCRIPTION: Tag = Tag::new(0x0018, 0x7006);
pub const DETECTOR_ID: Tag = Tag::new(0x0018, 0x700A);
pub const DETECTOR_MANUFACTURER_NAME: Tag = Tag::new(0x0018, 0x702A);
pub const DETECTOR_MANUFACTURER_MODEL_NAME: Tag = Tag::new(0x0018, 0x702B);
pub const DETECTOR_ACTIVE_TIME: Tag = Tag::new(0x0018, 0x7014);
pub const DETECTOR_ACTIVATION_OFFSET: Tag = Tag::new(0x0018, 0x7016);
pub const DETECTOR_CONDITIONS_NOMINAL_FLAG: Tag = Tag::new(0x0018, 0x7000);
pub const DETECTOR_TEMPERATURE: Tag = Tag::new(0x0018, 0x7001);
pub const DETECTOR_ELEMENT_PHYSICAL_SIZE: Tag = Tag::new(0x0018, 0x7020);
pub const DETECTOR_ELEMENT_SPACING: Tag = Tag::new(0x0018, 0x7022);
pub const DETECTOR_ACTIVE_DIMENSIONS: Tag = Tag::new(0x0018, 0x7026);
pub const DETECTOR_BINNING: Tag = Tag::new(0x0018, 0x701A);
pub const FIELD_OF_VIEW_SHAPE: Tag = Tag::new(0x0018, 0x1147);
pub const FIELD_OF_VIEW_DIMENSIONS: Tag = Tag::new(0x0018, 0x1149);

// ---------------------------------------------------------------------------
// DX X-Ray Acquisition Tags (Group 0018)
// ---------------------------------------------------------------------------
pub const XRAY_TUBE_CURRENT_IN_MA: Tag = Tag::new(0x0018, 0x8151);
pub const EXPOSURE_TIME_IN_MS: Tag = Tag::new(0x0018, 0x9328);
pub const DISTANCE_SOURCE_TO_DETECTOR: Tag = Tag::new(0x0018, 0x1110);
pub const DISTANCE_SOURCE_TO_PATIENT: Tag = Tag::new(0x0018, 0x1111);
pub const ESTIMATED_DOSE_SAVING: Tag = Tag::new(0x0018, 0x9324);
pub const EXPOSURE_CONTROL_MODE: Tag = Tag::new(0x0018, 0x7060);
pub const EXPOSURE_CONTROL_MODE_DESCRIPTION: Tag = Tag::new(0x0018, 0x7062);
pub const EXPOSURE_STATUS: Tag = Tag::new(0x0018, 0x7064);
pub const PHOTOTIMER_SETTING: Tag = Tag::new(0x0018, 0x7065);
pub const SENSITIVITY_VALUE: Tag = Tag::new(0x0018, 0x6000);
pub const ANODE_TARGET_MATERIAL: Tag = Tag::new(0x0018, 0x1191);
pub const BODY_PART_THICKNESS: Tag = Tag::new(0x0018, 0x11A0);
pub const COMPRESSION_FORCE: Tag = Tag::new(0x0018, 0x11A2);
pub const GRID: Tag = Tag::new(0x0018, 0x1166);
pub const FOCAL_SPOT_SIZE: Tag = Tag::new(0x0018, 0x1190);
pub const IMAGE_AND_FLUOROSCOPY_AREA_DOSE_PRODUCT: Tag = Tag::new(0x0018, 0x115E);

// ---------------------------------------------------------------------------
// DICOS General Series Energy Tags (Group 6100)
// ---------------------------------------------------------------------------
pub const SERIES_ENERGY: Tag = Tag::new(0x6100, 0x0030);
pub const SERIES_ENERGY_DESCRIPTION: Tag = Tag::new(0x6100, 0x0031);

// ---------------------------------------------------------------------------
// Extended Image Pixel Module (Group 0028)
// ---------------------------------------------------------------------------
pub const PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
pub const SMALLEST_IMAGE_PIXEL_VALUE: Tag = Tag::new(0x0028, 0x0106);
pub const LARGEST_IMAGE_PIXEL_VALUE: Tag = Tag::new(0x0028, 0x0107);
pub const PIXEL_PADDING_VALUE: Tag = Tag::new(0x0028, 0x0120);
pub const PIXEL_PADDING_RANGE_LIMIT: Tag = Tag::new(0x0028, 0x0121);
pub const LOSSY_IMAGE_COMPRESSION: Tag = Tag::new(0x0028, 0x2110);
pub const LOSSY_IMAGE_COMPRESSION_RATIO: Tag = Tag::new(0x0028, 0x2112);
pub const LUT_DESCRIPTOR: Tag = Tag::new(0x0028, 0x3002);
pub const LUT_DATA: Tag = Tag::new(0x0028, 0x3006);
pub const VOI_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x3010);
pub const MODALITY_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x3000);
pub const RED_PALETTE_COLOR_LUT_DATA: Tag = Tag::new(0x0028, 0x1201);
pub const GREEN_PALETTE_COLOR_LUT_DATA: Tag = Tag::new(0x0028, 0x1202);
pub const BLUE_PALETTE_COLOR_LUT_DATA: Tag = Tag::new(0x0028, 0x1203);

// ---------------------------------------------------------------------------
// CT Acquisition Parameters (Group 0018)
// ---------------------------------------------------------------------------
pub const SCAN_OPTIONS: Tag = Tag::new(0x0018, 0x0022);
pub const DATA_COLLECTION_DIAMETER: Tag = Tag::new(0x0018, 0x0090);
pub const RECONSTRUCTION_DIAMETER: Tag = Tag::new(0x0018, 0x1100);
pub const CONVOLUTION_KERNEL: Tag = Tag::new(0x0018, 0x1210);
pub const EXPOSURE_TIME: Tag = Tag::new(0x0018, 0x1150);
pub const XRAY_TUBE_CURRENT: Tag = Tag::new(0x0018, 0x1151);
pub const EXPOSURE: Tag = Tag::new(0x0018, 0x1152);
pub const EXPOSURE_IN_MAS: Tag = Tag::new(0x0018, 0x1153);
pub const FILTER_TYPE: Tag = Tag::new(0x0018, 0x1160);
pub const GENERATOR_POWER: Tag = Tag::new(0x0018, 0x1170);
pub const FOCAL_SPOTS: Tag = Tag::new(0x0018, 0x1190);
pub const TABLE_HEIGHT: Tag = Tag::new(0x0018, 0x1130);
pub const ROTATION_DIRECTION: Tag = Tag::new(0x0018, 0x1140);
pub const GANTRY_DETECTOR_TILT: Tag = Tag::new(0x0018, 0x1120);
pub const TABLE_SPEED: Tag = Tag::new(0x0018, 0x9309);
pub const TABLE_FEED_PER_ROTATION: Tag = Tag::new(0x0018, 0x9310);
pub const SPIRAL_PITCH_FACTOR: Tag = Tag::new(0x0018, 0x9311);
pub const SINGLE_COLLIMATION_WIDTH: Tag = Tag::new(0x0018, 0x9306);
pub const TOTAL_COLLIMATION_WIDTH: Tag = Tag::new(0x0018, 0x9307);
pub const DATE_OF_LAST_CALIBRATION: Tag = Tag::new(0x0018, 0x1200);
pub const TIME_OF_LAST_CALIBRATION: Tag = Tag::new(0x0018, 0x1201);
pub const ACQUISITION_TYPE: Tag = Tag::new(0x0018, 0x9302);
pub const TUBE_ANGLE: Tag = Tag::new(0x0018, 0x9303);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_ordering() {
        assert!(Tag::new(0x0002, 0x0000) < Tag::new(0x0002, 0x0001));
        assert!(Tag::new(0x0002, 0xFFFF) < Tag::new(0x0008, 0x0000));
        assert!(Tag::new(0x0028, 0x0010) < Tag::new(0x7FE0, 0x0010));
    }

    #[test]
    fn tag_equality() {
        let a = Tag::new(0x0028, 0x0010);
        let b = ROWS;
        assert_eq!(a, b);
    }

    #[test]
    fn is_private() {
        assert!(!ROWS.is_private());
        assert!(!PIXEL_DATA.is_private());
        // Group 0x4011 would be private (odd)
        assert!(Tag::new(0x4011, 0x0001).is_private());
        // DICOS group 0x4010 is even, so NOT private
        assert!(!OOI_TYPE.is_private());
    }

    #[test]
    fn is_group_0002() {
        assert!(TRANSFER_SYNTAX_UID.is_group_0002());
        assert!(MEDIA_STORAGE_SOP_CLASS_UID.is_group_0002());
        assert!(!PATIENT_NAME.is_group_0002());
        assert!(!PIXEL_DATA.is_group_0002());
    }

    #[test]
    fn tag_name_lookup() {
        assert_eq!(PATIENT_NAME.name(), "PatientName");
        assert_eq!(ROWS.name(), "Rows");
        assert_eq!(COLUMNS.name(), "Columns");
        assert_eq!(PIXEL_DATA.name(), "PixelData");
        assert_eq!(TRANSFER_SYNTAX_UID.name(), "TransferSyntaxUID");
        assert_eq!(MODALITY.name(), "Modality");
        assert_eq!(NUMBER_OF_FRAMES.name(), "NumberOfFrames");
        assert_eq!(SOP_CLASS_UID.name(), "SOPClassUID");
    }

    #[test]
    fn unknown_tag_name_is_empty() {
        let unknown = Tag::new(0x9999, 0x9999);
        assert_eq!(unknown.name(), "");
    }

    #[test]
    fn tag_debug_format() {
        let s = format!("{:?}", PATIENT_NAME);
        assert!(s.contains("0010"));
        assert!(s.contains("PatientName"));
    }

    #[test]
    fn tag_display_format() {
        let s = format!("{}", ROWS);
        assert_eq!(s, "(0028,0010)");
    }

    #[test]
    fn sequence_delimiter_constants() {
        assert_eq!(ITEM.group, 0xFFFE);
        assert_eq!(ITEM.element, 0xE000);
        assert_eq!(ITEM_DELIMITATION_ITEM.element, 0xE00D);
        assert_eq!(SEQUENCE_DELIMITATION_ITEM.element, 0xE0DD);
    }

    #[test]
    fn dicos_tags_are_in_group_4010() {
        assert_eq!(OOI_TYPE.group, 0x4010);
        assert_eq!(ATD_ABILITY.group, 0x4010);
        assert_eq!(ALARM_DECISION.group, 0x4010);
        assert_eq!(FLIGHT_NUMBER.group, 0x4010);
    }

    #[test]
    fn tag_hash_consistency() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ROWS);
        set.insert(COLUMNS);
        set.insert(ROWS); // duplicate
        assert_eq!(set.len(), 2);
    }
}
