# Relay desktop UI catalog, Russian (ru), section: Channels, privacy and the connection string.
# See docs/i18n.md.

channels-heading = Каналы
channels-none = Вы не состоите ни в одном канале.
channels-yours = Ваши каналы
channel-listed = в списке
channel-password = пароль
channel-invite-only = по приглашению
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
invite-token-note = Код приглашения (держите его в секрете). Любой, у кого он есть, может вступить в канал, пока код действует и не исчерпан:
channel-create-heading = Создать канал
channel-name-hint = Название канала
channel-listed-label = В списке: показывать название на устройствах участников
channel-password-label = Требовать пароль для входа
channel-invite-only-label = Только по приглашению
password-hint = Пароль
channel-join-heading = Вступить в канал
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
channels-none-help = Канал определяет, кто может видеть вас и звонить вам, и никогда не включает ваш микрофон. Создайте канал или вступите в него по приглашению.
connection-copy = Копировать адрес
connection-error-next = Убедитесь, что Relay работает в фоне, и повторите попытку.
connection-qr-help = Отсканируйте этот код на другом устройстве, чтобы добавить это.
privacy-who-rings-help = Определяет, кто может вам позвонить.
privacy-visibility-help = Скрытому устройству всё равно можно позвонить по строке подключения.
privacy-messages = Сообщения
privacy-trusted = Доверенные устройства
privacy-trusted-none = Вы пока не доверяете ни одному устройству.
privacy-trusted-help = Забудьте устройство, чтобы перестать ему доверять.
connection-scan = Сканировать QR-код
scan-wrong = Этот QR-код не является адресом Relay.
scan-camera-denied = Чтобы сканировать код, Relay нужен доступ к камере. Разрешите его и попробуйте снова.
scan-camera-blocked = Доступ к камере для Relay выключен. Включите его в настройках системы, чтобы сканировать.
scan-unavailable = Это устройство не может сканировать QR-коды. Скопируйте адрес.
scan-failed = Сканирование не удалось. Попробуйте снова или скопируйте адрес.
scan-help = Отсканируйте код на другом устройстве, чтобы добавить его в контакты.
scan-trusting = Подключение и добавление в доверенные…
scan-added = Добавлено в контакты: { $name }.
scan-added-unnamed = Устройство добавлено в контакты.
scan-unreachable = Не удалось связаться с этим устройством. Убедитесь, что оно в той же сети и на нём открыт Relay, и отсканируйте снова.
scan-mismatch = Устройство не соответствует коду, поэтому оно не добавлено в доверенные.
scan-trust-failed = Не удалось добавить устройство: { $reason }
privacy-blocked = Заблокированные устройства
privacy-blocked-none = Вы не заблокировали ни одного устройства.
privacy-blocked-help = Заблокированные устройства не могут вам звонить и не показываются рядом. Разблокированное устройство снова можно будет найти, но контактом оно не станет.
nearby-heading = Каналы в вашей сети
nearby-devices = { $count ->
        [one] { $count } устройство
        [few] { $count } устройства
        [many] { $count } устройств
       *[other] { $count } устройства
    }
nearby-none = В вашей сети пока не найдено каналов
nearby-none-offline = Это устройство не в сети и не видит другие устройства.
nearby-ambiguous = Это название носят два канала. Вступите по коду.
nearby-needs-password = Нужен пароль
channel-invite-unlisted = Каналы «только по приглашению» никогда не показываются в сети.
