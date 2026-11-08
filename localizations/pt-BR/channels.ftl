# Relay desktop UI catalog, Portuguese (Brazil) (pt-BR), section: Channels, privacy and the connection string.
# Tradução para o português do Brasil; ids e $variáveis iguais aos do inglês.

channels-heading = Canais
channels-note = Um canal define quem pode ver e ligar para você. Ele nunca liga o seu microfone.
channels-none = Você não está em nenhum canal.
channel-listed = listado
channel-password = senha
channel-invite-only = só por convite
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } dispositivo em comum
        [many] { $peers } de dispositivos em comum
       *[other] { $peers } dispositivos em comum
    }
channel-invite = Convidar…
channel-leave-yes = Sim, sair
channel-leave-stay = Ficar
channel-leave = Sair
invite-uses = { $count ->
        [one] { $count } uso
        [many] { $count } de usos
       *[other] { $count } usos
    }
invite-create = Criar convite
invite-token-note = Código de convite (mantenha em segredo). Quem o tiver pode entrar até ele expirar ou esgotar os usos:
channel-create-heading = Criar um canal
channel-name-hint = Nome do canal
channel-listed-label = Listado: mostrar o nome nos dispositivos dos membros
channel-password-label = Exigir senha para entrar
channel-invite-only-label = Só por convite
password-hint = Senha
channel-join-heading = Entrar em um canal
channel-join-note = Cole um código de convite ou um ID de canal com a senha.
channel-join-hint = Código de convite ou ID do canal
password-optional-hint = Senha (desnecessária com convite)
privacy-none = As configurações de privacidade aparecem quando o daemon estiver conectado.
privacy-inbound-anyone = Qualquer pessoa na rede
privacy-inbound-shared = Só membros dos meus canais
privacy-who-rings = Quem pode ligar para mim
privacy-advert-visible = Visível para dispositivos por perto
privacy-advert-hidden = Oculto (acessível só pela string de conexão)
privacy-visibility = Visibilidade na rede
invite-ttl-hour = 1 hora
invite-ttl-day = 1 dia
invite-ttl-week = 7 dias
connection-none = O Relay não encontrou um endereço para compartilhar. Verifique se ele está em execução e tente de novo.
connection-share = Compartilhe este endereço para que outro dispositivo da sua rede possa ligar para você:
connection-addresses = Endereços: { $addresses } · porta { $port }
connection-fingerprint = Impressão digital { $fingerprint }
connection-warning = Qualquer pessoa com este endereço pode ligar para você. Isso não a torna confiável nem dá acesso ao seu microfone.
connection-qr-too-long = O endereço é longo demais para um código QR.
channel-hidden = oculto
channels-none-help = Crie um canal para falar com várias pessoas ao mesmo tempo, ou entre em um com um convite.
connection-copy = Copiar endereço
connection-error-next = Verifique se o Relay está em execução em segundo plano e tente de novo.
connection-qr-help = Escaneie este código com outro dispositivo para ele ligar para você.
privacy-who-rings-help = Define quem pode iniciar uma chamada com você.
privacy-visibility-help = Dispositivos ocultos ainda podem receber chamadas pela string de conexão.
privacy-messages = Mensagens
privacy-trusted = Dispositivos confiáveis
privacy-trusted-none = Você ainda não confia em nenhum dispositivo.
privacy-trusted-help = Esqueça um dispositivo para deixar de confiar nele.
connection-scan = Ler um código QR
scan-wrong = Esse código QR não é um endereço do Relay.
scan-camera-denied = O Relay precisa da câmera para ler um código. Permita e tente de novo.
scan-camera-blocked = O acesso à câmera está desativado para o Relay. Ative nas configurações do sistema para ler.
scan-unavailable = Este dispositivo não consegue ler códigos QR. Copie o endereço.
scan-failed = A leitura não funcionou. Tente de novo ou copie o endereço.
scan-help = Leia o código mostrado no outro dispositivo para adicioná-lo aos seus contatos.
scan-trusting = Conectando e confiando…
scan-added = { $name } foi adicionado aos contatos.
scan-added-unnamed = O dispositivo foi adicionado aos contatos.
scan-unreachable = Não foi possível alcançar esse dispositivo. Verifique se ele está na mesma rede com o Relay aberto e leia o código de novo.
scan-mismatch = Esse dispositivo não corresponde ao código, então não foi considerado confiável.
scan-trust-failed = Não foi possível adicionar esse dispositivo: { $reason }
privacy-blocked = Dispositivos bloqueados
privacy-blocked-none = Você não bloqueou nenhum dispositivo.
privacy-blocked-help = Dispositivos bloqueados não podem ligar para você e não aparecem como próximos. Desbloquear permite encontrá-lo de novo; ele não vira um contato.
