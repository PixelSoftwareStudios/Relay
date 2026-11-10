# Relay desktop UI catalog, German (de), section: Tray menu and tooltip.
# German catalog. Terms: Anruf, auflegen, stummschalten, Push-to-Talk, Gerät, Kanal, Einladung,
# Fingerabdruck, Vertrauen, Profilbild, Sitzung, Dateiangebot. See docs/i18n.md.

tray-mock = Relay (Testdaten)
tray-starting = Relay: Daemon wird gestartet
tray-connecting = Relay: Verbindung zum Daemon wird aufgebaut
tray-reconnecting = Relay: Verbindung zum Daemon verloren, wird wiederhergestellt
tray-offline = Relay: Daemon läuft nicht
tray-incoming =
    { $count ->
        [one] Relay: { $count } eingehender Anruf
       *[other] Relay: { $count } eingehende Anrufe
    }
tray-active =
    { $count ->
        [one] Relay: { $count } aktiver Anruf
       *[other] Relay: { $count } aktive Anrufe
    }
tray-idle = Relay: bereit
tray-show = Relay anzeigen
tray-quit = Diese App beenden (Daemon läuft weiter)
tray-status-available = Erreichbar
tray-status-unavailable = Nicht erreichbar
tray-stay-available = Im Hintergrund erreichbar bleiben
