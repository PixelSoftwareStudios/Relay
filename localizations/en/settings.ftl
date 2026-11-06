# Relay desktop UI catalog, English (en), section: Settings: this app, hotkey, profile, daemon, language.
# Project Fluent. English is the reference and the fallback: every id used in
# the code is defined in exactly one section file here, and a translation must
# use the same $variables. Ids are stable semantic keys, never the English text.
# See docs/i18n.md.

link-starting = Starting the Relay daemon
link-connecting = Connecting to the Relay daemon
link-connected = Connected
link-reconnecting = Daemon connection lost, reconnecting
link-offline = Daemon not running
avatar-size-refusal = avatar must be 1 byte to 1 MiB (PNG, JPEG, WebP, GIF, BMP or TIFF)
hotkey-note = This app registers no global hotkey and has no on-screen talk button yet. Push-to-talk only transmits while an engine-side input source (for example the CLI) holds the key.
close-tray = Closing the window hides it to the tray; calls keep going.
close-no-tray = There is no tray icon on this platform, so closing the window quits this app.
quit-note = Quitting this app hangs up the calls it started or accepted and leaves the Relay daemon running. This app cannot stop the daemon.
calls-active =
    { $count ->
        [one] One call is active.
       *[other] { $count } calls are active.
    }
quit-prompt = Quit this app?
quit-prompt-calls =
    { $count ->
        [one] Quit this app? Your active call will be hung up.
       *[other] Quit this app? Your { $count } active calls will be hung up.
    }
daemon-stopped-again = It stopped again right after starting.
daemon-keeps-stopping = The Relay daemon keeps stopping, so it is not restarted again automatically. { $why }
avatar-bad-format = avatar rejected: unsupported or unrecognised image format (use PNG, JPEG, WebP, GIF, BMP or TIFF)
avatar-zero-size = avatar rejected: avatar has zero width or height
avatar-too-large = avatar rejected: avatar dimensions { $width }x{ $height } exceed the { $limit } pixel limit
avatar-undecodable = avatar rejected: avatar could not be decoded: { $error }
daemon-started = The Relay daemon started.
daemon-already-running = The Relay daemon is already running.
daemon-not-found = Cannot find the Relay daemon ({ $binary }). Install it next to this app or on your PATH, or set { $env } to its location.
daemon-start-failed = The Relay daemon did not start: { $reason }.
daemon-start-failed-log = The Relay daemon did not start: { $reason }. Last log line: { $last }
daemon-stop-failed = The Relay daemon did not stop: { $error }
daemon-stopped = The Relay daemon stopped.
notify-off = Off: a call that rings while this window is hidden or in the background is only shown in the tray tooltip and heard from the daemon's ringtone.
notify-on = Shows the caller's name (never message text) when a call rings while this window is hidden or not in front.
notify-refused = The system refused the last notification ({ $reason }). Calls still ring through the daemon and show in this window and the tray tooltip.
login-unsupported = Not available here: { $reason }
login-off = Off. When on, this app is registered with the system and opens its window at login. The daemon is not started until the app needs it.
login-on = On: the system starts this app when you log in (it opens its window). Turn it off here to remove the entry.
login-failed = Could not read or change the login item: { $reason }
banner-mock = Mock data: this view is not connected to a Relay daemon.
link-offline-auto = Starting it automatically.
daemon-starting-button = Starting the daemon...
daemon-start-button = Start daemon
app-hide = Hide window
app-quit = Quit this app
app-hang-up-quit = Hang up and quit
autostart-label = Start the daemon automatically
autostart-hover = When the daemon is not running, Relay apps start it. It advertises on the local network only while an app is open.
daemon-heading = Relay daemon
daemon-not-connected = The daemon is not connected.
daemon-paused = Automatic start is paused until you press Start daemon.
daemon-keeps-running = The daemon keeps running when you close this app. Quitting it ends all calls and releases the microphone.
daemon-quit-ask = Quit daemon...
daemon-quit-confirm = Quit the daemon now? Active calls will end.
daemon-quit = Quit daemon
profile-name-hint = Display name
profile-save-name = Save name
profile-image-note = PNG, JPEG, WebP, GIF, BMP or TIFF, up to 1 MiB and 2048 x 2048. The engine scales it and shares it with your trusted peers. Removing it tells them to stop showing it.
profile-choose-image = Choose image…
profile-upload = Upload
profile-discard = Discard
profile-remove = Remove picture
hotkey-heading = Push-to-talk hotkey
hotkey-intro = Hold this key anywhere to talk while a call is in push-to-talk mode.
hotkey-field = Hotkey
hotkey-hint = Ctrl+Alt+Space (empty turns it off)
hotkey-nothing = Nothing to change.
notify-label = Notify on incoming calls
login-label = Start this app at login
login-error = Start at login: { $reason }
dialog-profile-image = Choose a profile image
dialog-image-filter = Image
dialog-export = Export the transcript
login-no-mechanism = this system has no supported login-item mechanism
login-no-exe = cannot find this program: { $error }
login-bad-path = the program path is not valid text
login-set-unavailable = not available: { $reason }
options-save-failed = the choice could not be saved: { $error }
language-label = Language
language-system = System ({ $language })
language-experimental = This language is experimental: the text renderer cannot join Devanagari letters, so some words look different from print.
link-offline-manual = Start the daemon; this window reconnects by itself.
hotkey-applied = { $label }: applied now
hotkey-check-accelerator = the hotkey must be at most 64 printable characters
hotkey-check-delay = release delay must be between 0 and 2000 ms
hotkey-label-accelerator = Push-to-talk hotkey
hotkey-label-delay = Release delay
hotkey-bad-value = the daemon reported a bad { $key }: { $value }
hotkey-rejected = { $label }: rejected
hotkey-rejected-reason = { $label }: rejected ({ $reason })
hotkey-saved = { $label }: saved, takes effect after a restart
hotkey-saved-reason = { $label }: saved, takes effect after a restart ({ $reason })
hotkey-note-button = This device has no global hotkey. On a call in push-to-talk mode, hold the Talk button to speak.
close-background = Leaving this app keeps your calls going in the background.
settings-profile = Profile
settings-profile-sub = Your name and picture
settings-audio = Audio
settings-audio-sub = Microphone, speakers and volume
settings-ptt = Push-to-talk
settings-ptt-sub = Hotkey and release delay
settings-calls = Calls
settings-calls-sub = Who can ring you and call sounds
settings-privacy = Privacy
settings-privacy-sub = Visibility, messages and trusted devices
settings-language = Language
settings-language-sub = The language of this app
settings-startup = Startup
settings-startup-sub = Notifications and start at login
settings-advanced = Advanced
settings-advanced-sub = Daemon and about
hotkey-note-global = Hold the hotkey anywhere to talk while a call is in push-to-talk mode.
profile-name-label = Display name
profile-name-help = Other people see this name when you call or message them.
profile-remove-ask = Remove your picture?
profile-remove-yes = Yes, remove
ptt-delay-help = How long the microphone stays open after you let go.
language-help = The app switches at once.
daemon-what = The daemon is the background program that makes the calls. This app is its window.
advanced-about = About
advanced-app-version = Relay { $version }
advanced-api = API { $major }.{ $minor }
advanced-counts = Devices { $peers } · Calls { $calls } · Channels { $channels }
advanced-revision = Revision { $revision }
history-setting-label = Keep call history
history-setting-help = Remembers who called whom, how each call ended and how long it lasted, on this device only. It is never sent to the other person and can be cleared at any time. When this is off, nothing new is recorded and existing entries stay until you clear them.
setting-unsupported = Not supported by the running background service: restart it.
