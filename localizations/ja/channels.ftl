# Relay desktop UI catalog, Japanese (ja), section: Channels, privacy and the connection string.
# Project Fluent. Same ids and $variables as localizations/en/channels.ftl. See docs/i18n.md.

channels-heading = チャンネル
channels-note = チャンネルは、誰があなたを見つけて通話できるかを決めます。マイクがオンになることはありません。
channels-none = どのチャンネルにも参加していません。
channel-listed = 公開
channel-password = パスワード
channel-invite-only = 招待制
channel-summary = { $role } · { $flags } · 共通のピア { $peers ->
       *[other] { $peers } 人
    }
channel-invite = 招待…
channel-leave-yes = はい、退出する
channel-leave-stay = 残る
channel-leave = 退出
invite-uses = { $count ->
       *[other] { $count } 回
    }
invite-create = 招待を作成
invite-token-note = 招待トークン(秘密情報)。有効期限が切れるか使い切られるまで、持っている人は誰でも参加できます:
channel-create-heading = チャンネルを作成
channel-name-hint = チャンネル名
channel-listed-label = 公開: メンバーのデバイスに名前を表示する
channel-password-label = 参加にパスワードを必要とする
channel-invite-only-label = 招待制
password-hint = パスワード
channel-join-heading = チャンネルに参加
channel-join-note = 招待トークン、またはチャンネル ID とそのパスワードを貼り付けてください。
channel-join-hint = RLYI1… またはチャンネル ID
password-optional-hint = パスワード(招待には不要)
privacy-none = プライバシー設定はデーモンに接続すると表示されます。
privacy-inbound-anyone = ネットワーク上の全員
privacy-inbound-shared = 自分のチャンネルのメンバーのみ
privacy-who-rings = 着信を許可する相手
privacy-advert-visible = 近くのデバイスに表示
privacy-advert-hidden = 非表示(接続文字列でのみ接続可能)
privacy-visibility = ネットワークでの表示
invite-ttl-hour = 1時間
invite-ttl-day = 1日
invite-ttl-week = 7日間
connection-none = デーモンから接続文字列が報告されませんでした。
connection-share = ネットワーク上の別のコンピューターから通話してもらうには、この文字列を共有してください:
connection-addresses = アドレス: { $addresses } · ポート { $port }
connection-fingerprint = フィンガープリント { $fingerprint }
connection-warning = この文字列を知っている人は誰でもあなたを呼び出せます。信頼やマイクへのアクセスは与えられません。
connection-qr-too-long = 接続文字列が長すぎて QR コードにできません。
channel-hidden = 非公開
channels-none-help = チャンネルを作成して複数人で話すか、招待で参加してください。
connection-copy = アドレスをコピー
connection-error-next = デーモンが動いているか確認して、もう一度試してください。
connection-qr-help = 別のデバイスでこのコードを読み取ると、あなたに発信できます。
privacy-who-rings-help = 誰があなたに発信できるかを決めます。
privacy-visibility-help = 非表示のデバイスにも、接続文字列を使えば発信できます。
privacy-messages = メッセージ
privacy-trusted = 信頼済みデバイス
privacy-trusted-none = 信頼済みのデバイスはまだありません。
privacy-trusted-help = 信頼をやめるには、そのデバイスを削除します。
connection-scan = QRコードをスキャン
scan-wrong = このQRコードはRelayのアドレスではありません。
scan-camera-denied = コードを読み取るには、Relayがカメラを使う必要があります。許可してもう一度お試しください。
scan-camera-blocked = Relayのカメラへのアクセスがオフです。読み取るにはシステム設定でオンにしてください。
scan-unavailable = このデバイスではQRコードを読み取れません。代わりにアドレスをコピーしてください。
scan-failed = 読み取れませんでした。もう一度お試しいただくか、アドレスをコピーしてください。
scan-help = 相手のデバイスに表示されたコードをスキャンすると、連絡先に追加されます。
scan-trusting = 接続して信頼しています…
scan-added = { $name } を連絡先に追加しました。
scan-added-unnamed = デバイスを連絡先に追加しました。
scan-unreachable = そのデバイスに接続できませんでした。同じネットワークで Relay が開いているか確認して、もう一度スキャンしてください。
scan-mismatch = そのデバイスはコードと一致しない（フィンガープリントが異なる）ため、信頼しませんでした。
scan-trust-failed = デバイスを追加できませんでした: { $reason }
