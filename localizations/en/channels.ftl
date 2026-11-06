# Relay desktop UI catalog, English (en), section: Channels, privacy and the connection string.
# Project Fluent. English is the reference and the fallback: every id used in
# the code is defined in exactly one section file here, and a translation must
# use the same $variables. Ids are stable semantic keys, never the English text.
# See docs/i18n.md.

channels-heading = Channels
channels-note = A channel decides who can see and call you. It never turns your microphone on.
channels-none = You are not in any channel.
channel-listed = listed
channel-password = password
channel-invite-only = invite only
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } shared peer
       *[other] { $peers } shared peers
    }
channel-invite = Invite…
channel-leave-yes = Yes, leave
channel-leave-stay = Stay
channel-leave = Leave
invite-uses = { $count ->
        [one] { $count } use
       *[other] { $count } uses
    }
invite-create = Create invite
invite-token-note = Invite token (secret). Anyone who has it can join until it expires or is used up:
channel-create-heading = Create a channel
channel-name-hint = Channel name
channel-listed-label = Listed: show the name to members' devices
channel-password-label = Require a password to join
channel-invite-only-label = Invite only
password-hint = Password
channel-join-heading = Join a channel
channel-join-note = Paste an invite token, or a channel id with its password.
channel-join-hint = RLYI1… or channel id
password-optional-hint = Password (not needed for an invite)
privacy-none = Privacy settings appear once the daemon is connected.
privacy-inbound-anyone = Anyone on the network
privacy-inbound-shared = Only members of my channels
privacy-who-rings = Who can ring me
privacy-advert-visible = Visible to nearby devices
privacy-advert-hidden = Hidden (only reachable by connection string)
privacy-visibility = Network visibility
invite-ttl-hour = 1 hour
invite-ttl-day = 1 day
invite-ttl-week = 7 days
connection-none = The daemon did not report a connection string.
connection-share = Share this string to be called from another computer on your network:
connection-addresses = Addresses: { $addresses } · port { $port }
connection-fingerprint = Fingerprint { $fingerprint }
connection-warning = Anyone with this string can ring you; it grants no trust and no microphone access.
connection-qr-too-long = The connection string is too long for a QR code.
channel-hidden = hidden
channels-none-help = Create a channel to talk with several people at once, or join one with an invite.
connection-copy = Copy address
connection-error-next = Check that the daemon is running, then try again.
connection-qr-help = Scan this code with another device to call you.
privacy-who-rings-help = Decides who can start a call with you.
privacy-visibility-help = Hidden devices can still be called with their connection string.
privacy-messages = Messages
privacy-trusted = Trusted devices
privacy-trusted-none = You do not trust any device yet.
privacy-trusted-help = Forget a device to stop trusting it.
