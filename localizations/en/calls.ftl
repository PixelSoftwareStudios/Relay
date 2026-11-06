# Relay desktop UI catalog, English (en), section: Calls: phases, transmit status, trust.
# Project Fluent. English is the reference and the fallback: every id used in
# the code is defined in exactly one section file here, and a translation must
# use the same $variables. Ids are stable semantic keys, never the English text.
# See docs/i18n.md.

phase-preparing = Preparing
phase-ringing = Ringing
phase-incoming = Incoming call
phase-connecting = Connecting
phase-connected = Connected
phase-interrupted = Interrupted
phase-ending = Ending
phase-ended = Ended
phase-failed = Failed
phase-declined = Declined
phase-cancelled = Cancelled
transmit-listening = Listening only
transmit-listening-detail = Your microphone is not sent.
transmit-no-consent = Microphone not allowed
transmit-no-consent-detail = Transmit consent has not been given for this call.
transmit-releasing = Releasing
transmit-releasing-detail = The key is up; the microphone stays live briefly so the end of a sentence is not cut.
transmit-how-open = open microphone
transmit-how-key = push-to-talk key held
transmitting = Transmitting
transmit-live-detail = Microphone is live ({ $how }).
transmit-open-armed = Open mic armed, not transmitting
transmit-open-armed-detail = The engine is not sending audio right now.
transmit-ptt-armed = Push-to-talk armed
transmit-ptt-armed-detail = Waiting for a key press from an engine-side input source.
engine-reason = Engine reason: { $reason }.
trust-unknown = Unknown
trust-tofu = Seen before, not verified
trust-verified = Verified
trust-key-changed = Key changed: do not trust
trust-blocked = Blocked
call-speaking = speaking
mode-listen = Listen only
mode-ptt = Push-to-talk
mode-open = Open mic
calls-heading = Calls
meter-incoming = Incoming level from { $name }
call-accept = Accept
call-decline = Decline
call-accept-note = Accepting does not turn on your microphone.
trust-line = { $trust }.
trust-line-key = { $trust }. Device key { $fingerprint } (to tell devices apart, not to verify).
call-end = End call
call-messages-hover = Text, images and files for this call
call-unmute = Unmute (call)
call-undeafen = Undeafen (call)
call-their-volume = Their volume { $value }
call-your-mic = Your microphone in this call
call-consent = I allow my microphone to be sent to { $name }
call-mute = Mute (call)
call-deafen = Deafen (call)
call-talk-hold = Hold to talk
incoming-title = { $name } is calling
call-mic-help = Your microphone stays off until you tick the box above.
calls-sounds = Sounds
ring-help = Sounds play on this computer only.
history-heading = History
history-empty = No call history yet
history-empty-help = The calls you make and receive will be listed here.
history-off-note = Call history is off. Nothing new is recorded; the entries below stay until you clear them.
history-incoming = Incoming
history-outgoing = Outgoing
history-missed = Missed
history-declined = Declined
history-not-reached = Not reached
history-cancelled = Cancelled
history-call-back = Call back
history-show-more = Show more
history-clear = Clear history
history-clear-ask = Delete the whole call history on this device?
history-clear-yes = Clear history
history-clear-keep = Keep
history-time-now = Just now
history-time-minutes = { $count } min ago
history-time-hours = { $count } h ago
history-time-yesterday = Yesterday, { $time }
history-time-days =
    { $count ->
        [one] 1 day ago
       *[other] { $count } days ago
    }
