//! Level meter rendering.

/// Quietest level shown; anything below is an empty bar.
pub const FLOOR_DBFS: f32 = -60.0;

/// Fraction of the bar to fill for a level, 0.0 to 1.0.
pub fn fill(dbfs: Option<f32>) -> f32 {
	match dbfs {
		Some(db) if db.is_finite() => ((db - FLOOR_DBFS) / -FLOOR_DBFS).clamp(0.0, 1.0),
		_ => 0.0,
	}
}

/// A text bar `width` cells wide, e.g. `#####-----`.
pub fn bar(dbfs: Option<f32>, width: usize) -> String {
	let filled = (fill(dbfs) * width as f32).round() as usize;
	let filled = filled.min(width);
	format!("{}{}", "#".repeat(filled), "-".repeat(width - filled))
}
