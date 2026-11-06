# Relay desktop UI catalog, English (en), section: Errors the UI words itself.
# Project Fluent. English is the reference and the fallback: every id used in
# the code is defined in exactly one section file here, and a translation must
# use the same $variables. Ids are stable semantic keys, never the English text.
# See docs/i18n.md.

err-not-connected = not connected to the Relay daemon
err-release-delay = release delay must be between 0 and 2000 ms
err-gain = gain must be a finite number between 0 and 4
err-bad-call-id = malformed call id
err-unknown-call = unknown call
err-no-lease = the engine returned no control lease
err-bad-peer-id = malformed peer id
err-unknown-peer = unknown peer
err-ring-volume = ringtone volume must be between 0 and 1
err-unknown-device = unknown or unavailable audio device
err-display-name = display name must be 1 to 64 printable characters
err-avatar-media-type = avatar needs a media type
conn-scheme = A connection string starts with relay://
err-invite-malformed = that invite token is malformed
err-join-target = enter an invite token or a 32-character channel id
err-bad-channel-id = malformed channel id
err-unknown-channel = unknown channel
err-channel-name = channel name must be 1 to 32 printable characters
err-channel-needs-password = a password-protected channel needs a password
err-enter-password = enter the channel password
err-invite-admin-only = only an owner or admin can create invites
err-invite-range = invite lifetime or use count is out of range
err-no-invite = the engine returned no invite
err-revoke-admin-only = only an owner or admin can revoke members
err-not-member = that peer is not a proven member of the channel
err-nothing-to-change = nothing to change
err-bad-avatar-hash = malformed avatar hash
conn-missing-fingerprint = The connection string is missing its #fingerprint part.
conn-bad-fingerprint = The fingerprint must be 64 hexadecimal characters.
conn-bad-address = The address must be a literal ip:port, not a name.
watch-resync = engine asked for a resync: { $reason }
watch-closed = the daemon closed the connection
err-unsupported = this engine does not support that request
err-daemon-unreachable = the Relay daemon is not reachable: { $reason }
err-engine-timeout = the engine did not answer in time
err-connection = connection problem: { $reason }
err-unreadable-image = That file is not a readable image.
err-engine-rejected = the engine rejected the request
err-no-paths = This device cannot choose files or folders for the Relay daemon.
err-cannot-start-daemon = This device cannot start the Relay daemon.
err-cannot-stop-daemon = This device cannot stop the Relay daemon.
err-read-file = Cannot read the file: { $error }
err-fingerprint-mismatch = That is not the fingerprint of this device's new key.
err-trust-full = You have reached the limit of { $limit } saved devices. Remove some from Contacts.
err-save-failed = Relay could not save this change. Try again, or restart the app.
