# Relay desktop UI catalog, Korean (ko), section: Channels, privacy and the connection string.
# Terms: call = 통화, hang up = 끊기, mute = 음소거, deafen = 듣기 끄기, peer = 사용자, channel = 채널,
# invite = 초대, fingerprint = 지문, trust = 신뢰, avatar = 프로필 사진, file offer = 파일 제안.
# See docs/i18n.md.

channels-heading = 채널
channels-note = 채널은 누가 나를 보고 전화할 수 있는지를 정합니다. 마이크를 켜지는 않습니다.
channels-none = 참가 중인 채널이 없습니다.
channel-listed = 공개 목록
channel-password = 비밀번호
channel-invite-only = 초대 전용
channel-summary = { $role } · { $flags } · { $peers ->
       *[other] 공유 기기 { $peers }대
    }
channel-invite = 초대…
channel-leave-yes = 예, 나가기
channel-leave-stay = 머무르기
channel-leave = 나가기
invite-uses = { $count ->
       *[other] { $count }회 사용
    }
invite-create = 초대 만들기
invite-token-note = 초대 코드(비밀로 유지하세요). 가진 사람은 누구나 만료되거나 소진될 때까지 참가할 수 있습니다:
channel-create-heading = 채널 만들기
channel-name-hint = 채널 이름
channel-listed-label = 공개 목록: 멤버 기기에 이름 표시
channel-password-label = 참가 시 비밀번호 필요
channel-invite-only-label = 초대 전용
password-hint = 비밀번호
channel-join-heading = 채널 참가
channel-join-note = 초대 코드를 붙여넣거나, 채널 ID와 비밀번호를 입력하세요.
channel-join-hint = 초대 코드 또는 채널 ID
password-optional-hint = 비밀번호 (초대 시에는 불필요)
privacy-none = 데몬에 연결되면 개인정보 보호 설정이 표시됩니다.
privacy-inbound-anyone = 네트워크의 누구나
privacy-inbound-shared = 내 채널 멤버만
privacy-who-rings = 나를 호출할 수 있는 사람
privacy-advert-visible = 근처 기기에 표시
privacy-advert-hidden = 숨김 (연결 문자열로만 접속 가능)
privacy-visibility = 네트워크 공개 범위
invite-ttl-hour = 1시간
invite-ttl-day = 1일
invite-ttl-week = 7일
connection-none = 공유할 주소를 찾지 못했습니다. Relay가 실행 중인지 확인한 뒤 다시 시도하세요.
connection-share = 네트워크의 다른 기기에서 내게 전화하려면 이 주소를 공유하세요:
connection-addresses = 주소: { $addresses } · 포트 { $port }
connection-fingerprint = 지문 { $fingerprint }
connection-warning = 이 주소가 있으면 누구나 나에게 전화할 수 있습니다. 그렇다고 신뢰받는 것은 아니며 마이크에 접근할 수도 없습니다.
connection-qr-too-long = 주소가 너무 길어 QR 코드로 만들 수 없습니다.
channel-hidden = 숨김
channels-none-help = 채널을 만들어 여러 사람과 함께 이야기하거나 초대로 참가하세요.
connection-copy = 주소 복사
connection-error-next = Relay가 백그라운드에서 실행 중인지 확인한 뒤 다시 시도하세요.
connection-qr-help = 다른 기기로 이 코드를 스캔하면 이 기기를 추가할 수 있습니다.
privacy-who-rings-help = 누가 내게 전화를 걸 수 있는지 정합니다.
privacy-visibility-help = 숨겨진 기기에도 연결 문자열로 전화할 수 있습니다.
privacy-messages = 메시지
privacy-trusted = 신뢰하는 기기
privacy-trusted-none = 신뢰하는 기기가 아직 없습니다.
privacy-trusted-help = 신뢰를 끝내려면 기기를 삭제하세요.
connection-scan = QR 코드 스캔
scan-wrong = 이 QR 코드는 Relay 주소가 아닙니다.
scan-camera-denied = 코드를 스캔하려면 Relay에 카메라 권한이 필요합니다. 허용한 뒤 다시 시도하세요.
scan-camera-blocked = Relay의 카메라 접근이 꺼져 있습니다. 스캔하려면 시스템 설정에서 켜세요.
scan-unavailable = 이 기기에서는 QR 코드를 스캔할 수 없습니다. 대신 주소를 복사하세요.
scan-failed = 스캔이 되지 않았습니다. 다시 시도하거나 주소를 복사하세요.
scan-help = 다른 기기에 표시된 코드를 스캔하면 연락처에 추가됩니다.
scan-trusting = 연결하고 신뢰하는 중…
scan-added = { $name }을(를) 연락처에 추가했습니다.
scan-added-unnamed = 기기를 연락처에 추가했습니다.
scan-unreachable = 해당 기기에 연결할 수 없습니다. 같은 네트워크에서 Relay가 열려 있는지 확인한 후 다시 스캔하세요.
scan-mismatch = 해당 기기가 코드와 일치하지 않아 신뢰하지 않았습니다.
scan-trust-failed = 기기를 추가할 수 없습니다: { $reason }
privacy-blocked = 차단한 기기
privacy-blocked-none = 차단한 기기가 없습니다.
privacy-blocked-help = 차단한 기기는 전화할 수 없고 근처 기기로도 표시되지 않습니다. 차단을 해제하면 다시 발견될 수 있지만 연락처가 되지는 않습니다.
