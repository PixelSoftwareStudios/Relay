use relay_ui::inbox::Inbox;

#[test]
fn the_avatar_inbox_is_bounded() {
	let inbox = Inbox::default();
	for i in 0..500 {
		inbox.push_avatar(format!("{i}"), None);
	}
	let drained = inbox.take_avatars();
	assert_eq!(drained.len(), 32);
	assert_eq!(drained.last().unwrap().0, "499", "newest results are kept");
	assert!(inbox.take_avatars().is_empty());
}
