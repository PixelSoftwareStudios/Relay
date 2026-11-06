# Relay desktop UI catalog, Spanish (es), section: Settings: this app, hotkey, profile, daemon, language.
# Translation of localizations/en/settings.ftl; same ids and $variables. See docs/i18n.md.

link-starting = Iniciando el daemon de Relay
link-connecting = Conectando con el daemon de Relay
link-connected = Conectado
link-reconnecting = Se perdió la conexión con el daemon, reconectando
link-offline = El daemon no está en ejecución
avatar-size-refusal = el avatar debe pesar entre 1 byte y 1 MiB (PNG, JPEG, WebP, GIF, BMP o TIFF)
hotkey-note = Esta app no registra ningún atajo global ni tiene aún un botón de hablar en pantalla. Pulsar para hablar solo transmite mientras una fuente de entrada del motor (por ejemplo, la CLI) mantiene la tecla presionada.
close-tray = Al cerrar la ventana se oculta en la bandeja; las llamadas continúan.
close-no-tray = Esta plataforma no tiene icono de bandeja, así que al cerrar la ventana se cierra esta app.
quit-note = Al salir de esta app se cuelgan las llamadas que inició o aceptó, y el daemon de Relay sigue en ejecución. Esta app no puede detener el daemon.
calls-active =
    { $count ->
        [one] Hay una llamada activa.
        [many] Hay { $count } de llamadas activas.
       *[other] Hay { $count } llamadas activas.
    }
quit-prompt = ¿Salir de esta app?
quit-prompt-calls =
    { $count ->
        [one] ¿Salir de esta app? Se colgará tu llamada activa.
        [many] ¿Salir de esta app? Se colgarán tus { $count } de llamadas activas.
       *[other] ¿Salir de esta app? Se colgarán tus { $count } llamadas activas.
    }
