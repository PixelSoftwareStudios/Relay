use relay_ui::messaging::virtual_list::VirtualList;

#[test]
fn only_the_viewport_plus_overscan_is_laid_out() {
	let mut l = VirtualList::new(50.0);
	l.sync(10_000, 1);
	assert_eq!(l.total(), 500_000.0);
	let v = l.visible(0.0, 400.0, 100.0);
	assert_eq!(v.rows, 0..10);
	let v = l.visible(250_000.0, 400.0, 0.0);
	assert_eq!(v.rows, 5000..5008);
	assert_eq!(v.offset, 250_000.0);
	assert!(v.rows.len() < 20);
}

#[test]
fn measurements_move_offsets_and_total() {
	let mut l = VirtualList::new(50.0);
	l.sync(4, 1);
	assert!(l.set_height(0, 150.0));
	assert!(!l.set_height(0, 150.2), "tiny change ignored");
	assert_eq!(l.total(), 300.0);
	assert_eq!(l.offset(1), 150.0);
	let v = l.visible(160.0, 10.0, 0.0);
	assert_eq!(v.rows, 1..2);
	assert_eq!(v.offset, 150.0);
}

#[test]
fn appending_keeps_measurements_but_a_new_key_or_shrink_resets() {
	let mut l = VirtualList::new(50.0);
	l.sync(2, 1);
	l.set_height(0, 90.0);
	l.sync(3, 1);
	assert_eq!(l.total(), 190.0);
	l.sync(3, 2);
	assert_eq!(l.total(), 150.0);
	l.set_height(0, 90.0);
	l.sync(1, 2);
	assert_eq!(l.total(), 50.0);
}

#[test]
fn scrolled_past_the_end_and_empty_lists_are_safe() {
	let mut l = VirtualList::new(50.0);
	assert!(l.visible(0.0, 100.0, 0.0).rows.is_empty());
	l.sync(3, 1);
	let v = l.visible(10_000.0, 100.0, 0.0);
	assert!(v.rows.is_empty());
	assert!(!l.set_height(9, 10.0));
	assert!(!l.set_height(0, f32::NAN));
}
