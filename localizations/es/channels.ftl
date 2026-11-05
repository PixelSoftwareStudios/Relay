# Relay desktop UI catalog, Spanish (es), section: Channels, privacy and the connection string.
# Translation of localizations/en/channels.ftl; same ids and $variables. See docs/i18n.md.

channels-heading = Canales
channels-note = Un canal decide quién puede verte y llamarte. Nunca activa tu micrófono.
channels-none = No estás en ningún canal.
channel-listed = listado
channel-password = contraseña
channel-invite-only = solo con invitación
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } par en común
        [many] { $peers } de pares en común
       *[other] { $peers } pares en común
    }
channel-invite = Invitar…
channel-leave-yes = Sí, salir
channel-leave-stay = Quedarme
channel-leave = Salir
invite-uses = { $count ->
        [one] { $count } uso
        [many] { $count } de usos
       *[other] { $count } usos
    }
invite-create = Crear invitación
invite-token-note = Token de invitación (secreto). Quien lo tenga puede unirse hasta que caduque o se agote:
channel-create-heading = Crear un canal
channel-name-hint = Nombre del canal
channel-listed-label = Listado: mostrar el nombre en los dispositivos de los miembros
channel-password-label = Pedir contraseña para unirse
channel-invite-only-label = Solo con invitación
password-hint = Contraseña
channel-join-heading = Unirse a un canal
channel-join-note = Pega un token de invitación, o un id de canal con su contraseña.
channel-join-hint = RLYI1… o id de canal
password-optional-hint = Contraseña (no hace falta con una invitación)
privacy-heading = Privacidad
privacy-none = Los ajustes de privacidad aparecen cuando el daemon está conectado.
privacy-inbound-anyone = Cualquiera en la red
privacy-inbound-shared = Solo miembros de mis canales
privacy-who-rings = Quién puede llamarme
privacy-advert-visible = Visible para dispositivos cercanos
privacy-advert-hidden = Oculto (solo accesible con la cadena de conexión)
privacy-visibility = Visibilidad en la red
invite-ttl-hour = 1 hora
invite-ttl-day = 1 día
invite-ttl-week = 7 días
connection-heading = Conexión
connection-none = El daemon no informó una cadena de conexión.
connection-share = Comparte esta cadena para que te llamen desde otro equipo de tu red:
connection-addresses = Direcciones: { $addresses } · puerto { $port }
connection-fingerprint = Huella digital { $fingerprint }
connection-warning = Cualquiera con esta cadena puede llamarte; no otorga confianza ni acceso al micrófono.
connection-qr-too-long = La cadena de conexión es demasiado larga para un código QR.
connection-call = Llamar a una cadena de conexión
channel-hidden = oculto
