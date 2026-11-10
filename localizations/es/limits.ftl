# Relay desktop UI catalog, es, section: Storage and limits (limits.* settings). Same ids and $variables as localizations/en/limits.ftl. See docs/i18n.md.

settings-limits = Almacenamiento y límites
settings-limits-sub = Tamaño de las cachés y cuánto tiempo se conservan los datos
limits-intro = Estos límites deciden cuánto conserva este dispositivo. Todo se queda en este dispositivo. Los valores predeterminados son adecuados para la mayoría. Los cambios se aplican al instante y no interrumpen nada que esté en uso.
limits-group-pictures = Imágenes
limits-group-chats = Chats y archivos
limits-group-history = Historial de llamadas
limits-unit-mib = MiB
limits-unit-entries =
    { $count ->
        [one] entrada
       *[other] entradas
    }
limits-unit-days =
    { $count ->
        [one] día
       *[other] días
    }
limits-unit-transfers =
    { $count ->
        [one] transferencia
       *[other] transferencias
    }
limits-unit-calls =
    { $count ->
        [one] llamada
       *[other] llamadas
    }
limits-unit-kib = KiB
limits-unit-seconds =
    { $count ->
        [one] segundo
       *[other] segundos
    }
limits-avatar-label = Caché de imágenes
limits-avatar-help = Aquí se guardan las imágenes de las personas con las que hablas para que se vean rápido. Cuando la caché se llena, se eliminan primero las imágenes usadas hace más tiempo; tu propia imagen nunca se elimina. Bajar el límite elimina imágenes al instante.
limits-avatar-used = En uso: { $used } MiB
limits-session-label = Memoria de un chat en curso
limits-session-help = La memoria máxima que puede usar el chat de una llamada en curso. Cuando se llena, se rechazan los mensajes y archivos nuevos de ese chat; no se borra nada de lo que ya está en el chat.
limits-session-used = Chat en curso más grande: { $used } MiB
limits-retained-label = Memoria de chats terminados
limits-retained-help = Los chats de llamadas terminadas siguen siendo legibles hasta que Relay se cierra. Si usan más que esto, se eliminan primero los chats terminados más antiguos.
limits-retained-used = Los chats terminados usan { $used } MiB
limits-entries-label = Llamadas en el historial
limits-entries-help = El máximo de llamadas que conserva el historial. Si hay más, se eliminan las más antiguas. 0 no conserva ninguna.
limits-age-label = Conservar llamadas durante
limits-age-help = Las llamadas con más antigüedad se eliminan del historial.
limits-transfers-label = Archivos a la vez
limits-transfers-help = Cuántos archivos se pueden enviar o recibir a la vez en una llamada. Si se alcanza el límite, los demás se rechazan hasta que termine uno.
limits-calls-label = Chats terminados conservados
limits-calls-help = Se conservan en memoria los chats de este número de llamadas terminadas recientes hasta que cierres Relay; los más antiguos se eliminan primero. Los chats nunca se guardan en el disco. 0 no conserva ninguno.
limits-ringtone-size-label = Tamaño máximo del tono
limits-ringtone-size-help = El archivo de audio más grande que puedes elegir como tono. Uno mayor se rechaza.
limits-ringtone-length-label = Duración máxima del tono
limits-ringtone-length-help = El sonido más largo que puedes elegir como tono. Uno más largo se rechaza, nunca se corta.
limits-check-range = { $label } debe estar entre { $min } y { $max } { $unit }
limits-check-pair = La memoria de chats terminados no puede ser menor que la de un chat en curso.
limits-reset = Restablecer valores predeterminados
limits-nothing = Nada que cambiar.
limits-applied = { $label }: aplicado ahora
limits-saved = { $label }: guardado, se aplica tras reiniciar
limits-saved-reason = { $label }: guardado, se aplica tras reiniciar ({ $reason })
limits-rejected = { $label }: rechazado
limits-rejected-reason = { $label }: rechazado ({ $reason })
