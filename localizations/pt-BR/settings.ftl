# Relay desktop UI catalog, Portuguese (Brazil) (pt-BR), section: Settings: this app, hotkey, profile, daemon, language.
# Tradução para o português do Brasil; ids e $variáveis iguais aos do inglês.

link-starting = Iniciando o daemon do Relay
link-connecting = Conectando ao daemon do Relay
link-connected = Conectado
link-reconnecting = Conexão com o daemon perdida, reconectando
link-offline = Daemon não está em execução
avatar-size-refusal = o avatar deve ter de 1 byte a 1 MiB (PNG, JPEG, WebP, GIF, BMP ou TIFF)
hotkey-note = Este app não registra nenhum atalho global e ainda não tem botão de falar na tela. O aperte para falar só transmite enquanto uma fonte de entrada do engine (por exemplo, a CLI) mantém a tecla pressionada.
close-tray = Fechar a janela a oculta na bandeja; as chamadas continuam.
close-no-tray = Não há ícone na bandeja nesta plataforma, então fechar a janela encerra este app.
quit-note = Sair deste app desliga as chamadas que ele iniciou ou aceitou e deixa o daemon do Relay em execução. Este app não pode parar o daemon.
calls-active =
    { $count ->
        [one] Uma chamada está ativa.
        [many] { $count } de chamadas estão ativas.
       *[other] { $count } chamadas estão ativas.
    }
quit-prompt = Sair deste app?
quit-prompt-calls =
    { $count ->
        [one] Sair deste app? Sua chamada ativa será desligada.
        [many] Sair deste app? Suas { $count } de chamadas ativas serão desligadas.
       *[other] Sair deste app? Suas { $count } chamadas ativas serão desligadas.
    }
