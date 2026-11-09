# Relay desktop UI catalog, Spanish (es), section: Channels, privacy and the connection string.
# Translation of localizations/en/channels.ftl; same ids and $variables. See docs/i18n.md.

channels-heading = Canales
channels-none = No estás en ningún canal.
channels-yours = Tus canales
channel-listed = listado
channel-password = contraseña
channel-invite-only = solo con invitación
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } dispositivo en común
        [many] { $peers } de dispositivos en común
       *[other] { $peers } dispositivos en común
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
invite-token-note = Código de invitación (mantenlo en secreto). Quien lo tenga puede unirse hasta que caduque o se agote:
channel-create-heading = Crear un canal
channel-name-hint = Nombre del canal
channel-listed-label = Listado: mostrar el nombre en los dispositivos de los miembros
channel-password-label = Pedir contraseña para unirse
channel-invite-only-label = Solo con invitación
password-hint = Contraseña
channel-join-heading = Unirse a un canal
channel-join-note = Pega un código de invitación, o introduce el nombre o el ID de un canal con su contraseña.
channel-join-hint = Código de invitación, nombre o ID de canal
password-optional-hint = Contraseña (no hace falta con una invitación)
privacy-none = Los ajustes de privacidad aparecen cuando el daemon está conectado.
privacy-inbound-anyone = Cualquiera en la red
privacy-inbound-shared = Solo miembros de mis canales
privacy-inbound-nobody = Nadie
privacy-inbound-nobody-help = Nadie puede llamar a este dispositivo. Tú sí puedes llamar a otros.
privacy-who-rings = Quién puede llamarme
privacy-advert-visible = Visible para dispositivos cercanos
privacy-advert-hidden = Oculto (solo accesible con la cadena de conexión)
privacy-visibility = Visibilidad en la red
invite-ttl-hour = 1 hora
invite-ttl-day = 1 día
invite-ttl-week = 7 días
connection-none = Relay no encontró una dirección para compartir. Comprueba que esté en ejecución y vuelve a intentarlo.
connection-share = Comparte esta dirección para que otro dispositivo de tu red pueda llamarte:
connection-addresses = Direcciones: { $addresses } · puerto { $port }
connection-fingerprint = Huella { $fingerprint }
connection-warning = Cualquiera con esta dirección puede llamarte. No le da confianza ni acceso a tu micrófono.
connection-qr-too-long = La dirección es demasiado larga para un código QR.
channel-hidden = oculto
channels-none-help = Un canal decide quién puede verte y llamarte, y nunca activa tu micrófono. Crea uno o únete con una invitación.
connection-copy = Copiar dirección
connection-error-next = Comprueba que Relay se esté ejecutando en segundo plano y vuelve a intentarlo.
connection-qr-help = Escanea este código con otro dispositivo para añadir este.
privacy-who-rings-help = Decide quién puede iniciar una llamada contigo.
privacy-visibility-help = Los dispositivos ocultos aún pueden recibir llamadas con su cadena de conexión.
privacy-messages = Mensajes
privacy-trusted = Dispositivos de confianza
privacy-trusted-none = Aún no confías en ningún dispositivo.
privacy-trusted-help = Olvida un dispositivo para dejar de confiar en él.
connection-scan = Escanear un código QR
scan-wrong = Ese código QR no es una dirección de Relay.
scan-camera-denied = Relay necesita la cámara para escanear un código. Permítela e inténtalo de nuevo.
scan-camera-blocked = El acceso a la cámara está desactivado para Relay. Actívalo en los ajustes del sistema para escanear.
scan-unavailable = Este dispositivo no puede escanear códigos QR. Copia la dirección en su lugar.
scan-failed = El escaneo no funcionó. Inténtalo de nuevo o copia la dirección en su lugar.
scan-help = Escanea el código que muestra el otro dispositivo para añadirlo a tus contactos.
scan-trusting = Conectando y estableciendo confianza…
scan-added = { $name } se ha añadido a los contactos.
scan-added-unnamed = El dispositivo se ha añadido a los contactos.
scan-unreachable = No se pudo contactar con ese dispositivo. Comprueba que esté en la misma red con Relay abierto y vuelve a escanear.
scan-mismatch = Ese dispositivo no coincide con el código, así que no se ha confiado en él.
scan-trust-failed = No se pudo añadir ese dispositivo: { $reason }
privacy-blocked = Dispositivos bloqueados
privacy-blocked-none = No has bloqueado ningún dispositivo.
privacy-blocked-help = Los dispositivos bloqueados no pueden llamarte y no aparecen como cercanos. Al desbloquear uno puede volver a encontrarse; no pasa a ser un contacto.
nearby-heading = Canales en tu red
nearby-devices = { $count ->
        [one] { $count } dispositivo
       *[other] { $count } dispositivos
    }
nearby-none = Aún no se han encontrado canales en tu red
nearby-none-offline = Este dispositivo está sin conexión, así que no puede ver otros dispositivos.
nearby-ambiguous = Dos canales usan este nombre. Únete con un código.
