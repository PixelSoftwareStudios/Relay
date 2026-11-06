# Relay desktop UI catalog, Russian (ru), section: Calls: phases, transmit status, trust.
# See docs/i18n.md.

phase-preparing = Подготовка
phase-ringing = Вызов
phase-incoming = Входящий звонок
phase-connecting = Соединение
phase-connected = Соединено
phase-interrupted = Прервано
phase-ending = Завершение
phase-ended = Завершено
phase-failed = Ошибка
phase-declined = Отклонено
phase-cancelled = Отменено
transmit-listening = Только прослушивание
transmit-listening-detail = Ваш микрофон не передаётся.
transmit-no-consent = Микрофон не разрешён
transmit-no-consent-detail = Для этого звонка не дано разрешение на передачу звука.
transmit-releasing = Отпускание
transmit-releasing-detail = Клавиша отпущена; микрофон ещё немного остаётся включённым, чтобы не обрезать конец фразы.
transmit-how-open = открытый микрофон
transmit-how-key = клавиша рации удерживается
transmitting = Передача
transmit-live-detail = Микрофон включён ({ $how }).
transmit-open-armed = Открытый микрофон готов, передачи нет
transmit-open-armed-detail = Движок сейчас не отправляет звук.
transmit-ptt-armed = Рация готова
transmit-ptt-armed-detail = Ожидание нажатия клавиши от источника ввода на стороне движка.
engine-reason = Причина от движка: { $reason }.
trust-unknown = Неизвестно
trust-tofu = Встречался раньше, не проверен
trust-verified = Проверен
trust-key-changed = Ключ изменился: не доверяйте
trust-blocked = Заблокирован
call-speaking = говорит
mode-listen = Только слушать
mode-ptt = Рация
mode-open = Открытый микрофон
calls-heading = Звонки
meter-incoming = Входящий уровень от { $name }
call-accept = Принять
call-decline = Отклонить
call-accept-note = Приём звонка не включает ваш микрофон.
trust-line = { $trust }.
trust-line-key = { $trust }. Ключ устройства { $fingerprint } (чтобы различать устройства, а не для проверки).
call-end = Завершить звонок
call-messages-hover = Текст, изображения и файлы этого звонка
call-messages = Сообщения
call-messages-unread = Сообщения ({ $count })
history-messages-count = Сообщения: { $count }
history-messages-gone = Сообщения этого звонка больше не хранятся
call-unmute = Включить микрофон (звонок)
call-undeafen = Включить звук (звонок)
call-their-volume = Их громкость { $value }
call-your-mic = Ваш микрофон в этом звонке
call-consent = Я разрешаю передавать мой микрофон участнику { $name }
call-mute = Отключить микрофон (звонок)
call-deafen = Отключить звук (звонок)
call-talk-hold = Удерживайте, чтобы говорить
incoming-title = Звонит { $name }
call-mic-help = Микрофон остается выключенным, пока вы не отметите флажок выше.
calls-sounds = Звуки
ring-help = Звуки воспроизводятся только на этом компьютере.
history-heading = История
history-empty = Истории звонков пока нет
history-empty-help = Здесь будут показаны звонки, которые вы совершаете и принимаете.
history-off-note = История звонков выключена. Новые звонки не записываются; записи ниже остаются, пока вы их не удалите.
history-incoming = Входящий
history-outgoing = Исходящий
history-missed = Пропущенный
history-declined = Отклонённый
history-not-reached = Не дозвонились
history-cancelled = Отменён
history-call-back = Перезвонить
history-show-more = Показать ещё
history-clear = Очистить историю
history-clear-ask = Удалить всю историю звонков на этом устройстве?
history-clear-yes = Очистить историю
history-clear-keep = Оставить
history-time-now = Только что
history-time-minutes = { $count } мин назад
history-time-hours = { $count } ч назад
history-time-yesterday = Вчера, { $time }
history-time-days =
    { $count ->
        [one] { $count } день назад
        [few] { $count } дня назад
       *[many] { $count } дней назад
    }

mic-ask-title = Relay нужен ваш микрофон
mic-ask-body = Разрешите доступ к микрофону, чтобы собеседник слышал вас во время звонков.
mic-allow = Разрешить микрофон
mic-off-title = Доступ к микрофону выключен
mic-off-body = Без него вас никто не услышит в звонке. Включите его в настройках системы и вернитесь сюда.
mic-open-settings = Открыть настройки системы
