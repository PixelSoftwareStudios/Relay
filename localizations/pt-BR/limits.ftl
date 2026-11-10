# Relay desktop UI catalog, pt-BR, section: Storage and limits (limits.* settings). Same ids and $variables as localizations/en/limits.ftl. See docs/i18n.md.

settings-limits = Armazenamento e limites
settings-limits-sub = Tamanho dos caches e por quanto tempo tudo é guardado
limits-intro = Estes limites definem quanto este dispositivo guarda. Tudo fica neste dispositivo. Os valores padrão servem para a maioria das pessoas. As alterações valem na hora, e nada que esteja em uso é interrompido.
limits-group-pictures = Imagens
limits-group-chats = Conversas e arquivos
limits-group-history = Histórico de chamadas
limits-unit-mib = MiB
limits-unit-entries =
    { $count ->
        [one] entrada
       *[other] entradas
    }
limits-unit-days =
    { $count ->
        [one] dia
       *[other] dias
    }
limits-unit-transfers =
    { $count ->
        [one] transferência
       *[other] transferências
    }
limits-unit-calls =
    { $count ->
        [one] chamada
       *[other] chamadas
    }
limits-unit-kib = KiB
limits-unit-seconds =
    { $count ->
        [one] segundo
       *[other] segundos
    }
limits-avatar-label = Cache de imagens
limits-avatar-help = As imagens das pessoas com quem você fala ficam guardadas aqui para aparecerem rapidamente. Quando o cache enche, as imagens usadas há mais tempo são removidas primeiro; sua própria imagem nunca é removida. Diminuir o limite remove imagens na hora.
limits-avatar-used = Em uso: { $used } MiB
limits-session-label = Memória por conversa em andamento
limits-session-help = O máximo de memória que a conversa de uma chamada em andamento pode usar. Quando ela enche, novas mensagens e arquivos nessa conversa são recusados; nada do que já está na conversa é apagado.
limits-session-used = Maior conversa em andamento: { $used } MiB
limits-retained-label = Memória para conversas encerradas
limits-retained-help = As conversas de chamadas encerradas continuam legíveis até o Relay ser fechado. Se usarem mais do que isso, as conversas encerradas mais antigas são removidas primeiro.
limits-retained-used = Conversas encerradas usam { $used } MiB
limits-entries-label = Chamadas no histórico
limits-entries-help = O máximo de chamadas que o histórico guarda. Se houver mais, as mais antigas são removidas. 0 não guarda nenhuma.
limits-age-label = Guardar chamadas por
limits-age-help = Chamadas mais antigas que isso são removidas do histórico.
limits-transfers-label = Arquivos ao mesmo tempo
limits-transfers-help = Quantos arquivos podem ser enviados ou recebidos ao mesmo tempo em uma chamada. Arquivos adicionais são recusados até que um termine.
limits-calls-label = Conversas encerradas mantidas
limits-calls-help = As conversas dessa quantidade de chamadas encerradas mais recentes ficam na memória até você sair; as mais antigas são removidas primeiro. As conversas nunca são salvas no disco. 0 não mantém nenhuma.
limits-ringtone-size-label = Maior arquivo de toque
limits-ringtone-size-help = O maior arquivo de áudio que você pode escolher como toque. Um arquivo maior é recusado.
limits-ringtone-length-label = Toque mais longo
limits-ringtone-length-help = O som mais longo que você pode escolher como toque. Um som mais longo é recusado, nunca cortado.
limits-check-range = { $label } deve estar entre { $min } e { $max } { $unit }
limits-check-pair = A memória para conversas encerradas não pode ser menor que a memória por conversa em andamento.
limits-reset = Restaurar padrões
limits-nothing = Nada a alterar.
limits-applied = { $label }: aplicado agora
limits-saved = { $label }: salvo, terá efeito após reiniciar
limits-saved-reason = { $label }: salvo, terá efeito após reiniciar ({ $reason })
limits-rejected = { $label }: recusado
limits-rejected-reason = { $label }: recusado ({ $reason })
