# Relay desktop UI catalog, English (en), section: Tray menu and tooltip.
# Project Fluent. English is the reference and the fallback: every id used in
# the code is defined in exactly one section file here, and a translation must
# use the same $variables. Ids are stable semantic keys, never the English text.
# See docs/i18n.md.

tray-mock = Relay (mock data)
tray-starting = Relay: starting daemon
tray-connecting = Relay: connecting to daemon
tray-reconnecting = Relay: daemon connection lost, reconnecting
tray-offline = Relay: daemon not running
tray-incoming =
    { $count ->
        [one] Relay: { $count } incoming call
       *[other] Relay: { $count } incoming calls
    }
tray-active =
    { $count ->
        [one] Relay: { $count } active call
       *[other] Relay: { $count } active calls
    }
tray-idle = Relay: idle
tray-show = Show Relay
tray-quit = Quit this app (daemon keeps running)
tray-status-available = Available
tray-status-unavailable = Not available
tray-stay-available = Stay available in the background
