//! Spike step S1 (docs/design/rust-spike.md): the road layers of the style.
//!
//!     cargo run --release -- spike/data/layers.json > made.json
//!
//! The layers come from Python (`layers.json`, made by tools/make_spike_fixture.py): they depend on the look and on flags of the
//! data that S1 found, so this step only checks the path from the file to the style. S2 adds the `roads` source from Parquet.

use serde_json::{json, Value};
use std::{env, fs, process};

fn style(layers: Value) -> Value {
    json!({ "layers": layers })
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: roadstyle-spike <layers.json>");
        process::exit(2);
    }
    let text = fs::read_to_string(&args[1]).unwrap_or_else(|e| {
        eprintln!("{}: {e}", args[1]);
        process::exit(1)
    });
    let layers: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("{}: not JSON: {e}", args[1]);
        process::exit(1)
    });
    println!("{}", style(layers));
}
