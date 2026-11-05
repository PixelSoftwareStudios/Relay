//! Level meter scaling.

/// Quietest level shown; anything below is an empty bar.
pub const FLOOR_DBFS: f32 = -60.0;

/// Fraction of a bar to fill for a level, 0.0 to 1.0. Missing or non-finite
/// readings are empty rather than guessed.
pub fn fill(dbfs: Option<f32>) -> f32 {
	match dbfs {
		Some(db) if db.is_finite() => ((db - FLOOR_DBFS) / -FLOOR_DBFS).clamp(0.0, 1.0),
		_ => 0.0,
	}
}
