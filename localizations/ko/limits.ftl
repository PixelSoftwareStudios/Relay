# Relay desktop UI catalog, ko, section: Storage and limits (limits.* settings). Same ids and $variables as localizations/en/limits.ftl. See docs/i18n.md.

settings-limits = 저장 공간 및 한도
settings-limits-sub = 캐시 크기와 보관 기간
limits-intro = 이 한도는 이 기기에 얼마나 보관할지 정합니다. 모든 데이터는 이 기기에만 저장됩니다. 대부분의 경우 기본값이 적합합니다. 변경 사항은 바로 적용되며, 사용 중인 항목은 중단되지 않습니다.
limits-group-pictures = 사진
limits-group-chats = 채팅 및 파일
limits-group-history = 통화 기록
limits-unit-mib = MiB
limits-unit-entries =
    { $count ->
       *[other] 개
    }
limits-unit-days =
    { $count ->
       *[other] 일
    }
limits-unit-transfers =
    { $count ->
       *[other] 건
    }
limits-unit-calls =
    { $count ->
       *[other] 건
    }
limits-unit-kib = KiB
limits-unit-seconds =
    { $count ->
       *[other] 초
    }
limits-avatar-label = 사진 캐시
limits-avatar-help = 대화 상대의 사진을 여기에 보관해 빠르게 표시합니다. 캐시가 가득 차면 가장 오래 사용하지 않은 사진부터 삭제되며, 내 사진은 삭제되지 않습니다. 한도를 낮추면 사진이 즉시 삭제됩니다.
limits-avatar-used = 사용 중: { $used } MiB
limits-session-label = 진행 중인 채팅당 메모리
limits-session-help = 진행 중인 통화의 채팅이 사용할 수 있는 최대 메모리입니다. 가득 차면 해당 채팅의 새 메시지와 파일이 거부되며, 이미 있는 내용은 삭제되지 않습니다.
limits-session-used = 가장 큰 진행 중 채팅: { $used } MiB
limits-retained-label = 종료된 채팅의 메모리
limits-retained-help = 종료된 통화의 채팅은 Relay가 종료될 때까지 읽을 수 있습니다. 이 값을 넘으면 가장 오래된 종료된 채팅부터 삭제됩니다.
limits-retained-used = 종료된 채팅 사용량: { $used } MiB
limits-entries-label = 기록에 남길 통화 수
limits-entries-help = 통화 기록에 보관하는 최대 통화 수입니다. 넘으면 가장 오래된 것부터 삭제됩니다. 0이면 보관하지 않습니다.
limits-age-label = 통화 보관 기간
limits-age-help = 이보다 오래된 통화는 기록에서 삭제됩니다.
limits-transfers-label = 동시 전송 파일 수
limits-transfers-help = 한 통화에서 동시에 보내거나 받을 수 있는 파일 수입니다. 그 이상의 파일은 하나가 끝날 때까지 거부됩니다.
limits-calls-label = 보관할 종료된 채팅 수
limits-calls-help = 최근 종료된 통화 중 이 개수만큼의 채팅은 앱을 종료할 때까지 메모리에 보관되며, 오래된 것부터 삭제됩니다. 채팅은 디스크에 저장되지 않습니다. 0이면 아무것도 보관하지 않습니다.
limits-ringtone-size-label = 최대 벨소리 파일 크기
limits-ringtone-size-help = 벨소리로 선택할 수 있는 가장 큰 오디오 파일입니다. 더 큰 파일은 거부됩니다.
limits-ringtone-length-label = 최대 벨소리 길이
limits-ringtone-length-help = 벨소리로 선택할 수 있는 가장 긴 소리입니다. 더 긴 소리는 잘리지 않고 거부됩니다.
limits-check-range = { $label }은(는) { $min }에서 { $max } { $unit } 사이여야 합니다
limits-check-pair = 종료된 채팅의 메모리는 진행 중인 채팅당 메모리보다 작을 수 없습니다.
limits-reset = 기본값으로 되돌리기
limits-nothing = 변경할 내용이 없습니다.
limits-applied = { $label }: 바로 적용됨
limits-saved = { $label }: 저장됨, 다시 시작하면 적용됩니다
limits-saved-reason = { $label }: 저장됨, 다시 시작하면 적용됩니다 ({ $reason })
limits-rejected = { $label }: 거부됨
limits-rejected-reason = { $label }: 거부됨 ({ $reason })
