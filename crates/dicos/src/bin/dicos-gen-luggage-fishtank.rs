//! Generate a synthetic DICOS CT volume that depicts luggage with a fish tank.
//!
//! The output intentionally uses neutral/public-safe metadata values.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use dicos::tag;
use dicos::types::{Dataset, Element, Value};
use dicos::vr::Vr;
use dicos::writer;

const DICOS_CT_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.501.1";
const EXPLICIT_VR_LE: &str = "1.2.840.10008.1.2.1";
const IMPL_CLASS_UID: &str = "1.2.826.0.1.3680043.8.498.999.1";

#[derive(Debug, Clone)]
struct Args {
    out: PathBuf,
    size: [usize; 3], // x,y,z
    seed: u64,
    with_noise: bool,
    include_threat_tags: bool,
}

#[derive(Debug, Clone, Copy)]
struct BBox {
    min: [usize; 3],
    max: [usize; 3],
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let (voxels, fish_tank_bbox) = synthesize_volume(args.size, args.seed, args.with_noise);
    let ds = build_dataset(&voxels, args.size, fish_tank_bbox, args.include_threat_tags);

    let out_file = File::create(&args.out)?;
    let mut writer_buf = BufWriter::new(out_file);
    let bytes = writer::write(&mut writer_buf, &ds)?;
    writer_buf.flush()?;

    println!(
        "Wrote {} ({} bytes) [{}x{}x{}]",
        args.out.display(),
        bytes,
        args.size[0],
        args.size[1],
        args.size[2]
    );
    Ok(())
}

fn parse_args<I>(mut it: I) -> Result<Args, String>
where
    I: Iterator<Item = String>,
{
    let mut args = Args {
        out: PathBuf::from("testdata/synthetic/luggage_fishtank_ct.dcs"),
        size: [256, 256, 160],
        seed: 20260225,
        with_noise: true,
        include_threat_tags: true,
    };

    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--out" => {
                let value = it
                    .next()
                    .ok_or_else(|| String::from("missing value for --out"))?;
                args.out = PathBuf::from(value);
            }
            "--size" => {
                let value = it
                    .next()
                    .ok_or_else(|| String::from("missing value for --size"))?;
                args.size = parse_size(&value)?;
            }
            "--seed" => {
                let value = it
                    .next()
                    .ok_or_else(|| String::from("missing value for --seed"))?;
                args.seed = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --seed: {value}"))?;
            }
            "--with-noise" => {
                let value = it
                    .next()
                    .ok_or_else(|| String::from("missing value for --with-noise"))?;
                args.with_noise = parse_on_off("--with-noise", &value)?;
            }
            "--include-threat-tags" => {
                let value = it
                    .next()
                    .ok_or_else(|| String::from("missing value for --include-threat-tags"))?;
                args.include_threat_tags = parse_on_off("--include-threat-tags", &value)?;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                return Err(format!("unknown argument: {flag}"));
            }
        }
    }

    Ok(args)
}

fn parse_size(s: &str) -> Result<[usize; 3], String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 {
        return Err(format!("invalid --size '{s}', expected x,y,z"));
    }

    let mut out = [0usize; 3];
    for (i, part) in parts.iter().enumerate() {
        out[i] = part
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid integer in --size: {part}"))?;
    }
    if out.iter().any(|&v| v < 32) {
        return Err(String::from(
            "--size values must be >= 32 in each axis for this generator",
        ));
    }
    Ok(out)
}

fn parse_on_off(flag: &str, value: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("{flag} expects on|off, got '{value}'")),
    }
}

fn print_help() {
    println!("dicos-gen-luggage-fishtank");
    println!("Generate a synthetic DICOS CT sample with a fish tank inside luggage.");
    println!();
    println!("Options:");
    println!("  --out <path>                   Output file path");
    println!("  --size <x,y,z>                 Volume size (default: 256,256,160)");
    println!("  --seed <u64>                   Noise seed (default: 20260225)");
    println!("  --with-noise <on|off>          Enable noise (default: on)");
    println!("  --include-threat-tags <on|off> Include Group 4010 ROI tags (default: on)");
}

