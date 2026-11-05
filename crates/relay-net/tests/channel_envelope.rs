use relay_net::control::{new_envelope, validate_envelope};
use relay_net::proto::channel_message::Body as ChannelBody;
use relay_net::proto::envelope::Body;
use relay_net::proto::{ChannelJoinConfirm, ChannelJoinDenied, ChannelMessage};

fn channel(body: ChannelBody) -> Body {
	Body::Channel(ChannelMessage { body: Some(body) })
}

#[test]
fn channel_messages_ride_in_envelopes_and_are_validated() {
	let ok = new_envelope(channel(ChannelBody::JoinDenied(ChannelJoinDenied {})));
	assert!(validate_envelope(&ok).is_ok());

	let short = new_envelope(channel(ChannelBody::JoinConfirm(ChannelJoinConfirm {
		confirmation: vec![0; 5],
	})));
	assert!(validate_envelope(&short).is_err());

	let empty = new_envelope(Body::Channel(ChannelMessage { body: None }));
	assert!(validate_envelope(&empty).is_err());
}
