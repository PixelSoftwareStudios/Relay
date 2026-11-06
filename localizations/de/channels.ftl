# Relay desktop UI catalog, German (de), section: Channels, privacy and the connection string.
# German catalog. Terms: Anruf, auflegen, stummschalten, Push-to-Talk, Peer, Kanal, Einladung,
# Fingerabdruck, Vertrauen, Profilbild, Sitzung, Dateiangebot. See docs/i18n.md.

channels-heading = Kanäle
channels-note = Ein Kanal bestimmt, wer dich sehen und anrufen kann. Er schaltet nie dein Mikrofon ein.
channels-none = Du bist in keinem Kanal.
channel-listed = gelistet
channel-password = Passwort
channel-invite-only = nur mit Einladung
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } gemeinsamer Peer
       *[other] { $peers } gemeinsame Peers
    }
channel-invite = Einladen…
channel-leave-yes = Ja, verlassen
channel-leave-stay = Bleiben
channel-leave = Verlassen
invite-uses = { $count ->
        [one] { $count } Verwendung
       *[other] { $count } Verwendungen
    }
invite-create = Einladung erstellen
invite-token-note = Einladungstoken (geheim). Jeder, der es hat, kann beitreten, bis es abläuft oder aufgebraucht ist:
channel-create-heading = Kanal erstellen
channel-name-hint = Kanalname
channel-listed-label = Gelistet: Namen auf Geräten von Mitgliedern anzeigen
channel-password-label = Passwort zum Beitreten verlangen
channel-invite-only-label = Nur mit Einladung
password-hint = Passwort
channel-join-heading = Kanal beitreten
channel-join-note = Füge ein Einladungstoken oder eine Kanal-ID mit Passwort ein.
channel-join-hint = RLYI1… oder Kanal-ID
password-optional-hint = Passwort (bei Einladung nicht nötig)
privacy-none = Die Datenschutzeinstellungen erscheinen, sobald der Daemon verbunden ist.
privacy-inbound-anyone = Jeder im Netzwerk
privacy-inbound-shared = Nur Mitglieder meiner Kanäle
privacy-who-rings = Wer mich anrufen darf
privacy-advert-visible = Für Geräte in der Nähe sichtbar
privacy-advert-hidden = Verborgen (nur über Verbindungszeichenfolge erreichbar)
privacy-visibility = Sichtbarkeit im Netzwerk
invite-ttl-hour = 1 Stunde
invite-ttl-day = 1 Tag
invite-ttl-week = 7 Tage
connection-none = Der Daemon hat keine Verbindungszeichenfolge gemeldet.
connection-share = Teile diese Zeichenfolge, um von einem anderen Computer in deinem Netzwerk angerufen zu werden:
connection-addresses = Adressen: { $addresses } · Port { $port }
connection-fingerprint = Fingerabdruck { $fingerprint }
connection-warning = Jeder mit dieser Zeichenfolge kann bei dir anrufen; sie gewährt weder Vertrauen noch Mikrofonzugriff.
connection-qr-too-long = Die Verbindungszeichenfolge ist für einen QR-Code zu lang.
channel-hidden = verborgen
channels-none-help = Erstellen Sie einen Kanal, um mit mehreren Personen gleichzeitig zu sprechen, oder treten Sie mit einer Einladung bei.
connection-copy = Adresse kopieren
connection-error-next = Prüfen Sie, ob der Daemon läuft, und versuchen Sie es dann erneut.
connection-qr-help = Scannen Sie diesen Code mit einem anderen Gerät, damit es Sie anrufen kann.
privacy-who-rings-help = Legt fest, wer einen Anruf mit Ihnen beginnen darf.
privacy-visibility-help = Versteckte Geräte können weiterhin über ihre Verbindungszeichenfolge angerufen werden.
privacy-messages = Nachrichten
privacy-trusted = Vertraute Geräte
privacy-trusted-none = Sie vertrauen noch keinem Gerät.
privacy-trusted-help = Vergessen Sie ein Gerät, um ihm nicht mehr zu vertrauen.
connection-scan = QR-Code scannen
scan-wrong = Dieser QR-Code ist keine Relay-Adresse.
scan-camera-denied = Relay braucht die Kamera, um einen Code zu scannen. Erlaube sie und versuche es erneut.
scan-camera-blocked = Der Kamerazugriff ist für Relay ausgeschaltet. Schalte ihn in den Systemeinstellungen ein, um zu scannen.
scan-unavailable = Dieses Gerät kann keine QR-Codes scannen. Kopiere stattdessen die Adresse.
scan-failed = Das Scannen hat nicht geklappt. Versuche es erneut oder kopiere stattdessen die Adresse.
scan-help = Scanne den Code auf dem anderen Gerät, um es zu deinen Kontakten hinzuzufügen.
scan-trusting = Verbinde und vertraue…
scan-added = { $name } wurde zu den Kontakten hinzugefügt.
scan-added-unnamed = Das Gerät wurde zu den Kontakten hinzugefügt.
scan-unreachable = Das Gerät ist nicht erreichbar. Prüfe, ob es im selben Netzwerk ist und Relay geöffnet hat, und scanne erneut.
scan-mismatch = Das Gerät stimmt nicht mit dem Code überein (sein Fingerabdruck ist anders). Ihm wurde nicht vertraut.
scan-trust-failed = Das Gerät konnte nicht hinzugefügt werden: { $reason }
