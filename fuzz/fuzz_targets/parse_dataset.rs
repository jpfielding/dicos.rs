#![no_main]

//! Fuzz target: the DICOS parser must never panic on arbitrary input.
//!
//! Feeds raw bytes straight into `parse_with_limit` with a 1 MiB per-element
//! allocation cap so a crafted length field cannot exhaust memory. Any
//! `Err` is an acceptable, well-typed rejection; only a panic (unwrap, index
//! out of bounds, unbounded allocation) is a finding.

use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let _ = dicos::parse_with_limit(Cursor::new(data), 1 << 20);
});
