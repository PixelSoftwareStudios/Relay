# Relay desktop UI catalog, Japanese (ja), section: Tray menu and tooltip.
# Project Fluent. Same ids and $variables as localizations/en/tray.ftl. See docs/i18n.md.

tray-mock = Relay(モックデータ)
tray-starting = Relay: デーモンを起動中
tray-connecting = Relay: デーモンに接続中
tray-reconnecting = Relay: デーモンとの接続が切れました。再接続中
tray-offline = Relay: デーモンが起動していません
tray-incoming =
    { $count ->
       *[other] Relay: 着信 { $count } 件
    }
tray-active =
    { $count ->
       *[other] Relay: 通話中 { $count } 件
    }
tray-idle = Relay: 待機中
tray-show = Relay を表示
tray-quit = このアプリを終了(デーモンは動作を続けます)
tray-status-available = 応答可能
tray-status-unavailable = 応答不可
tray-stay-available = バックグラウンドで応答可能な状態を保つ
