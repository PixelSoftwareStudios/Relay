use relay_client::meter::fill;

#[test]
fn level_maps_and_clamps() {
	assert_eq!(fill(Some(0.0)), 1.0);
	assert!((fill(Some(-30.0)) - 0.5).abs() < 1e-6);
	assert_eq!(fill(Some(-90.0)), 0.0);
	assert_eq!(fill(Some(6.0)), 1.0);
}

#[test]
fn unknown_readings_are_empty() {
	assert_eq!(fill(None), 0.0);
	assert_eq!(fill(Some(f32::NAN)), 0.0);
	assert_eq!(fill(Some(f32::INFINITY)), 0.0);
}
