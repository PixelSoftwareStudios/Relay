//! Device sample formats normalised to `f32` in `[-1, 1]`.

/// A device PCM sample type.
pub trait Sample: Copy + Send + 'static {
	/// Silence in this format.
	const SILENCE: Self;

	/// Normalise to `f32`. Non-finite floats become silence.
	fn to_f32(self) -> f32;

	/// Convert from `f32`, saturating at the format limits; non-finite
	/// input becomes silence.
	fn from_f32(value: f32) -> Self;
}

fn clean(value: f32) -> f32 {
	if value.is_finite() {
		value.clamp(-1.0, 1.0)
	} else {
		0.0
	}
}

impl Sample for f32 {
	const SILENCE: Self = 0.0;

	fn to_f32(self) -> f32 {
		clean(self)
	}

	fn from_f32(value: f32) -> Self {
		clean(value)
	}
}

impl Sample for i16 {
	const SILENCE: Self = 0;

	fn to_f32(self) -> f32 {
		f32::from(self) / 32_768.0
	}

	fn from_f32(value: f32) -> Self {
		(clean(value) * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
	}
}

impl Sample for u16 {
	const SILENCE: Self = 32_768;

	fn to_f32(self) -> f32 {
		(f32::from(self) - 32_768.0) / 32_768.0
	}

	fn from_f32(value: f32) -> Self {
		(clean(value) * 32_768.0 + 32_768.0)
			.round()
			.clamp(0.0, 65_535.0) as u16
	}
}

impl Sample for i32 {
	const SILENCE: Self = 0;

	fn to_f32(self) -> f32 {
		(f64::from(self) / 2_147_483_648.0) as f32
	}

	fn from_f32(value: f32) -> Self {
		(f64::from(clean(value)) * 2_147_483_648.0)
			.round()
			.clamp(-2_147_483_648.0, 2_147_483_647.0) as i32
	}
}
