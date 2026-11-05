//! `audio-corpus model-info <onnx>`: print the declared inputs and outputs.

use std::path::Path;

use tract_onnx::prelude::*;

use crate::audio::{BenchError, Result};

pub fn describe(path: &Path) -> Result<String> {
	let model = tract_onnx::onnx()
		.model_for_path(path)
		.map_err(|e| BenchError::Dsp(format!("load {}: {e}", path.display())))?;
	let mut text = String::new();
	for (kind, outlets) in [("input", &model.inputs), ("output", &model.outputs)] {
		for outlet in outlets {
			let node = &model.nodes[outlet.node];
			let fact = model
				.outlet_fact(*outlet)
				.map_err(|e| BenchError::Dsp(e.to_string()))?;
			text.push_str(&format!("{kind} {} {fact:?}\n", node.name));
		}
	}
	match model.into_optimized() {
		Ok(_) => text.push_str("tract: optimises and runs\n"),
		Err(e) => text.push_str(&format!("tract: cannot build a runnable model: {e:#}\n")),
	}
	Ok(text)
}
