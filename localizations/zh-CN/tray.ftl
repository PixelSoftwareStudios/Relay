# Relay desktop UI catalog, Simplified Chinese (zh-CN), section: Tray.
# Terms: 通话 call, 挂断 hang up, 静音 mute, 拒听 deafen, 按键通话 push-to-talk, 对等端 peer, 频道 channel,
# 邀请 invite, 指纹 fingerprint, 信任 trust, 头像 avatar, 会话 session, 文件分享 file offer. Chinese has only the "other" plural category.
# See docs/i18n.md.

tray-mock = Relay（模拟数据）
tray-starting = Relay：正在启动守护进程
tray-connecting = Relay：正在连接守护进程
tray-reconnecting = Relay：与守护进程的连接已断开，正在重连
tray-offline = Relay：守护进程未运行
tray-incoming =
    { $count ->
       *[other] Relay：{ $count } 个来电
    }
tray-active =
    { $count ->
       *[other] Relay：{ $count } 个进行中的通话
    }
tray-idle = Relay：空闲
tray-show = 显示 Relay
tray-quit = 退出此应用（守护进程继续运行）
tray-status-available = 可接听
tray-status-unavailable = 不可接听
tray-stay-available = 在后台保持可用
