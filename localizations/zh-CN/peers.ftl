# Relay desktop UI catalog, Simplified Chinese (zh-CN), section: People nearby.
# Terms: 通话 call, 挂断 hang up, 静音 mute, 拒听 deafen, 按键通话 push-to-talk, 对等端 peer, 频道 channel,
# 邀请 invite, 指纹 fingerprint, 信任 trust, 头像 avatar, 会话 session, 文件分享 file offer. Chinese has only the "other" plural category.
# See docs/i18n.md.

peer-unknown = 未知设备
peer-online = 在线
peer-unreachable = 离线
peer-key = 密钥 { $fingerprint }
peer-shared-channels = 共同频道：{ $names }
peer-call = 呼叫
revoke-ask = 将 { $name } 移出 { $channel }？
revoke-yes = 是，移出
revoke-keep = 保留
revoke-from = 移出 { $channel }
home-nearby = 附近
people-none-nearby = 此网络上还没有发现设备。
people-none-nearby-help = 两台设备必须连接同一个 Wi-Fi，并且都已打开 Relay。如果系统询问，请允许 Relay 访问本地网络。有些路由器会阻止设备互相看到（客户端隔离或访客网络）。如果仍然不行，请使用下方的连接设备。
people-add-title = 通过地址添加设备
people-add-help = 粘贴另一台设备在“连接设备”中显示的地址。
peer-call-blocked = 暂时无法呼叫此设备。
peer-no-key = 尚未获得此设备的密钥。
peer-key-help = 将此密钥与对方设备上的密钥比对，以确认通话对象。
forget-device = 忘记此设备
forget-ask = 忘记 { $name }？它将不再受信任。
forget-yes = 是，忘记
sidebar-you = 你
connect-title = 连接设备
visibility-discoverable = 可被发现
visibility-discoverable-help = 同一网络中的设备可以发现并呼叫这台设备。
visibility-private = 私密
visibility-private-help = 这台设备在网络中处于隐藏状态，只有拥有你地址的人才能呼叫你。
visibility-not-advertising = 未广播
visibility-not-advertising-reason = 未广播：{ $reason }
visibility-go-private = 切换为私密
visibility-go-discoverable = 设为可被发现
people-found-help = 这些设备在你的网络中，但你还没有信任它们。确认对方身份后再信任。
found-trust = 信任此设备
found-trust-ask = 信任 { $name } 之前，请把此指纹与该设备上显示的指纹（连接设备，然后详情）进行对比。只有两者相同才信任。
found-trust-yes = 是，信任
found-trust-cancel = 暂不
found-subtitle = 尚未信任 · { $fingerprint }
contacts-empty-title = 还没有联系人
contacts-empty-help = 联系人是你已信任的设备。在“连接”中找到设备并信任它，它就会出现在这里。
contacts-go-connect = 前往“连接”
contact-more = 更多选项
peer-addresses = 地址：{ $addresses }
contact-last-seen = 最后在线：{ $when }
peer-port = 端口 { $port }
contact-favorite-add = 添加到收藏
contact-favorite-remove = 从收藏中移除

# 密钥已更改：受信任的设备出示了另一把安全密钥。
contact-key-changed-label = 安全密钥已更改
keychange-title = { $name } 的安全密钥已更改
keychange-body = 通常是因为那台设备上的 Relay 被重新安装或重置了。在你再次验证之前，通话会被阻止。
keychange-scan = 扫描对方的二维码
keychange-compare = 比对并信任新密钥
keychange-remove = 删除联系人
keychange-remove-ask = 要从联系人中删除 { $name } 吗？将不再信任它。
keychange-remove-yes = 是，删除
keychange-compare-intro = 在 { $name } 上依次打开连接、连接设备、详情。输入或粘贴那里显示的指纹。只有两者一致时才信任新密钥。
keychange-new-key = 此设备看到的新密钥
keychange-field-hint = 另一台设备上的指纹
keychange-match = 两者一致。
keychange-no-match = 这些数字还不一样。
keychange-confirm = 信任新密钥
keychange-cancel = 暂不
keychange-previous = 之前的密钥 { $fingerprint }
found-call-help = 呼叫附近的设备，对方接听后会以“未验证”身份加入联系人。若想先比对指纹，请使用“信任此设备”。
block-device = 屏蔽设备
block-ask = 屏蔽 { $name }？它将无法再呼叫你或看到你，并会从联系人中移除。你可以在设置中解除屏蔽。
block-yes = 屏蔽
unblock = 解除屏蔽
