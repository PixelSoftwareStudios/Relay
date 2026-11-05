# Relay desktop UI catalog, Korean (ko), section: Tray menu and tooltip.
# Terms: call = 통화, hang up = 끊기, mute = 음소거, deafen = 듣기 끄기, peer = 사용자, channel = 채널,
# invite = 초대, fingerprint = 지문, trust = 신뢰, avatar = 프로필 사진, file offer = 파일 제안.
# See docs/i18n.md.

tray-mock = Relay (모의 데이터)
tray-starting = Relay: 데몬 시작 중
tray-connecting = Relay: 데몬에 연결 중
tray-reconnecting = Relay: 데몬 연결이 끊겨 다시 연결 중
tray-offline = Relay: 데몬이 실행 중이 아님
tray-incoming =
    { $count ->
       *[other] Relay: 수신 통화 { $count }건
    }
tray-active =
    { $count ->
       *[other] Relay: 진행 중인 통화 { $count }건
    }
tray-idle = Relay: 대기 중
tray-show = Relay 열기
tray-quit = 이 앱 종료 (데몬은 계속 실행)
