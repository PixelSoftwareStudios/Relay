# Relay desktop UI catalog, English (en), section: Channels, privacy and the connection string.
# Project Fluent. English is the reference and the fallback: every id used in
# the code is defined in exactly one section file here, and a translation must
# use the same $variables. Ids are stable semantic keys, never the English text.
# See docs/i18n.md.

channels-heading = Channels
channels-none = You are not in any channel.
channel-listed = listed
channel-password = password
channel-invite-only = invite only
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } shared device
       *[other] { $peers } shared devices
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
invite-token-note = Invite code (keep it secret). Anyone who has it can join until it expires or is used up:
channel-create-heading = Create a channel
channel-name-hint = Channel name
channel-listed-label = Listed: show the name to members' devices
channel-password-label = Require a password to join
channel-invite-only-label = Invite only
password-hint = Password
channel-join-heading = Join a channel
channel-join-note = Paste an invite code, or enter a channel name or id with its password.
channel-join-hint = Invite code, channel name or id
password-optional-hint = Password (not needed for an invite)
privacy-none = Privacy settings appear once the daemon is connected.
privacy-inbound-anyone = Anyone on the network
privacy-inbound-shared = Only members of my channels
privacy-inbound-nobody = Nobody
privacy-inbound-nobody-help = Nobody can ring this device. You can still call others.
privacy-who-rings = Who can ring me
privacy-advert-visible = Visible to nearby devices
privacy-advert-hidden = Hidden (only reachable by connection string)
privacy-visibility = Network visibility
invite-ttl-hour = 1 hour
invite-ttl-day = 1 day
invite-ttl-week = 7 days
connection-none = Relay could not find an address to share. Check that it is running, then try again.
connection-share = Share this address so another device on your network can call you:
connection-addresses = Addresses: { $addresses } · port { $port }
connection-fingerprint = Fingerprint { $fingerprint }
connection-warning = Anyone with this address can call you. It does not make them trusted and gives no access to your microphone.
connection-qr-too-long = The address is too long for a QR code.
channel-hidden = hidden
channels-none-help = A channel decides who can see and call you, and never turns your microphone on. Create one, or join with an invite.
connection-copy = Copy address
connection-error-next = Check that Relay is running in the background, then try again.
connection-qr-help = Scan this code with another device to add this one.
privacy-who-rings-help = Decides who can start a call with you.
privacy-visibility-help = Hidden devices can still be called with their connection string.
privacy-messages = Messages
privacy-trusted = Trusted devices
privacy-trusted-none = You do not trust any device yet.
privacy-trusted-help = Forget a device to stop trusting it.
connection-scan = Scan a QR code
scan-wrong = That QR code is not a Relay address.
scan-camera-denied = Relay needs the camera to scan a code. Allow it and try again.
scan-camera-blocked = Camera access is turned off for Relay. Turn it on in the system settings to scan.
scan-unavailable = This device cannot scan QR codes. Copy the address instead.
scan-failed = The scan did not work. Try again, or copy the address instead.
scan-help = Scan the code shown on the other device to add it to your contacts.
scan-trusting = Connecting and trusting…
scan-added = Added { $name } to Contacts.
scan-added-unnamed = Added the device to Contacts.
scan-unreachable = Could not reach that device. Check that it is on the same network with Relay open, then scan again.
scan-mismatch = That device does not match the code, so it was not trusted.
scan-trust-failed = Could not add that device: { $reason }
privacy-blocked = Blocked devices
privacy-blocked-none = You have not blocked any device.
privacy-blocked-help = Blocked devices cannot call you and are not shown as nearby. Unblocking lets one be found again; it does not make it a contact.
nearby-heading = Channels on your network
nearby-devices = { $count ->
        [one] { $count } device
       *[other] { $count } devices
    }
nearby-none = No channels found on your network yet
nearby-none-offline = This device is offline, so it cannot see other devices.
nearby-ambiguous = Two channels use this name. Join with a code instead.