fn synthesize_volume(size: [usize; 3], seed: u64, with_noise: bool) -> (Vec<u16>, BBox) {
    let [sx, sy, sz] = size;
    let mut data = vec![0u16; sx * sy * sz];

    // Global background.
    data.fill(450);

    // Luggage body and shell.
    let luggage = BBox {
        min: [sx / 8, sy / 8, sz / 6],
        max: [sx - sx / 8 - 1, sy - sy / 8 - 1, sz - sz / 6 - 1],
    };
    fill_box(&mut data, size, luggage, 1800);
    paint_shell(&mut data, size, luggage, 3, 6200);

    // Fish tank in luggage center.
    let fish_tank = BBox {
        min: [sx / 3, sy / 3, sz / 3],
        max: [sx * 2 / 3, sy * 2 / 3, sz * 2 / 3],
    };

    // Water.
    fill_box(&mut data, size, fish_tank, 11200);

    // Air gap near top of tank.
    let air_gap = BBox {
        min: [fish_tank.min[0] + 2, fish_tank.min[1] + 2, fish_tank.max[2] - 4],
        max: [fish_tank.max[0] - 2, fish_tank.max[1] - 2, fish_tank.max[2] - 2],
    };
    fill_box(&mut data, size, air_gap, 900);

    // Glass wall.
    paint_shell(&mut data, size, fish_tank, 2, 26000);

    // Simple fish ellipsoids.
    paint_ellipsoid(
        &mut data,
        size,
        [sx * 46 / 100, sy * 47 / 100, sz * 47 / 100],
        [sx / 28, sy / 36, sz / 48],
        14500,
    );
    paint_ellipsoid(
        &mut data,
        size,
        [sx * 56 / 100, sy * 54 / 100, sz * 44 / 100],
        [sx / 32, sy / 42, sz / 54],
        15000,
    );

    // A few clutter blocks in the luggage around the tank.
    let clutter_a = BBox {
        min: [sx / 5, sy / 5, sz / 3],
        max: [sx / 4, sy * 2 / 5, sz / 2],
    };
    let clutter_b = BBox {
        min: [sx * 3 / 4, sy / 4, sz / 3],
        max: [sx * 4 / 5, sy * 3 / 7, sz / 2],
    };
    fill_box(&mut data, size, clutter_a, 7800);
    fill_box(&mut data, size, clutter_b, 9200);

    if with_noise {
        add_seeded_noise(&mut data, seed);
    }

    (data, fish_tank)
}

fn fill_box(data: &mut [u16], size: [usize; 3], bbox: BBox, value: u16) {
    let [sx, sy, _sz] = size;
    for z in bbox.min[2]..=bbox.max[2] {
        for y in bbox.min[1]..=bbox.max[1] {
            let row = z * sx * sy + y * sx;
            for x in bbox.min[0]..=bbox.max[0] {
                data[row + x] = value;
            }
        }
    }
}

fn paint_shell(data: &mut [u16], size: [usize; 3], bbox: BBox, thickness: usize, value: u16) {
    let [sx, sy, _sz] = size;
    for z in bbox.min[2]..=bbox.max[2] {
        for y in bbox.min[1]..=bbox.max[1] {
            let row = z * sx * sy + y * sx;
            for x in bbox.min[0]..=bbox.max[0] {
                let dx = x - bbox.min[0];
                let ex = bbox.max[0] - x;
                let dy = y - bbox.min[1];
                let ey = bbox.max[1] - y;
                let dz = z - bbox.min[2];
                let ez = bbox.max[2] - z;
                if dx < thickness
                    || ex < thickness
                    || dy < thickness
                    || ey < thickness
                    || dz < thickness
                    || ez < thickness
                {
                    data[row + x] = value;
                }
            }
        }
    }
}

fn paint_ellipsoid(
    data: &mut [u16],
    size: [usize; 3],
    center: [usize; 3],
    radius: [usize; 3],
    value: u16,
) {
    let [sx, sy, sz] = size;
    let [cx, cy, cz] = center;
    let [rx, ry, rz] = radius;
    if rx == 0 || ry == 0 || rz == 0 {
        return;
    }

    let z_min = cz.saturating_sub(rz);
    let z_max = (cz + rz).min(sz - 1);
    let y_min = cy.saturating_sub(ry);
    let y_max = (cy + ry).min(sy - 1);
    let x_min = cx.saturating_sub(rx);
    let x_max = (cx + rx).min(sx - 1);

    for z in z_min..=z_max {
        for y in y_min..=y_max {
            let row = z * sx * sy + y * sx;
            for x in x_min..=x_max {
                let dx = (x as f64 - cx as f64) / rx as f64;
                let dy = (y as f64 - cy as f64) / ry as f64;
                let dz = (z as f64 - cz as f64) / rz as f64;
                if dx * dx + dy * dy + dz * dz <= 1.0 {
                    data[row + x] = value;
                }
            }
        }
    }
}

