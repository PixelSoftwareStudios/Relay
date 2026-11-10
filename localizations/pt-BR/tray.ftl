# Relay desktop UI catalog, Portuguese (Brazil) (pt-BR), section: Tray menu and tooltip.
# Tradução para o português do Brasil; ids e $variáveis iguais aos do inglês.

tray-mock = Relay (dados fictícios)
tray-starting = Relay: iniciando o daemon
tray-connecting = Relay: conectando ao daemon
tray-reconnecting = Relay: conexão com o daemon perdida, reconectando
tray-offline = Relay: o daemon não está em execução
tray-incoming =
    { $count ->
        [one] Relay: { $count } chamada recebida
        [many] Relay: { $count } de chamadas recebidas
       *[other] Relay: { $count } chamadas recebidas
    }
tray-active =
    { $count ->
        [one] Relay: { $count } chamada ativa
        [many] Relay: { $count } de chamadas ativas
       *[other] Relay: { $count } chamadas ativas
    }
tray-idle = Relay: inativo
tray-show = Mostrar o Relay
tray-quit = Sair deste app (o daemon continua em execução)
tray-status-available = Disponível
tray-status-unavailable = Indisponível
tray-stay-available = Continuar disponível em segundo plano
