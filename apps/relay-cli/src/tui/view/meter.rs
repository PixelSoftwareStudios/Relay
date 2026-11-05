//! Level meter rendering.

use relay_client::meter::fill;

/// A text bar `width` cells wide, e.g. `#####-----`.
pub fn bar(dbfs: Option<f32>, width: usize) -> String {
	let filled = (fill(dbfs) * width as f32).round() as usize;
	let filled = filled.min(width);
	format!("{}{}", "#".repeat(filled), "-".repeat(width - filled))
}
