# Relay desktop UI catalog, Spanish (es), section: Tray menu and tooltip.
# Translation of localizations/en/tray.ftl; same ids and $variables. See docs/i18n.md.

tray-mock = Relay (datos de prueba)
tray-starting = Relay: iniciando el daemon
tray-connecting = Relay: conectando con el daemon
tray-reconnecting = Relay: se perdió la conexión con el daemon, reconectando
tray-offline = Relay: el daemon no está en ejecución
tray-incoming =
    { $count ->
        [one] Relay: { $count } llamada entrante
        [many] Relay: { $count } de llamadas entrantes
       *[other] Relay: { $count } llamadas entrantes
    }
tray-active =
    { $count ->
        [one] Relay: { $count } llamada activa
        [many] Relay: { $count } de llamadas activas
       *[other] Relay: { $count } llamadas activas
    }
tray-idle = Relay: inactivo
tray-show = Mostrar Relay
tray-quit = Salir de esta app (el daemon sigue en ejecución)
