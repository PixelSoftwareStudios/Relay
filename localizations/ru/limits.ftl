# Relay desktop UI catalog, ru, section: Storage and limits (limits.* settings). Same ids and $variables as localizations/en/limits.ftl. See docs/i18n.md.

settings-limits = Хранилище и лимиты
settings-limits-sub = Размеры кэшей и сроки хранения
limits-intro = Эти лимиты определяют, сколько данных хранит это устройство. Все данные остаются на нём. Значения по умолчанию подходят большинству. Изменения действуют сразу, и ничего из того, что используется, не обрезается.
limits-group-pictures = Фото профиля
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
limits-avatar-label = Кэш фото профиля
limits-avatar-help = Фото людей, с которыми вы общаетесь, хранятся здесь, чтобы быстро отображаться. Когда кэш заполнен, сначала удаляются фото, которые дольше всего не использовались; ваше собственное фото не удаляется никогда. Если уменьшить лимит, фото удаляются сразу.
limits-avatar-used = Занято: { $used } МиБ
limits-session-label = Память для текущего чата
limits-session-help = Сколько памяти может занять чат одного текущего звонка. Когда она заполнена, новые сообщения и файлы в этом чате отклоняются; то, что уже есть в чате, не удаляется.
limits-session-used = Самый большой текущий чат: { $used } МиБ
limits-retained-label = Память для завершённых чатов
limits-retained-help = Чаты завершённых звонков остаются доступными, пока Relay не закроется. Если они занимают больше этого объёма, сначала удаляются самые старые.
limits-retained-used = Завершённые чаты занимают { $used } МиБ
limits-entries-label = Звонков в истории
limits-entries-help = Сколько звонков хранит история. Если их больше, самые старые удаляются. 0 — не хранить ничего.
limits-age-label = Хранить звонки
limits-age-help = Звонки старше этого срока удаляются из истории.
limits-transfers-label = Файлов одновременно
limits-transfers-help = Сколько файлов можно одновременно отправлять или получать в одном звонке. Следующий файл отклоняется, пока не завершится один из текущих.
limits-calls-label = Завершённых чатов в памяти
limits-calls-help = Чаты стольких последних завершённых звонков хранятся в памяти, пока вы не выйдете; более старые удаляются первыми. Чаты никогда не сохраняются на диск. 0 — не хранить ничего.
limits-ringtone-size-label = Макс. размер рингтона
limits-ringtone-size-help = Самый большой аудиофайл, который можно выбрать рингтоном. Файл большего размера отклоняется.
limits-ringtone-length-label = Макс. длительность рингтона
limits-ringtone-length-help = Самая длинная запись, которую можно выбрать рингтоном. Более длинная отклоняется, а не обрезается.
limits-check-range = { $label }: допустимо от { $min } до { $max } { $unit }
limits-check-pair = Память для завершённых чатов не может быть меньше памяти для текущего чата.
limits-reset = Сбросить к значениям по умолчанию
limits-nothing = Нечего менять.
limits-applied = { $label }: применено
limits-saved = { $label }: сохранено, вступит в силу после перезапуска
limits-saved-reason = { $label }: сохранено, вступит в силу после перезапуска ({ $reason })
limits-rejected = { $label }: отклонено
limits-rejected-reason = { $label }: отклонено ({ $reason })
