# Relay desktop UI catalog, Korean (ko), section: Errors the UI words itself.
# Terms: call = 통화, hang up = 끊기, mute = 음소거, deafen = 듣기 끄기, peer = 사용자, channel = 채널,
# invite = 초대, fingerprint = 지문, trust = 신뢰, avatar = 프로필 사진, file offer = 파일 제안.
# See docs/i18n.md.

err-not-connected = Relay 데몬에 연결되어 있지 않습니다
err-release-delay = 해제 지연은 0에서 2000 ms 사이여야 합니다
err-gain = 감도는 0에서 4 사이의 유한한 수여야 합니다
err-bad-call-id = 통화 ID 형식이 잘못되었습니다
err-unknown-call = 알 수 없는 통화입니다
err-no-lease = 엔진이 제어 권한을 반환하지 않았습니다
err-bad-peer-id = 사용자 ID 형식이 잘못되었습니다
err-unknown-peer = 알 수 없는 사용자입니다
err-ring-volume = 벨소리 음량은 0에서 1 사이여야 합니다
err-unknown-device = 알 수 없거나 사용할 수 없는 오디오 장치입니다
err-display-name = 표시 이름은 표시 가능한 문자 1~64자여야 합니다
err-avatar-media-type = 프로필 사진에는 미디어 형식이 필요합니다
conn-scheme = 연결 문자열은 relay://로 시작합니다
err-invite-malformed = 초대 토큰 형식이 잘못되었습니다
err-join-target = 초대 토큰 또는 32자 채널 ID를 입력하세요
err-bad-channel-id = 채널 ID 형식이 잘못되었습니다
err-unknown-channel = 알 수 없는 채널입니다
err-channel-name = 채널 이름은 표시 가능한 문자 1~32자여야 합니다
err-channel-needs-password = 비밀번호로 보호된 채널에는 비밀번호가 필요합니다
err-enter-password = 채널 비밀번호를 입력하세요
err-invite-admin-only = 소유자 또는 관리자만 초대를 만들 수 있습니다
err-invite-range = 초대 유효 기간 또는 사용 횟수가 허용 범위를 벗어났습니다
err-no-invite = 엔진이 초대를 반환하지 않았습니다
err-revoke-admin-only = 소유자 또는 관리자만 멤버를 제거할 수 있습니다
err-not-member = 해당 사용자는 이 채널의 확인된 멤버가 아닙니다
err-nothing-to-change = 변경할 내용이 없습니다
err-bad-avatar-hash = 프로필 사진 해시 형식이 잘못되었습니다
conn-missing-fingerprint = 연결 문자열에 #지문 부분이 없습니다.
conn-bad-fingerprint = 지문은 16진수 64자여야 합니다.
conn-bad-address = 주소는 이름이 아닌 ip:port 형식이어야 합니다.
watch-resync = 엔진이 재동기화를 요청했습니다: { $reason }
watch-closed = 데몬이 연결을 닫았습니다
err-unsupported = 이 엔진은 해당 요청을 지원하지 않습니다
err-daemon-unreachable = Relay 데몬에 접속할 수 없습니다: { $reason }
err-engine-timeout = 엔진이 제때 응답하지 않았습니다
err-connection = 연결 문제: { $reason }
err-unreadable-image = 읽을 수 있는 이미지 파일이 아닙니다.
err-engine-rejected = 엔진이 요청을 거부했습니다
err-no-paths = 이 기기에서는 Relay 데몬용 파일이나 폴더를 선택할 수 없습니다.
err-cannot-start-daemon = 이 기기에서는 Relay 데몬을 시작할 수 없습니다.
err-cannot-stop-daemon = 이 기기에서는 Relay 데몬을 중지할 수 없습니다.
err-read-file = 파일을 읽을 수 없습니다: { $error }
err-fingerprint-mismatch = 이 기기의 새 키 지문이 아닙니다.
err-trust-full = 저장된 기기 한도({ $limit }대)에 도달했습니다. 연락처에서 일부를 삭제하세요.
err-save-failed = Relay가 이 변경을 저장하지 못했습니다. 다시 시도하거나 앱을 다시 시작하세요.
err-trust-unreachable = 신뢰하기 위해 해당 기기에 연결하지 못했습니다. 같은 네트워크에서 Relay가 열려 있는지 확인한 뒤 다시 시도하세요.
err-trust-mismatch = 해당 기기는 표시된 키를 가지고 있지 않아 신뢰하지 않았습니다. 아무것도 저장되지 않았습니다.
