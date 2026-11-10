# Relay desktop UI catalog, Simplified Chinese (zh-CN), section: Tray.
# Terms: 通话 call, 呼叫 to call, 挂断 hang up, 静音 mute, 关闭声音 deafen, 按住说话 push-to-talk, 后台服务 daemon, 设备 device/peer, 频道 channel,
# 邀请码 invite code, 指纹 fingerprint, 信任 trust, 验证 verify, 屏蔽 block, 来电 incoming call, 头像 avatar, 会话 session, 文件分享 file offer. Chinese has only the "other" plural category.
# See docs/i18n.md.

tray-mock = Relay（模拟数据）
tray-starting = Relay：正在启动后台服务
tray-connecting = Relay：正在连接后台服务
tray-reconnecting = Relay：与后台服务的连接已断开，正在重连
tray-offline = Relay：后台服务未运行
tray-incoming =
    { $count ->
       *[other] Relay：{ $count } 个来电
    }
tray-active =
    { $count ->
       *[other] Relay：{ $count } 个通话进行中
    }
tray-idle = Relay：空闲
tray-show = 显示 Relay
tray-quit = 退出此应用（后台服务继续运行）
tray-status-available = 可接听
tray-status-unavailable = 不可接听
tray-stay-available = 在后台保持可用
