# Relay desktop UI catalog, English (en), section: People nearby.
# Project Fluent. English is the reference and the fallback: every id used in
# the code is defined in exactly one section file here, and a translation must
# use the same $variables. Ids are stable semantic keys, never the English text.
# See docs/i18n.md.

peer-unknown = Unknown peer
peer-online = Online
peer-unreachable = Not reachable
peer-key = Key { $fingerprint }
peer-shared-channels = Shared channels: { $names }
peer-call = Call
revoke-ask = Remove { $name } from { $channel }?
revoke-yes = Yes, revoke
revoke-keep = Keep
revoke-from = Revoke from { $channel }
home-nearby = Nearby
home-empty-title = Nobody to call yet
home-empty-body = Relay finds devices on your local network by itself. If nobody shows up, add a device by its connection string.
home-add-by-address = Add a device
people-none-nearby = No devices found on this network yet.
people-none-nearby-help = Both devices must be on the same Wi-Fi with Relay open. Allow local network access for Relay if your system asks. Some routers block devices from seeing each other (client isolation or a guest network). If nothing helps, use Connect a device above.
people-known = Other trusted devices
people-known-help = You have trusted these before, but they are not on this network now.
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
