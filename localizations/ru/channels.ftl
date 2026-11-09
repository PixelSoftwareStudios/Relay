# Relay desktop UI catalog, Russian (ru), section: Channels, privacy and the connection string.
# See docs/i18n.md.

channels-heading = Каналы
channels-none = Вы не состоите ни в одном канале.
channel-listed = в списке
channel-password = пароль
channel-invite-only = по приглашениям
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } общее устройство
        [few] { $peers } общих устройства
        [many] { $peers } общих устройств
       *[other] { $peers } общего устройства
    }
channel-invite = Пригласить…
channel-leave-yes = Да, выйти
channel-leave-stay = Остаться
channel-leave = Выйти
invite-uses = { $count ->
        [one] { $count } использование
        [few] { $count } использования
        [many] { $count } использований
       *[other] { $count } использования
    }
invite-create = Создать приглашение
invite-token-note = Код приглашения (держите его в секрете). Любой, у кого он есть, может войти, пока срок не истёк и приглашение не исчерпано:
channel-create-heading = Создать канал
channel-name-hint = Название канала
channel-listed-label = В списке: показывать название на устройствах участников
channel-password-label = Требовать пароль для входа
channel-invite-only-label = Только по приглашениям
password-hint = Пароль
channel-join-heading = Войти в канал
channel-join-note = Вставьте код приглашения или введите название либо идентификатор канала с паролем.
channel-join-hint = Код приглашения, название или идентификатор канала
password-optional-hint = Пароль (для приглашения не нужен)
privacy-none = Настройки конфиденциальности появятся после подключения к демону.
privacy-inbound-anyone = Все в сети
privacy-inbound-shared = Только участники моих каналов
privacy-inbound-nobody = Никто
privacy-inbound-nobody-help = Никто не может позвонить на это устройство. Вы по-прежнему можете звонить другим.
privacy-who-rings = Кто может мне звонить
privacy-advert-visible = Видно устройствам рядом
privacy-advert-hidden = Скрыто (доступно только по строке подключения)
privacy-visibility = Видимость в сети
invite-ttl-hour = 1 час
invite-ttl-day = 1 день
invite-ttl-week = 7 дней
connection-none = Relay не нашёл адрес, которым можно поделиться. Убедитесь, что он запущен, и повторите попытку.
connection-share = Поделитесь этим адресом, чтобы вам могли позвонить с другого устройства в вашей сети:
connection-addresses = Адреса: { $addresses } · порт { $port }
connection-fingerprint = Отпечаток { $fingerprint }
connection-warning = Любой, у кого есть этот адрес, может вам позвонить. Это не делает его доверенным и не даёт доступа к микрофону.
connection-qr-too-long = Адрес слишком длинный для QR-кода.
channel-hidden = скрыт
channels-none-help = Канал определяет, кто может видеть вас и звонить вам, и никогда не включает ваш микрофон. Создайте канал или присоединитесь по приглашению.
connection-copy = Копировать адрес
connection-error-next = Убедитесь, что Relay работает в фоне, и повторите попытку.
connection-qr-help = Отсканируйте этот код другим устройством, чтобы добавить это устройство.
privacy-who-rings-help = Определяет, кто может вам позвонить.
privacy-visibility-help = Скрытому устройству всё равно можно позвонить по строке подключения.
privacy-messages = Сообщения
privacy-trusted = Доверенные устройства
privacy-trusted-none = Вы пока не доверяете ни одному устройству.
privacy-trusted-help = Забудьте устройство, чтобы перестать ему доверять.
connection-scan = Сканировать QR-код
scan-wrong = Этот QR-код не является адресом Relay.
scan-camera-denied = Relay нужна камера, чтобы сканировать код. Разрешите доступ и попробуйте снова.
scan-camera-blocked = Доступ к камере для Relay выключен. Включите его в настройках системы, чтобы сканировать.
scan-unavailable = Это устройство не умеет сканировать QR-коды. Скопируйте адрес.
scan-failed = Сканирование не удалось. Попробуйте снова или скопируйте адрес.
scan-help = Отсканируйте код на другом устройстве, чтобы добавить его в контакты.
scan-trusting = Подключение и установка доверия…
scan-added = Добавлено в контакты: { $name }.
scan-added-unnamed = Устройство добавлено в контакты.
scan-unreachable = Не удалось связаться с этим устройством. Проверьте, что оно в той же сети и Relay открыт, и отсканируйте снова.
scan-mismatch = Это устройство не совпадает с кодом, поэтому доверие не установлено.
scan-trust-failed = Не удалось добавить устройство: { $reason }
privacy-blocked = Заблокированные устройства
privacy-blocked-none = Вы не блокировали ни одно устройство.
privacy-blocked-help = Заблокированные устройства не могут вам звонить и не показываются рядом. После разблокировки его снова можно найти, но контактом оно не станет.
nearby-heading = Каналы в вашей сети
nearby-devices = { $count ->
        [one] { $count } устройство
        [few] { $count } устройства
        [many] { $count } устройств
       *[other] { $count } устройства
    }
nearby-none = В вашей сети пока не найдено каналов
nearby-none-offline = Это устройство не в сети и не видит другие устройства.
nearby-ambiguous = Два канала используют это название. Войдите по коду.
