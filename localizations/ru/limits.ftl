# Relay desktop UI catalog, ru, section: Storage and limits (limits.* settings). Same ids and $variables as localizations/en/limits.ftl. See docs/i18n.md.

settings-limits = Хранилище и лимиты
settings-limits-sub = Размеры кэшей и сроки хранения
limits-intro = Эти лимиты определяют, сколько данных хранит это устройство. Всё остаётся на этом устройстве. Значения по умолчанию подходят большинству. Изменение действует сразу, и ничего из используемого не обрезается.
limits-group-pictures = Изображения
limits-group-chats = Чаты и файлы
limits-group-history = История звонков
limits-unit-mib = МиБ
limits-unit-entries =
    { $count ->
        [one] записи
        [few] записей
        [many] записей
       *[other] записи
    }
limits-unit-days =
    { $count ->
        [one] дня
        [few] дней
        [many] дней
       *[other] дня
    }
limits-unit-transfers =
    { $count ->
        [one] передачи
        [few] передач
        [many] передач
       *[other] передачи
    }
limits-unit-calls =
    { $count ->
        [one] звонка
        [few] звонков
        [many] звонков
       *[other] звонка
    }
limits-unit-kib = КиБ
limits-unit-seconds =
    { $count ->
        [one] секунды
        [few] секунд
        [many] секунд
       *[other] секунды
    }
limits-avatar-label = Кэш изображений
limits-avatar-help = Изображения людей, с которыми вы говорите, хранятся здесь, чтобы показываться быстро. Когда кэш заполнен, сначала удаляются изображения, которыми дольше всего не пользовались; ваше собственное изображение не удаляется никогда. Если уменьшить лимит, изображения удаляются сразу.
limits-avatar-used = Занято: { $used } МиБ
limits-session-label = Память одного идущего чата
limits-session-help = Сколько памяти может занять чат одного идущего звонка. Когда она заполнена, новые сообщения и файлы в этом чате отклоняются; ничего из того, что уже есть в чате, не удаляется.
limits-session-used = Самый большой идущий чат: { $used } МиБ
limits-retained-label = Память завершённых чатов
limits-retained-help = Чаты завершённых звонков остаются доступными, пока Relay не закроется. Если они занимают больше этого, сначала удаляются самые старые завершённые чаты.
limits-retained-used = Завершённые чаты занимают { $used } МиБ
limits-entries-label = Звонков в истории
limits-entries-help = Сколько звонков хранит история. Если их больше, самые старые удаляются. 0 не хранит ничего.
limits-age-label = Хранить звонки
limits-age-help = Звонки старше этого срока удаляются из истории.
limits-transfers-label = Файлов одновременно
limits-transfers-help = Сколько файлов можно отправлять или получать одновременно в одном звонке. Лишний файл отклоняется, пока не завершится другой.
limits-calls-label = Сколько завершённых чатов хранить
limits-calls-help = Чаты стольких последних завершённых звонков хранятся в памяти, пока вы не выйдете; более старые удаляются первыми. Чаты никогда не сохраняются на диск. 0 — не хранить ничего.
limits-ringtone-size-label = Самый большой файл рингтона
limits-ringtone-size-help = Самый большой аудиофайл, который можно выбрать рингтоном. Файл больше этого отклоняется.
limits-ringtone-length-label = Самый длинный рингтон
limits-ringtone-length-help = Самая длинная запись, которую можно выбрать рингтоном. Более длинная отклоняется, а не обрезается.
limits-check-range = { $label }: допустимо от { $min } до { $max } { $unit }
limits-check-pair = Память завершённых чатов не может быть меньше памяти одного идущего чата.
limits-reset = Сбросить к значениям по умолчанию
limits-nothing = Нечего менять.
limits-applied = { $label }: применено сразу
limits-saved = { $label }: сохранено, вступит в силу после перезапуска
limits-saved-reason = { $label }: сохранено, вступит в силу после перезапуска ({ $reason })
limits-rejected = { $label }: отклонено
limits-rejected-reason = { $label }: отклонено ({ $reason })
