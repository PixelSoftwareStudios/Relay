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
privacy-heading = Privacy
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
connection-heading = Connection
connection-none = The daemon did not report a connection string.
connection-share = Share this string to be called from another computer on your network:
connection-addresses = Addresses: { $addresses } · port { $port }
connection-fingerprint = Fingerprint { $fingerprint }
connection-warning = Anyone with this string can ring you; it grants no trust and no microphone access.
connection-qr-too-long = The connection string is too long for a QR code.
connection-call = Call a connection string
channel-hidden = hidden
