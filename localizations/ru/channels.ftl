# Relay desktop UI catalog, Russian (ru), section: Channels, privacy and the connection string.
# See docs/i18n.md.

channels-heading = Каналы
channels-note = Канал определяет, кто может видеть вас и звонить вам. Он никогда не включает ваш микрофон.
channels-none = Вы не состоите ни в одном канале.
channel-listed = в списке
channel-password = пароль
channel-invite-only = по приглашениям
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } общий участник
        [few] { $peers } общих участника
        [many] { $peers } общих участников
       *[other] { $peers } общего участника
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
invite-token-note = Токен приглашения (секретный). Любой, у кого он есть, может войти, пока срок не истёк и приглашение не исчерпано:
channel-create-heading = Создать канал
channel-name-hint = Название канала
channel-listed-label = В списке: показывать название на устройствах участников
channel-password-label = Требовать пароль для входа
channel-invite-only-label = Только по приглашениям
password-hint = Пароль
channel-join-heading = Войти в канал
channel-join-note = Вставьте токен приглашения или идентификатор канала с паролем.
channel-join-hint = RLYI1… или идентификатор канала
password-optional-hint = Пароль (для приглашения не нужен)
privacy-heading = Конфиденциальность
privacy-none = Настройки конфиденциальности появятся после подключения к демону.
privacy-inbound-anyone = Все в сети
privacy-inbound-shared = Только участники моих каналов
privacy-who-rings = Кто может мне звонить
privacy-advert-visible = Видно устройствам поблизости
privacy-advert-hidden = Скрыто (доступно только по строке подключения)
privacy-visibility = Видимость в сети
invite-ttl-hour = 1 час
invite-ttl-day = 1 день
invite-ttl-week = 7 дней
connection-heading = Подключение
connection-none = Демон не сообщил строку подключения.
connection-share = Поделитесь этой строкой, чтобы вам могли позвонить с другого компьютера в вашей сети:
connection-addresses = Адреса: { $addresses } · порт { $port }
connection-fingerprint = Отпечаток { $fingerprint }
connection-warning = Любой, у кого есть эта строка, может вам позвонить; она не даёт ни доверия, ни доступа к микрофону.
connection-qr-too-long = Строка подключения слишком длинная для QR-кода.
connection-call = Позвонить по строке подключения
channel-hidden = скрыт
