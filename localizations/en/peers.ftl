# Relay desktop UI catalog, English (en), section: People nearby.
# Project Fluent. English is the reference and the fallback: every id used in
# the code is defined in exactly one section file here, and a translation must
# use the same $variables. Ids are stable semantic keys, never the English text.
# See docs/i18n.md.

peer-unknown = Unknown device
peer-online = Online
peer-unreachable = Offline
peer-key = Key { $fingerprint }
peer-shared-channels = Shared channels: { $names }
peer-call = Call
revoke-ask = Remove { $name } from { $channel }?
revoke-yes = Yes, revoke
revoke-keep = Keep
revoke-from = Revoke from { $channel }
home-nearby = Nearby
people-none-nearby = No devices found on this network yet.
people-none-nearby-help = Both devices must be on the same Wi-Fi with Relay open. Allow local network access for Relay if your system asks. Some routers block devices from seeing each other (client isolation or a guest network). If nothing helps, use Connect a device below.
people-add-title = Add a device by address
people-add-help = Paste the address shown under Connect a device on the other device.
peer-call-blocked = This device cannot be called right now.
peer-no-key = No key has been seen for this device yet.
peer-key-help = Compare this key with the one on the other device to be sure who you are talking to.
forget-device = Forget this device
forget-ask = Forget { $name }? It will no longer be trusted.
forget-yes = Yes, forget
sidebar-you = You
connect-title = Connect a device
visibility-discoverable = Discoverable
visibility-discoverable-help = Devices on the same network can see this one and call it.
visibility-private = Private
visibility-private-help = This device is hidden on the network. Only someone with your address can call you.
visibility-not-advertising = Not advertising
visibility-not-advertising-reason = Not advertising: { $reason }
visibility-go-private = Go private
visibility-go-discoverable = Make discoverable
people-found-help = These devices are on your network but you have not trusted them yet. Trust one only after you have checked who it is.
found-trust = Trust this device
found-trust-ask = Before you trust { $name }, compare this fingerprint with the one shown on that device (Connect a device, then Details). Trust it only if they are the same.
found-trust-yes = Yes, trust it
found-trust-cancel = Not now
found-subtitle = Not trusted yet · { $fingerprint }
contacts-empty-title = No contacts yet
contacts-empty-help = A contact is a device you have trusted. Find one under Connect and trust it, then it shows up here.
contacts-go-connect = Go to Connect
contact-more = More options
peer-addresses = Addresses: { $addresses }
contact-last-seen = Last seen { $when }
peer-port = Port { $port }
contact-favorite-add = Add to favorites
contact-favorite-remove = Remove from favorites

# Key changed: a trusted device shows another security key.
contact-key-changed-label = Security key changed
keychange-title = { $name }'s security key changed
keychange-body = This usually happens after Relay was reinstalled or reset on that device. Calls are blocked until you verify it again.
keychange-scan = Scan their QR code
keychange-compare = Compare and trust new key
keychange-remove = Remove contact
keychange-remove-ask = Remove { $name } from your contacts? It will no longer be trusted.
keychange-remove-yes = Yes, remove
keychange-compare-intro = On { $name }, open Connect, then Connect a device, then Details. Type or paste the fingerprint it shows. Trust the new key only if both are the same.
keychange-new-key = New key received from this device
keychange-field-hint = Fingerprint from the other device
keychange-match = They match.
keychange-no-match = These digits are not the same yet.
keychange-confirm = Trust new key
keychange-cancel = Not now
keychange-previous = Previous key { $fingerprint }
found-call-help = Calling a nearby device adds it to Contacts as unverified once it answers. To compare its fingerprint first, use Trust this device.
block-device = Block device
block-ask = Block { $name }? It can no longer call you or see you, and it is removed from Contacts. You can unblock it in Settings.
block-yes = Block
unblock = Unblock
