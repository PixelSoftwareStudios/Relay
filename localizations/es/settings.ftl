# Relay desktop UI catalog, Spanish (es), section: Settings: this app, hotkey, profile, daemon, language.
# Translation of localizations/en/settings.ftl; same ids and $variables. See docs/i18n.md.

link-starting = Iniciando el daemon de Relay
link-connecting = Conectando con el daemon de Relay
link-connected = Conectado
link-reconnecting = Se perdió la conexión con el daemon, reconectando
link-offline = El daemon no está en ejecución
avatar-size-refusal = el avatar debe pesar entre 1 byte y 1 MiB (PNG, JPEG, WebP, GIF, BMP o TIFF)
hotkey-note = Este dispositivo no tiene atajo global ni botón Hablar en pantalla. Pulsar para hablar solo transmite mientras una fuente de entrada ajena a esta app (por ejemplo, la CLI) mantiene la tecla presionada.
close-tray = Al cerrar la ventana se oculta en la bandeja; las llamadas continúan.
close-no-tray = Esta plataforma no tiene bandeja, así que al cerrar la ventana se cierra esta app.
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
daemon-starting-button = Iniciando el daemon…
daemon-start-button = Iniciar daemon
app-hide = Ocultar ventana
app-quit = Salir de esta app
app-hang-up-quit = Colgar y salir
autostart-label = Iniciar el daemon automáticamente
autostart-hover = Si el daemon no está en ejecución, las apps de Relay lo inician. Se anuncia en la red local solo mientras haya una app abierta.
daemon-heading = Daemon de Relay
daemon-not-connected = El daemon no está conectado.
daemon-paused = El inicio automático está en pausa hasta que pulses Iniciar daemon.
daemon-keeps-running = El daemon sigue en ejecución al cerrar esta app. Al salir de él se terminan todas las llamadas y se libera el micrófono.
daemon-quit-ask = Salir del daemon…
daemon-quit-confirm = ¿Salir del daemon ahora? Las llamadas activas terminarán.
daemon-quit = Salir del daemon
profile-name-hint = Nombre visible
profile-save-name = Guardar nombre
profile-image-note = PNG, JPEG, WebP, GIF, BMP o TIFF, hasta 1 MiB y 2048 x 2048. El motor la escala y la comparte con tus dispositivos de confianza. Al quitarla, se les indica que dejen de mostrarla.
profile-choose-image = Elegir foto…
profile-upload = Subir
profile-discard = Descartar
profile-remove = Quitar foto
hotkey-heading = Atajo de pulsar para hablar
hotkey-intro = Mantén esta tecla presionada en cualquier lugar para hablar mientras una llamada está en modo pulsar para hablar.
hotkey-field = Atajo
hotkey-hint = Ctrl+Alt+Espacio (vacío = desactivado)
hotkey-nothing = Nada que cambiar.
notify-label = Notificar las llamadas entrantes
login-label = Iniciar esta app al iniciar sesión
login-error = Inicio al iniciar sesión: { $reason }
dialog-profile-image = Elegir una foto de perfil
dialog-image-filter = Imagen
dialog-export = Exportar la transcripción
login-no-mechanism = este sistema no tiene un mecanismo compatible de elementos de inicio de sesión
login-no-exe = no se encuentra este programa: { $error }
login-bad-path = la ruta del programa no es texto válido
login-set-unavailable = no disponible: { $reason }
options-save-failed = no se pudo guardar la opción: { $error }
language-label = Idioma
language-system = Sistema ({ $language })
language-experimental = Este idioma es experimental: la traducción aún no ha sido revisada por un hablante nativo.
link-offline-manual = Inicia el daemon; esta ventana se reconecta sola.
hotkey-applied = { $label }: aplicado ahora
hotkey-check-accelerator = el atajo debe tener como máximo 64 caracteres imprimibles
hotkey-check-delay = el retardo al soltar debe estar entre 0 y 2000 ms
hotkey-label-accelerator = Atajo de pulsar para hablar
hotkey-label-delay = Retardo al soltar
hotkey-bad-value = el daemon informó un valor incorrecto de { $key }: { $value }
hotkey-rejected = { $label }: rechazado
hotkey-rejected-reason = { $label }: rechazado ({ $reason })
hotkey-saved = { $label }: guardado, se aplicará tras reiniciar
hotkey-saved-reason = { $label }: guardado, se aplicará tras reiniciar ({ $reason })
hotkey-note-button = Este dispositivo no tiene atajo global. En una llamada en modo pulsar para hablar, mantén pulsado el botón Hablar para hablar.
close-background = Al salir de esta app, tus llamadas siguen en segundo plano.
settings-profile = Perfil
settings-profile-sub = Tu nombre y tu foto
settings-audio = Audio
settings-audio-sub = Micrófono, altavoces y volumen
settings-ptt = Pulsar para hablar
settings-ptt-sub = Atajo y retardo al soltar
settings-calls = Llamadas
settings-calls-sub = Quién puede llamarte y sonidos
settings-privacy = Privacidad
settings-privacy-sub = Visibilidad, mensajes y dispositivos de confianza
settings-language = Idioma
settings-language-sub = El idioma de esta app
settings-startup = Inicio
settings-startup-sub = Notificaciones e inicio con la sesión
settings-advanced = Avanzado
settings-advanced-sub = Daemon y acerca de
hotkey-note-global = Mantén pulsado el atajo en cualquier lugar para hablar mientras una llamada está en modo pulsar para hablar.
profile-name-label = Nombre visible
profile-name-help = Por defecto es el nombre de tu dispositivo. Los demás ven este nombre cuando eres visible o te llaman.
profile-remove-ask = ¿Quitar tu foto?
profile-remove-yes = Sí, quitar
ptt-delay-help = Cuánto tiempo sigue abierto el micrófono después de soltar.
language-help = La app cambia al instante.
daemon-what = El daemon es el programa en segundo plano que gestiona las llamadas. Esta app es su ventana.
advanced-about = Acerca de
advanced-app-version = Relay { $version }
advanced-api = API { $major }.{ $minor }
advanced-counts = Dispositivos { $peers } · Llamadas { $calls } · Canales { $channels }
advanced-revision = Revisión { $revision }
history-setting-label = Guardar el historial de llamadas
history-setting-help = Recuerda quién llamó a quién, cómo terminó cada llamada y cuánto duró, solo en este dispositivo. Nunca se envía a la otra persona y se puede borrar en cualquier momento. Si está desactivado, no se registra nada nuevo y las entradas existentes permanecen hasta que las borres.
setting-unsupported = El servicio en segundo plano en ejecución no lo admite: reinícialo.
daemon-outdated = El servicio en segundo plano está desactualizado. Reinícialo para terminar de actualizar.
daemon-restart = Reiniciar
daemon-restarting = Reiniciando…
daemon-restart-in-call = Termina primero la llamada en curso: reiniciar la cortaría.
calls-default-mode = Al conectarse una llamada
calls-default-mode-help = Elige cómo empieza tu micrófono en cada llamada nueva. Puedes cambiarlo durante la llamada.
calls-default-muted = Tu micrófono está apagado hasta que lo actives.
calls-default-unmuted = Tu micrófono está encendido en cuanto se conecta la llamada.
calls-default-ptt = Tu micrófono solo está encendido mientras mantienes pulsado el botón o la tecla de hablar.
setting-saved = Guardado.
setting-saved-restart = Guardado. Se aplicará tras reiniciar.
setting-saved-restart-why = Guardado. Se aplicará tras reiniciar ({ $reason }).
setting-rejected = No se pudo guardar. Inténtalo de nuevo.
setting-rejected-why = No se pudo guardar: { $reason }. Inténtalo de nuevo.

hotkey-note-both = Mantén pulsado el atajo en cualquier lugar, o el botón Hablar de la llamada, para hablar mientras una llamada está en modo pulsar para hablar.
hotkey-state-off = El atajo está desactivado. Aún puedes mantener pulsado el botón Hablar de la llamada.
hotkey-state-on = El atajo { $key } está listo.
hotkey-state-failed = No se pudo registrar el atajo: { $reason }. Aún puedes mantener pulsado el botón Hablar de la pantalla.
hotkey-applied-here = Atajo de pulsar para hablar: guardado y aplicado ahora
hotkey-reason-invalid = { $key } no es una combinación de teclas válida
hotkey-reason-taken = no se pudo reservar, quizá otro programa ya la usa
hotkey-reason-unsupported = esta sesión no permite atajos globales
hotkey-reason-permission = el sistema todavía no lo ha permitido
hotkey-reason-other = { $detail }
