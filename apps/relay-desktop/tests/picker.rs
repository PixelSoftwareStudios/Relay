use relay_desktop::host::read_picked;
use relay_ui::controller::AVATAR_MAX_BYTES;

fn temp(name: &str) -> std::path::PathBuf {
	let dir = std::env::temp_dir().join(format!("relay-picker-{}", std::process::id()));
	std::fs::create_dir_all(&dir).unwrap();
	dir.join(name)
}

#[tokio::test]
async fn a_small_png_is_read() {
	let path = temp("ok.png");
	std::fs::write(&path, [1u8; 10]).unwrap();
	let file = read_picked(&path).await.unwrap();
	assert!(file.name.ends_with("ok.png"));
	assert_eq!(file.bytes.len(), 10);
}

#[tokio::test]
async fn an_oversized_file_is_refused_before_reading() {
	let path = temp("big.jpg");
	std::fs::write(&path, vec![0u8; AVATAR_MAX_BYTES + 5]).unwrap();
	let err = read_picked(&path).await.unwrap_err();
	assert!(err.contains("1 MiB"), "{err}");
}

#[tokio::test]
async fn other_types_and_missing_files_are_refused() {
	let path = temp("note.txt");
	std::fs::write(&path, b"x").unwrap();
	assert!(read_picked(&path).await.is_err());
	assert!(read_picked(&temp("missing.png")).await.is_err());
}
