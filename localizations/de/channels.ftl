# Relay desktop UI catalog, German (de), section: Channels, privacy and the connection string.
# German catalog. Terms: Anruf, auflegen, stummschalten, Push-to-Talk, Gerät, Kanal, Einladung,
# Fingerabdruck, Vertrauen, Profilbild, Sitzung, Dateiangebot. See docs/i18n.md.

channels-heading = Kanäle
channels-note = Ein Kanal bestimmt, wer dich sehen und anrufen kann. Er schaltet nie dein Mikrofon ein.
channels-none = Du bist in keinem Kanal.
channel-listed = gelistet
channel-password = Passwort
channel-invite-only = nur mit Einladung
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } gemeinsames Gerät
       *[other] { $peers } gemeinsame Geräte
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
invite-token-note = Einladungscode (geheim halten). Jeder, der ihn hat, kann beitreten, bis er abläuft oder aufgebraucht ist:
channel-create-heading = Kanal erstellen
channel-name-hint = Kanalname
channel-listed-label = Gelistet: Namen auf Geräten von Mitgliedern anzeigen
channel-password-label = Passwort zum Beitreten verlangen
channel-invite-only-label = Nur mit Einladung
password-hint = Passwort
channel-join-heading = Kanal beitreten
channel-join-note = Füge einen Einladungscode oder eine Kanal-ID mit Passwort ein.
channel-join-hint = Einladungscode oder Kanal-ID
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
connection-none = Relay hat keine Adresse zum Teilen gefunden. Prüfe, ob es läuft, und versuche es erneut.
connection-share = Teile diese Adresse, damit dich ein anderes Gerät in deinem Netzwerk anrufen kann:
connection-addresses = Adressen: { $addresses } · Port { $port }
connection-fingerprint = Fingerabdruck { $fingerprint }
connection-warning = Jeder mit dieser Adresse kann dich anrufen. Sie macht niemanden vertrauenswürdig und gibt keinen Zugriff auf dein Mikrofon.
connection-qr-too-long = Die Adresse ist für einen QR-Code zu lang.
channel-hidden = verborgen
channels-none-help = Erstelle einen Kanal, um mit mehreren Personen gleichzeitig zu sprechen, oder tritt mit einer Einladung bei.
connection-copy = Adresse kopieren
connection-error-next = Prüfe, ob Relay im Hintergrund läuft, und versuche es dann erneut.
connection-qr-help = Scanne diesen Code mit einem anderen Gerät, um dieses hinzuzufügen.
privacy-who-rings-help = Legt fest, wer einen Anruf mit dir beginnen darf.
privacy-visibility-help = Verborgene Geräte können weiterhin über ihre Verbindungszeichenfolge angerufen werden.
privacy-messages = Nachrichten
privacy-trusted = Vertraute Geräte
privacy-trusted-none = Du vertraust noch keinem Gerät.
privacy-trusted-help = Vergiss ein Gerät, um ihm nicht mehr zu vertrauen.
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
scan-mismatch = Das Gerät stimmt nicht mit dem Code überein. Ihm wurde nicht vertraut.
scan-trust-failed = Das Gerät konnte nicht hinzugefügt werden: { $reason }
privacy-blocked = Blockierte Geräte
privacy-blocked-none = Du hast kein Gerät blockiert.
privacy-blocked-help = Blockierte Geräte können dich nicht anrufen und werden nicht als in der Nähe angezeigt. Beim Freigeben kann es wieder gefunden werden; es wird dadurch kein Kontakt.
