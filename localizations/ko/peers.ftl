# Relay desktop UI catalog, Korean (ko), section: People nearby.
# Terms: call = 통화, hang up = 끊기, mute = 음소거, deafen = 듣기 끄기, peer = 사용자, channel = 채널,
# invite = 초대, fingerprint = 지문, trust = 신뢰, avatar = 프로필 사진, file offer = 파일 제안.
# See docs/i18n.md.

peer-unknown = 알 수 없는 기기
peer-online = 온라인
peer-unreachable = 오프라인
peer-key = 키 { $fingerprint }
peer-shared-channels = 공유 채널: { $names }
peer-call = 통화
revoke-ask = { $channel }에서 { $name } 님을 제거할까요?
revoke-yes = 예, 제거
revoke-keep = 유지
revoke-from = { $channel }에서 제거
home-nearby = 근처
people-none-nearby = 이 네트워크에서 아직 기기를 찾지 못했습니다.
people-none-nearby-help = 두 기기가 같은 Wi-Fi에 연결되어 있고 Relay가 열려 있어야 합니다. 시스템에서 요청하면 Relay의 로컬 네트워크 접근을 허용하세요. 일부 공유기는 기기끼리 서로 보이지 않게 막습니다(클라이언트 격리 또는 게스트 네트워크). 그래도 안 되면 아래의 기기 연결을 사용하세요.
people-add-title = 주소로 기기 추가
people-add-help = 다른 기기의 기기 연결에 표시된 주소를 붙여 넣으세요.
peer-call-blocked = 지금은 이 기기에 전화할 수 없습니다.
peer-no-key = 이 기기의 키가 아직 확인되지 않았습니다.
peer-key-help = 상대를 확인하려면 이 키를 다른 기기의 키와 비교하세요.
forget-device = 이 기기 삭제
forget-ask = { $name } 기기를 삭제할까요? 더 이상 신뢰하지 않게 됩니다.
forget-yes = 삭제
sidebar-you = 나
connect-title = 기기 연결
visibility-discoverable = 검색 가능
visibility-discoverable-help = 같은 네트워크의 기기가 이 기기를 찾아 전화할 수 있습니다.
visibility-private = 비공개
visibility-private-help = 이 기기는 네트워크에서 숨겨져 있습니다. 내 주소를 가진 사람만 전화할 수 있습니다.
visibility-not-advertising = 알리지 않음
visibility-not-advertising-reason = 알리지 않음: { $reason }
visibility-go-private = 비공개로 전환
visibility-go-discoverable = 검색 가능하게 하기
people-found-help = 이 기기들은 네트워크에 있지만 아직 신뢰하지 않았습니다. 누구인지 확인한 뒤에만 신뢰하세요.
found-trust = 이 기기 신뢰
found-trust-ask = { $name }을(를) 신뢰하기 전에 이 지문을 그 기기에 표시된 지문(기기 연결, 자세히)과 비교하세요. 같을 때만 신뢰하세요.
found-trust-yes = 예, 신뢰
found-trust-cancel = 나중에
found-subtitle = 아직 신뢰 안 함 · { $fingerprint }
contacts-empty-title = 아직 연락처가 없습니다
contacts-empty-help = 연락처는 신뢰한 기기입니다. 연결에서 기기를 찾아 신뢰하면 여기에 표시됩니다.
contacts-go-connect = 연결로 이동
contact-more = 옵션 더보기
peer-addresses = 주소: { $addresses }
contact-last-seen = 마지막 확인: { $when }
peer-port = 포트 { $port }
contact-favorite-add = 즐겨찾기에 추가
contact-favorite-remove = 즐겨찾기에서 삭제

# 키가 바뀜: 신뢰하던 기기가 다른 보안 키를 보여 줍니다.
contact-key-changed-label = 보안 키가 바뀌었습니다
keychange-title = { $name }의 보안 키가 바뀌었습니다
keychange-body = 해당 기기에서 Relay를 다시 설치했거나 초기화했을 때 흔히 생깁니다. 다시 확인할 때까지 통화할 수 없습니다.
keychange-scan = 상대의 QR 코드 스캔
keychange-compare = 비교하고 새 키 신뢰
keychange-remove = 연락처 삭제
keychange-remove-ask = { $name }을(를) 연락처에서 삭제할까요? 더 이상 신뢰하지 않게 됩니다.
keychange-remove-yes = 예, 삭제
keychange-compare-intro = { $name }에서 연결, 기기 연결, 자세히 순서로 여세요. 표시된 지문을 입력하거나 붙여 넣으세요. 두 값이 같을 때만 새 키를 신뢰하세요.
keychange-new-key = 이 기기에서 확인한 새 키
keychange-field-hint = 다른 기기의 지문
keychange-match = 일치합니다.
keychange-no-match = 아직 같은 숫자가 아닙니다.
keychange-confirm = 새 키 신뢰
keychange-cancel = 나중에
keychange-previous = 이전 키 { $fingerprint }
found-call-help = 근처 기기에 전화하면 상대가 받는 순간 확인되지 않은 연락처로 추가됩니다. 먼저 지문을 비교하려면 이 기기 신뢰하기를 사용하세요.
block-device = 기기 차단
block-ask = { $name }을(를) 차단할까요? 이 기기는 더 이상 전화하거나 나를 볼 수 없고 연락처에서 삭제됩니다. 설정에서 차단을 해제할 수 있습니다.
block-yes = 차단
unblock = 차단 해제
peer-call-offline = 이 기기는 오프라인입니다. 네트워크에 다시 연결되면 바로 전화할 수 있습니다.