fn add_seeded_noise(data: &mut [u16], seed: u64) {
    let mut x = seed ^ 0x9E37_79B9_7F4A_7C15;
    for voxel in data.iter_mut() {
        // Xorshift-style deterministic PRNG.
        x ^= x << 7;
        x ^= x >> 9;
        x ^= x << 8;
        let noise = (x & 0x1F) as i32 - 15; // [-15, +16]
        let v = i32::from(*voxel) + noise;
        *voxel = v.clamp(0, i32::from(u16::MAX)) as u16;
    }
}

fn build_dataset(
    voxels: &[u16],
    size: [usize; 3],
    fish_tank: BBox,
    include_threat_tags: bool,
) -> Dataset {
    let [sx, sy, sz] = size;
    let now_date = "20260225";
    let now_time = "120000.000000";

    let sop_instance_uid = "1.2.826.0.1.3680043.8.498.999.1001";
    let study_uid = "1.2.826.0.1.3680043.8.498.999.2001";
    let series_uid = "1.2.826.0.1.3680043.8.498.999.3001";
    let frame_uid = "1.2.826.0.1.3680043.8.498.999.4001";

    let mut ds = Dataset::new();

    // File Meta Information.
    ds.put_string(tag::MEDIA_STORAGE_SOP_CLASS_UID, Vr::UI, DICOS_CT_SOP_CLASS_UID);
    ds.put_string(tag::MEDIA_STORAGE_SOP_INSTANCE_UID, Vr::UI, sop_instance_uid);
    ds.put_string(tag::TRANSFER_SYNTAX_UID, Vr::UI, EXPLICIT_VR_LE);
    ds.put_string(tag::IMPLEMENTATION_CLASS_UID, Vr::UI, IMPL_CLASS_UID);
    ds.put_string(tag::IMPLEMENTATION_VERSION_NAME, Vr::SH, "DICOSRS_2026A");

    // SOP + study + series.
    ds.put_string(tag::SOP_CLASS_UID, Vr::UI, DICOS_CT_SOP_CLASS_UID);
    ds.put_string(tag::SOP_INSTANCE_UID, Vr::UI, sop_instance_uid);
    ds.put_string(tag::MODALITY, Vr::CS, "CT");
    ds.insert(Element::new(
        tag::IMAGE_TYPE,
        Vr::CS,
        Value::Strings(vec![
            String::from("ORIGINAL"),
            String::from("PRIMARY"),
            String::from("AXIAL"),
        ]),
    ));
    ds.put_string(tag::PRESENTATION_INTENT_TYPE, Vr::CS, "FOR PRESENTATION");

    ds.put_string(tag::STUDY_INSTANCE_UID, Vr::UI, study_uid);
    ds.put_string(tag::SERIES_INSTANCE_UID, Vr::UI, series_uid);
    ds.put_string(tag::FRAME_OF_REFERENCE_UID, Vr::UI, frame_uid);

    ds.put_string(tag::STUDY_DATE, Vr::DA, now_date);
    ds.put_string(tag::SERIES_DATE, Vr::DA, now_date);
    ds.put_string(tag::CONTENT_DATE, Vr::DA, now_date);
    ds.put_string(tag::INSTANCE_CREATION_DATE, Vr::DA, now_date);
    ds.put_string(tag::STUDY_TIME, Vr::TM, now_time);
    ds.put_string(tag::SERIES_TIME, Vr::TM, now_time);
    ds.put_string(tag::CONTENT_TIME, Vr::TM, now_time);
    ds.put_string(tag::INSTANCE_CREATION_TIME, Vr::TM, now_time);

    ds.put_string(tag::STUDY_DESCRIPTION, Vr::LO, "Synthetic Luggage Scan");
    ds.put_string(tag::SERIES_DESCRIPTION, Vr::LO, "Luggage with Fish Tank (Synthetic)");
    ds.put_string(tag::PATIENT_NAME, Vr::PN, "UNKNOWN^OBJECT");
    ds.put_string(tag::PATIENT_ID, Vr::LO, "PUBLIC-DEMO-001");
    ds.put_string(tag::MANUFACTURER, Vr::LO, "GenericVendor");
    ds.put_string(tag::MANUFACTURER_MODEL_NAME, Vr::LO, "DemoScanner");
    ds.put_string(tag::STATION_NAME, Vr::SH, "LAB_STATION");
    ds.put_string(tag::DEVICE_SERIAL_NUMBER, Vr::LO, "SN-000000");
    ds.put_string(tag::SOFTWARE_VERSIONS, Vr::LO, "dicos.rs-demo");

    // Image Pixel Module.
    ds.put_u16(tag::SAMPLES_PER_PIXEL, Vr::US, 1);
    ds.put_string(tag::PHOTOMETRIC_INTERPRETATION, Vr::CS, "MONOCHROME2");
    ds.put_u16(tag::ROWS, Vr::US, sy as u16);
    ds.put_u16(tag::COLUMNS, Vr::US, sx as u16);
    ds.put_u16(tag::BITS_ALLOCATED, Vr::US, 16);
    ds.put_u16(tag::BITS_STORED, Vr::US, 16);
    ds.put_u16(tag::HIGH_BIT, Vr::US, 15);
    ds.put_u16(tag::PIXEL_REPRESENTATION, Vr::US, 0);
    ds.put_string(tag::NUMBER_OF_FRAMES, Vr::IS, sz.to_string());

    // Basic CT spatial/display tags.
    ds.put_string(tag::PIXEL_SPACING, Vr::DS, "1.0\\1.0");
    ds.put_string(tag::SLICE_THICKNESS, Vr::DS, "1.0");
    ds.put_string(tag::SPACING_BETWEEN_SLICES, Vr::DS, "1.0");
    ds.put_string(tag::IMAGE_ORIENTATION_PATIENT, Vr::DS, "1\\0\\0\\0\\1\\0");
    ds.put_string(tag::IMAGE_POSITION_PATIENT, Vr::DS, "0\\0\\0");
    ds.put_string(tag::RESCALE_INTERCEPT, Vr::DS, "0");
    ds.put_string(tag::RESCALE_SLOPE, Vr::DS, "1");
    ds.put_string(tag::RESCALE_TYPE, Vr::LO, "US");
    ds.put_string(tag::WINDOW_CENTER, Vr::DS, "12000");
    ds.put_string(tag::WINDOW_WIDTH, Vr::DS, "28000");
    ds.put_string(tag::WINDOW_CENTER_WIDTH_EXPLANATION, Vr::LO, "DEFAULT");
    ds.put_string(tag::VOI_LUT_FUNCTION, Vr::CS, "LINEAR");

    if include_threat_tags {
        let mut roi = Dataset::new();
        roi.put_string(
            tag::BOUNDING_BOX_TOP_LEFT,
            Vr::DS,
            format!(
                "{}\\{}\\{}",
                fish_tank.min[0], fish_tank.min[1], fish_tank.min[2]
            ),
        );
        roi.put_string(
            tag::BOUNDING_BOX_BOTTOM_RIGHT,
            Vr::DS,
            format!(
                "{}\\{}\\{}",
                fish_tank.max[0], fish_tank.max[1], fish_tank.max[2]
            ),
        );
        roi.put_string(tag::POTENTIAL_THREAT_OBJECT_ID, Vr::LO, "OOI-1");
        roi.put_string(tag::THREAT_CATEGORY_DESCRIPTION, Vr::LO, "Aquarium");
        roi.put_string(tag::THREAT_CONFIDENCE_SCORE, Vr::DS, "0.97");
        roi.put_string(tag::OOI_LABEL, Vr::LO, "FISH_TANK");
        roi.put_string(tag::ALARM_DECISION, Vr::CS, "ALARM");

        ds.insert(Element::new(
            tag::THREAT_ROI_SEQUENCE,
            Vr::SQ,
            Value::Sequence(vec![roi]),
        ));
        ds.put_u16(tag::NUMBER_OF_ALARM_OBJECTS, Vr::US, 1);
    }

    // Native uncompressed pixel data.
    let mut pixel_bytes = Vec::with_capacity(voxels.len() * 2);
    for p in voxels {
        pixel_bytes.extend_from_slice(&p.to_le_bytes());
    }
    ds.insert(Element::new(tag::PIXEL_DATA, Vr::OW, Value::Bytes(pixel_bytes)));

    ds
}