daemon-stopped-again = Se detuvo otra vez justo después de iniciarse.
daemon-keeps-stopping = El daemon de Relay se sigue deteniendo, por lo que no se reiniciará de nuevo automáticamente. { $why }
avatar-bad-format = avatar rechazado: formato de imagen no admitido o no reconocido (usa PNG, JPEG, WebP, GIF, BMP o TIFF)
avatar-zero-size = avatar rechazado: el avatar tiene ancho o alto cero
avatar-too-large = avatar rechazado: las dimensiones del avatar, { $width }x{ $height }, superan el límite de { $limit } píxeles
avatar-undecodable = avatar rechazado: no se pudo decodificar el avatar: { $error }
daemon-started = El daemon de Relay se inició.
daemon-already-running = El daemon de Relay ya está en ejecución.
daemon-not-found = No se encuentra el daemon de Relay ({ $binary }). Instálalo junto a esta app o en tu PATH, o define { $env } con su ubicación.
daemon-start-failed = El daemon de Relay no se inició: { $reason }.
daemon-start-failed-log = El daemon de Relay no se inició: { $reason }. Última línea del registro: { $last }
daemon-stop-failed = El daemon de Relay no se detuvo: { $error }
daemon-stopped = El daemon de Relay se detuvo.
notify-off = Desactivado: una llamada que suena con esta ventana oculta o en segundo plano solo se muestra en el tooltip de la bandeja y se oye con el tono del daemon.
notify-on = Muestra el nombre de quien llama (nunca el texto de los mensajes) cuando suena una llamada con esta ventana oculta o sin estar al frente.
notify-refused = El sistema rechazó la última notificación ({ $reason }). Las llamadas siguen sonando mediante el daemon y se muestran en esta ventana y en el tooltip de la bandeja.
login-unsupported = No disponible aquí: { $reason }
login-off = Desactivado. Si se activa, esta app se registra en el sistema y abre su ventana al iniciar sesión. El daemon no se inicia hasta que la app lo necesite.
login-on = Activado: el sistema inicia esta app al iniciar sesión (abre su ventana). Desactívalo aquí para quitar la entrada.
login-failed = No se pudo leer ni cambiar el elemento de inicio de sesión: { $reason }
banner-mock = Datos de prueba: esta vista no está conectada a un daemon de Relay.
link-offline-auto = Se iniciará automáticamente.
daemon-starting-button = Iniciando el daemon...
daemon-start-button = Iniciar daemon
app-heading = Esta app
app-hide = Ocultar ventana
app-quit = Salir de esta app (el daemon sigue en ejecución)
app-hang-up-quit = Colgar y salir
autostart-label = Iniciar el daemon automáticamente
autostart-hover = Si el daemon no está en ejecución, las apps de Relay lo inician. Se anuncia en la red local solo mientras haya una app abierta.
daemon-heading = Daemon de Relay
daemon-not-connected = El daemon no está conectado.
daemon-paused = El inicio automático está en pausa hasta que pulses Iniciar daemon.
daemon-keeps-running = El daemon sigue en ejecución al cerrar esta app. Al salir de él se terminan todas las llamadas y se libera el micrófono.
daemon-quit-ask = Salir del daemon...
daemon-quit-confirm = ¿Salir del daemon ahora? Las llamadas activas terminarán.
daemon-quit = Salir del daemon
profile-heading = Tu perfil
profile-name-hint = Nombre visible
profile-save-name = Guardar nombre
profile-image-note = PNG, JPEG, WebP, GIF, BMP o TIFF, hasta 1 MiB y 2048 x 2048. El motor la escala y la comparte con tus pares de confianza. Al quitarla, se les indica que dejen de mostrarla.
profile-choose-image = Elegir imagen…
profile-upload = Subir
profile-discard = Descartar
profile-remove = Quitar imagen
hotkey-heading = Atajo de pulsar para hablar
hotkey-intro = Mantén esta tecla presionada en cualquier lugar para hablar mientras una llamada está en modo pulsar para hablar.
hotkey-field = Atajo
hotkey-hint = Ctrl+Alt+Space (vacío lo desactiva)
hotkey-delay = Retardo al soltar
hotkey-delay-hover = Cuánto tiempo sigue abierto el micrófono después de soltar la tecla.
hotkey-nothing = Nada que cambiar.
notify-label = Notificar las llamadas entrantes
login-label = Iniciar esta app al iniciar sesión
login-error = Inicio al iniciar sesión: { $reason }
dialog-profile-image = Elegir una imagen de perfil
dialog-image-filter = Imagen
dialog-export = Exportar la transcripción
login-no-mechanism = este sistema no tiene un mecanismo compatible de elementos de inicio de sesión
login-no-exe = no se encuentra este programa: { $error }
login-bad-path = la ruta del programa no es texto válido
login-set-unavailable = no disponible: { $reason }
options-save-failed = no se pudo guardar la opción: { $error }
language-label = Idioma
language-system = Sistema ({ $language })
language-experimental = Este idioma es experimental: el renderizador de texto no puede unir las letras devanagari, por lo que algunas palabras se ven distintas a la escritura impresa.
link-offline-manual = Inicia el daemon; esta ventana se reconecta sola.
hotkey-applied = { $label }: aplicado ahora
hotkey-check-accelerator = el atajo debe tener como máximo 64 caracteres imprimibles
hotkey-check-delay = el retardo al soltar debe estar entre 0 y 2000 ms
hotkey-label-accelerator = Atajo de pulsar para hablar
hotkey-label-delay = Retardo al soltar
hotkey-missing-key = el daemon no informó { $key }
hotkey-bad-value = el daemon informó un valor incorrecto de { $key }: { $value }
hotkey-rejected = { $label }: rechazado
hotkey-rejected-reason = { $label }: rechazado ({ $reason })
hotkey-saved = { $label }: guardado, se aplicará tras reiniciar
hotkey-saved-reason = { $label }: guardado, se aplicará tras reiniciar ({ $reason })
hotkey-note-button = Este dispositivo no tiene atajo global. En una llamada en modo pulsar para hablar, mantén pulsado el botón Hablar para hablar.
close-background = Al salir de esta app, tus llamadas siguen en segundo plano.