daemon-stopped-again = Ele parou de novo logo após iniciar.
daemon-keeps-stopping = O daemon do Relay continua parando, então não será reiniciado automaticamente de novo. { $why }
avatar-bad-format = avatar rejeitado: formato de imagem não compatível ou não reconhecido (use PNG, JPEG, WebP, GIF, BMP ou TIFF)
avatar-zero-size = avatar rejeitado: o avatar tem largura ou altura zero
avatar-too-large = avatar rejeitado: as dimensões do avatar, { $width }x{ $height }, excedem o limite de { $limit } pixels
avatar-undecodable = avatar rejeitado: não foi possível decodificar o avatar: { $error }
daemon-started = O daemon do Relay foi iniciado.
daemon-already-running = O daemon do Relay já está em execução.
daemon-not-found = Não foi possível encontrar o daemon do Relay ({ $binary }). Instale-o ao lado deste app ou no seu PATH, ou defina { $env } com o local dele.
daemon-start-failed = O daemon do Relay não iniciou: { $reason }.
daemon-start-failed-log = O daemon do Relay não iniciou: { $reason }. Última linha do log: { $last }
daemon-stop-failed = O daemon do Relay não parou: { $error }
daemon-stopped = O daemon do Relay parou.
notify-off = Desativado: uma chamada que toca com esta janela oculta ou em segundo plano só aparece na dica da bandeja e é ouvida pelo toque do daemon.
notify-on = Mostra o nome de quem liga (nunca o texto das mensagens) quando uma chamada toca com esta janela oculta ou fora de foco.
notify-refused = O sistema recusou a última notificação ({ $reason }). As chamadas continuam tocando pelo daemon e aparecem nesta janela e na dica da bandeja.
login-unsupported = Indisponível aqui: { $reason }
login-off = Desativado. Quando ativado, este app é registrado no sistema e abre a janela ao iniciar a sessão. O daemon só é iniciado quando o app precisar.
login-on = Ativado: o sistema inicia este app quando você entra na sessão (ele abre a janela). Desative aqui para remover a entrada.
login-failed = Não foi possível ler ou alterar o item de inicialização: { $reason }
banner-mock = Dados de teste: esta tela não está conectada a um daemon do Relay.
link-offline-auto = Iniciando automaticamente.
daemon-starting-button = Iniciando o daemon...
daemon-start-button = Iniciar daemon
app-hide = Ocultar janela
app-quit = Sair deste app
app-hang-up-quit = Desligar e sair
autostart-label = Iniciar o daemon automaticamente
autostart-hover = Quando o daemon não está em execução, os apps do Relay o iniciam. Ele só se anuncia na rede local enquanto um app estiver aberto.
daemon-heading = Daemon do Relay
daemon-not-connected = O daemon não está conectado.
daemon-paused = O início automático está pausado até você clicar em Iniciar daemon.
daemon-keeps-running = O daemon continua em execução quando você fecha este app. Encerrá-lo termina todas as chamadas e libera o microfone.
daemon-quit-ask = Encerrar daemon...
daemon-quit-confirm = Encerrar o daemon agora? As chamadas ativas serão terminadas.
daemon-quit = Encerrar daemon
profile-name-hint = Nome de exibição
profile-save-name = Salvar nome
profile-image-note = PNG, JPEG, WebP, GIF, BMP ou TIFF, até 1 MiB e 2048 x 2048. O engine a redimensiona e a compartilha com as pessoas em quem você confia. Remover a imagem avisa a elas para pararem de mostrá-la.
profile-choose-image = Escolher imagem…
profile-upload = Enviar
profile-discard = Descartar
profile-remove = Remover imagem
hotkey-heading = Atalho do aperte para falar
hotkey-intro = Mantenha esta tecla pressionada em qualquer lugar para falar enquanto uma chamada estiver no modo aperte para falar.
hotkey-field = Atalho
hotkey-hint = Ctrl+Alt+Space (vazio desativa)
hotkey-nothing = Nada a alterar.
notify-label = Notificar chamadas recebidas
login-label = Iniciar este app ao entrar na sessão
login-error = Iniciar ao entrar na sessão: { $reason }
dialog-profile-image = Escolher uma imagem de perfil
dialog-image-filter = Imagem
dialog-export = Exportar a conversa
login-no-mechanism = este sistema não tem um mecanismo compatível de item de inicialização
login-no-exe = não foi possível encontrar este programa: { $error }
login-bad-path = o caminho do programa não é texto válido
login-set-unavailable = indisponível: { $reason }
options-save-failed = não foi possível salvar a escolha: { $error }
language-label = Idioma
language-system = Sistema ({ $language })
language-experimental = Este idioma é experimental: o renderizador de texto não consegue unir as letras devanágari, então algumas palavras ficam diferentes da escrita impressa.
link-offline-manual = Inicie o daemon; esta janela se reconecta sozinha.
hotkey-applied = { $label }: aplicado agora
hotkey-check-accelerator = o atalho deve ter no máximo 64 caracteres imprimíveis
hotkey-check-delay = o atraso de liberação deve ficar entre 0 e 2000 ms
hotkey-label-accelerator = Atalho do aperte para falar
hotkey-label-delay = Atraso de liberação
hotkey-missing-key = o daemon não informou { $key }
hotkey-bad-value = o daemon informou um valor inválido para { $key }: { $value }
hotkey-rejected = { $label }: rejeitado
hotkey-rejected-reason = { $label }: rejeitado ({ $reason })
hotkey-saved = { $label }: salvo, vale após reiniciar
hotkey-saved-reason = { $label }: salvo, vale após reiniciar ({ $reason })
hotkey-note-button = Este dispositivo não tem atalho global. Em uma chamada no modo pressionar para falar, mantenha o botão Falar pressionado para falar.
close-background = Ao sair deste app, suas chamadas continuam em segundo plano.
settings-profile = Perfil
settings-profile-sub = Seu nome e sua foto
settings-audio = Áudio
settings-audio-sub = Microfone, alto-falantes e volume
settings-ptt = Pressionar para falar
settings-ptt-sub = Atalho e atraso ao soltar
settings-calls = Chamadas
settings-calls-sub = Quem pode ligar e sons de chamada
settings-privacy = Privacidade
settings-privacy-sub = Visibilidade, mensagens e dispositivos confiáveis
settings-language = Idioma
settings-language-sub = O idioma deste app
settings-startup = Inicialização
settings-startup-sub = Notificações e início ao entrar
settings-advanced = Avançado
settings-advanced-sub = Daemon e sobre
hotkey-note-global = Segure o atalho em qualquer lugar para falar enquanto uma chamada está no modo pressionar para falar.
profile-name-label = Nome de exibição
profile-name-help = As outras pessoas veem este nome quando você liga ou envia mensagem.
profile-remove-ask = Remover sua foto?
profile-remove-yes = Sim, remover
ptt-delay-help = Quanto tempo o microfone fica aberto depois que você solta.
language-help = O app muda na hora.
daemon-what = O daemon é o programa em segundo plano que faz as chamadas. Este app é a janela dele.
advanced-about = Sobre
advanced-app-version = Relay { $version }
advanced-api = API { $major }.{ $minor }
advanced-counts = Dispositivos { $peers } · Chamadas { $calls } · Canais { $channels }
advanced-revision = Revisão { $revision }
history-setting-label = Manter o histórico de chamadas
history-setting-help = Lembra quem ligou para quem, como cada chamada terminou e quanto durou, somente neste dispositivo. Nunca é enviado à outra pessoa e pode ser apagado a qualquer momento. Se estiver desativado, nada novo é registrado e as entradas existentes permanecem até você apagá-las.
