# Relay desktop UI catalog, Simplified Chinese (zh-CN), section: Errors the UI words itself.
# Terms: 通话 call, 挂断 hang up, 静音 mute, 拒听 deafen, 按键通话 push-to-talk, 对等端 peer, 频道 channel,
# 邀请 invite, 指纹 fingerprint, 信任 trust, 头像 avatar, 会话 session, 文件分享 file offer. Chinese has only the "other" plural category.
# See docs/i18n.md.

err-not-connected = 未连接到 Relay 守护进程
err-release-delay = 松开延迟必须在 0 到 2000 ms 之间
err-gain = 增益必须是 0 到 4 之间的有限数值
err-bad-call-id = 通话 ID 格式不正确
err-unknown-call = 未知通话
err-no-lease = 引擎没有返回控制租约
err-bad-peer-id = 对等端 ID 格式不正确
err-unknown-peer = 未知对等端
err-consent-required = 发送声音需要麦克风授权
err-ring-volume = 铃声音量必须在 0 到 1 之间
err-unknown-device = 未知或不可用的音频设备
err-display-name = 显示名称必须为 1 到 64 个可打印字符
err-avatar-media-type = 头像需要指定媒体类型
conn-scheme = 连接字符串以 relay:// 开头
err-invite-malformed = 该邀请令牌格式不正确
err-join-target = 请输入邀请令牌或 32 位频道 ID
err-bad-channel-id = 频道 ID 格式不正确
err-unknown-channel = 未知频道
err-channel-name = 频道名称必须为 1 到 32 个可打印字符
err-channel-needs-password = 受密码保护的频道需要填写密码
err-enter-password = 请输入频道密码
err-invite-admin-only = 只有所有者或管理员可以创建邀请
err-invite-range = 邀请有效期或使用次数超出范围
err-no-invite = 引擎没有返回邀请
err-revoke-admin-only = 只有所有者或管理员可以移除成员
err-not-member = 该对等端未被证实是此频道的成员
err-nothing-to-change = 没有需要更改的内容
err-bad-avatar-hash = 头像哈希格式不正确
conn-missing-fingerprint = 连接字符串缺少 #指纹 部分。
conn-bad-fingerprint = 指纹必须是 64 位十六进制字符。
conn-bad-address = 地址必须是 ip:port 的字面形式，不能是名称。
watch-resync = 引擎要求重新同步：{ $reason }
watch-closed = 守护进程已关闭连接
err-unsupported = 此引擎不支持该请求
err-daemon-unreachable = 无法连接 Relay 守护进程：{ $reason }
err-engine-timeout = 引擎未在规定时间内响应
err-connection = 连接问题：{ $reason }
err-unreadable-image = 该文件不是可读取的图片。
err-engine-rejected = 引擎拒绝了该请求
err-no-paths = 此设备无法为 Relay 守护进程选择文件或文件夹。
err-cannot-start-daemon = 此设备无法启动 Relay 守护进程。
err-cannot-stop-daemon = 此设备无法停止 Relay 守护进程。
err-read-file = 无法读取文件：{ $error }
