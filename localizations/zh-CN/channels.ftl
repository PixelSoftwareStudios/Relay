# Relay desktop UI catalog, Simplified Chinese (zh-CN), section: Channels, privacy and the connection string.
# Terms: 通话 call, 挂断 hang up, 静音 mute, 拒听 deafen, 按键通话 push-to-talk, 对等端 peer, 频道 channel,
# 邀请 invite, 指纹 fingerprint, 信任 trust, 头像 avatar, 会话 session, 文件分享 file offer. Chinese has only the "other" plural category.
# See docs/i18n.md.

channels-heading = 频道
channels-note = 频道决定谁能看到并呼叫你，不会开启你的麦克风。
channels-none = 你尚未加入任何频道。
channel-listed = 公开列出
channel-password = 密码
channel-invite-only = 仅限邀请
channel-summary = { $role } · { $flags } · { $peers ->
       *[other] { $peers } 个共同对等端
    }
channel-invite = 邀请…
channel-leave-yes = 是，退出
channel-leave-stay = 留下
channel-leave = 退出
invite-uses = { $count ->
       *[other] { $count } 次使用
    }
invite-create = 创建邀请
invite-token-note = 邀请令牌（机密）。持有者在其过期或用完之前都可以加入：
channel-create-heading = 创建频道
channel-name-hint = 频道名称
channel-listed-label = 公开列出：向成员的设备显示名称
channel-password-label = 加入需要密码
channel-invite-only-label = 仅限邀请
password-hint = 密码
channel-join-heading = 加入频道
channel-join-note = 粘贴邀请令牌，或频道 ID 及其密码。
channel-join-hint = RLYI1… 或频道 ID
password-optional-hint = 密码（使用邀请时无需填写）
privacy-none = 守护进程连接后才会显示隐私设置。
privacy-inbound-anyone = 网络中的任何人
privacy-inbound-shared = 仅我的频道成员
privacy-who-rings = 谁可以呼叫我
privacy-advert-visible = 附近设备可见
privacy-advert-hidden = 隐藏（只能通过连接字符串联系）
privacy-visibility = 网络可见性
invite-ttl-hour = 1 小时
invite-ttl-day = 1 天
invite-ttl-week = 7 天
connection-none = 守护进程没有提供连接字符串。
connection-share = 分享此字符串，即可让网络中的其他电脑呼叫你：
connection-addresses = 地址：{ $addresses } · 端口 { $port }
connection-fingerprint = 指纹 { $fingerprint }
connection-warning = 任何持有此字符串的人都可以呼叫你；它不会授予信任，也不会授予麦克风访问权限。
connection-qr-too-long = 连接字符串太长，无法生成二维码。
channel-hidden = 隐藏
channels-none-help = 创建频道可与多人同时通话，或通过邀请加入频道。
connection-copy = 复制地址
connection-error-next = 请确认守护进程正在运行，然后重试。
connection-qr-help = 用另一台设备扫描此二维码，即可呼叫你。
privacy-who-rings-help = 决定谁可以呼叫你。
privacy-visibility-help = 隐藏的设备仍可通过连接字符串被呼叫。
privacy-messages = 消息
privacy-trusted = 受信任的设备
privacy-trusted-none = 你还没有信任任何设备。
privacy-trusted-help = 忘记某台设备即可取消对它的信任。
