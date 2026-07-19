//! dicosctl -- DICOS command-line tool.
//!
//! Provides basic utilities for inspecting DICOS files:
//! - `dump` -- Print all metadata as JSON
//! - `info` -- Print a human-readable summary

use std::fs::File;
use std::io::BufReader;
use std::process;

use clap::{Parser, Subcommand};
use serde_json::{json, Value as JsonValue};

use dicos::reader;
use dicos::tag::Tag;
use dicos::types::{Dataset, Value};

#[derive(Parser)]
#[command(name = "dicosctl", about = "DICOS command-line tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Dump all metadata as JSON.
    Dump {
        /// Path to the DICOS (.dcs) file.
        file: String,
        /// Emit compact (single-line) JSON instead of pretty-printed.
        #[arg(short, long)]
        compact: bool,
    },
    /// Print a human-readable summary of the file.
    Info {
        /// Path to the DICOS (.dcs) file.
        file: String,
    },
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Dump { ref file, compact } => cmd_dump(file, !compact),
        Commands::Info { ref file } => cmd_info(file),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

fn open_dataset(path: &str) -> Result<Dataset, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let ds = reader::parse(reader)?;
    Ok(ds)
}

// ---------------------------------------------------------------------------
// dump command
// ---------------------------------------------------------------------------

fn cmd_dump(path: &str, pretty: bool) -> Result<(), Box<dyn std::error::Error>> {
    let ds = open_dataset(path)?;
    let json = dataset_to_json(&ds);

    let output = if pretty {
        serde_json::to_string_pretty(&json)?
    } else {
        serde_json::to_string(&json)?
    };

    println!("{output}");
    Ok(())
}

fn dataset_to_json(ds: &Dataset) -> JsonValue {
    let mut map = serde_json::Map::new();

    for elem in ds.iter() {
        let key = format_tag(&elem.tag);
        let entry = element_to_json(elem);
        map.insert(key, entry);
    }

    JsonValue::Object(map)
}

fn format_tag(tag: &Tag) -> String {
    let name = tag.name();
    if name.is_empty() {
        format!("({:04X},{:04X})", tag.group, tag.element)
    } else {
        name.to_string()
    }
}

fn element_to_json(elem: &dicos::types::Element) -> JsonValue {
    let vr_str = format!("{}", elem.vr);

    match &elem.value {
        Value::Str(s) => json!({
            "vr": vr_str,
            "Value": s,
        }),
        Value::Strings(ss) => json!({
            "vr": vr_str,
            "Value": ss,
        }),
        Value::U16(v) => json!({
            "vr": vr_str,
            "Value": v,
        }),
        Value::U16s(vs) => json!({
            "vr": vr_str,
            "Value": vs,
        }),
        Value::U32(v) => json!({
            "vr": vr_str,
            "Value": v,
        }),
        Value::I16(v) => json!({
            "vr": vr_str,
            "Value": v,
        }),
        Value::I32(v) => json!({
            "vr": vr_str,
            "Value": v,
        }),
        Value::F32(v) => json!({
            "vr": vr_str,
            "Value": v,
        }),
        Value::F64(v) => json!({
            "vr": vr_str,
            "Value": v,
        }),
        Value::F32s(vs) => json!({
            "vr": vr_str,
            "Value": vs,
        }),
        Value::F64s(vs) => json!({
            "vr": vr_str,
            "Value": vs,
        }),
        Value::Bytes(b) => json!({
            "vr": vr_str,
            "Length": b.len(),
        }),
        Value::Sequence(items) => {
            let item_json: Vec<JsonValue> = items.iter().map(dataset_to_json).collect();
            json!({
                "vr": "SQ",
                "Value": item_json,
            })
        }
        Value::PixelData(pd) => json!({
            "vr": "OW",
            "Encapsulated": pd.is_compressed(),
            "Frames": pd.num_frames(),
        }),
    }
}

// ---------------------------------------------------------------------------
// info command
// ---------------------------------------------------------------------------

fn cmd_info(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let ds = open_dataset(path)?;

    println!("File: {path}");
    println!("Elements: {}", ds.len());

    if let Some(modality) = ds.get_string(dicos::tag::MODALITY) {
        println!("Modality: {modality}");
    }

    if let (Some(rows), Some(cols)) = (ds.rows(), ds.columns()) {
        if rows > 0 && cols > 0 {
            println!("Dimensions: {cols}x{rows}");
        }
    }

    let frames = ds.number_of_frames();
    if frames > 1 {
        println!("Frames: {frames}");
    }

    if let Some(bits) = ds.bits_allocated() {
        println!("Bits Allocated: {bits}");
    }

    let ts = ds.transfer_syntax();
    println!("Transfer Syntax: {} ({})", ts.name(), ts.uid());

    if let Some(sop_class) = ds.get_string(dicos::tag::SOP_CLASS_UID) {
        println!("SOP Class: {sop_class}");
    }

    if let Some(sop_instance) = ds.get_string(dicos::tag::SOP_INSTANCE_UID) {
        println!("SOP Instance: {sop_instance}");
    }

    if let Some(patient_name) = ds.get_string(dicos::tag::PATIENT_NAME) {
        println!("Patient Name: {patient_name}");
    }

    if let Some(series_desc) = ds.get_string(dicos::tag::SERIES_DESCRIPTION) {
        println!("Series: {series_desc}");
    }

    if let Some(manufacturer) = ds.get_string(dicos::tag::MANUFACTURER) {
        println!("Manufacturer: {manufacturer}");
    }

    if let Some(model) = ds.get_string(dicos::tag::MANUFACTURER_MODEL_NAME) {
        println!("Model: {model}");
    }

    Ok(())
}
