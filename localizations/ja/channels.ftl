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
privacy-heading = プライバシー
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
connection-heading = 接続
connection-none = デーモンから接続文字列が報告されませんでした。
connection-share = ネットワーク上の別のコンピューターから通話してもらうには、この文字列を共有してください:
connection-addresses = アドレス: { $addresses } · ポート { $port }
connection-fingerprint = フィンガープリント { $fingerprint }
connection-warning = この文字列を知っている人は誰でもあなたを呼び出せます。信頼やマイクへのアクセスは与えられません。
connection-qr-too-long = 接続文字列が長すぎて QR コードにできません。
connection-call = 接続文字列で通話
channel-hidden = 非公開
