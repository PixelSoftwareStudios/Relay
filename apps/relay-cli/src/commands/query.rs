//! Read-only commands: `status` and `peers`.

use super::Out;
use super::views::{PeerList, StatusView};
use crate::cli::CliError;
use crate::client::Control;

pub async fn status<C: Control>(client: &mut C, out: &mut Out<'_>) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	out.emit(&StatusView::new(client.info(), &snapshot))
}

pub async fn peers<C: Control>(client: &mut C, out: &mut Out<'_>) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	out.emit(&PeerList::from_snapshot(&snapshot))
}
