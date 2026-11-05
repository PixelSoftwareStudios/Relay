# Relay desktop UI catalog, Russian (ru), section: Tray menu and tooltip.
# See docs/i18n.md.

tray-mock = Relay (тестовые данные)
tray-starting = Relay: запуск демона
tray-connecting = Relay: подключение к демону
tray-reconnecting = Relay: связь с демоном потеряна, переподключение
tray-offline = Relay: демон не запущен
tray-incoming =
    { $count ->
        [one] Relay: { $count } входящий звонок
        [few] Relay: { $count } входящих звонка
        [many] Relay: { $count } входящих звонков
       *[other] Relay: { $count } входящего звонка
    }
tray-active =
    { $count ->
        [one] Relay: { $count } активный звонок
        [few] Relay: { $count } активных звонка
        [many] Relay: { $count } активных звонков
       *[other] Relay: { $count } активного звонка
    }
tray-idle = Relay: ожидание
tray-show = Показать Relay
tray-quit = Выйти из приложения (демон продолжит работу)
