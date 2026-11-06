# Relay desktop UI catalog, Simplified Chinese (zh-CN), section: People nearby.
# Terms: 通话 call, 挂断 hang up, 静音 mute, 拒听 deafen, 按键通话 push-to-talk, 对等端 peer, 频道 channel,
# 邀请 invite, 指纹 fingerprint, 信任 trust, 头像 avatar, 会话 session, 文件分享 file offer. Chinese has only the "other" plural category.
# See docs/i18n.md.

peer-unknown = 未知对等端
peer-online = 在线
peer-unreachable = 无法连接
peer-key = 密钥 { $fingerprint }
peer-shared-channels = 共同频道：{ $names }
peer-call = 呼叫
revoke-ask = 将 { $name } 移出 { $channel }？
revoke-yes = 是，移出
revoke-keep = 保留
revoke-from = 移出 { $channel }
home-nearby = 附近
home-empty-title = 还没有可呼叫的人
home-empty-body = Relay 会自动发现本地网络中的设备。如果没有出现，请用连接字符串添加设备。
home-add-by-address = 添加设备
people-none-nearby = 此网络上还没有发现设备。
people-none-nearby-help = 两台设备必须连接同一个 Wi-Fi，并且都已打开 Relay。如果系统询问，请允许 Relay 访问本地网络。有些路由器会阻止设备互相看到（客户端隔离或访客网络）。如果仍然不行，请使用上方的连接设备。
people-known = 其他受信任的设备
people-known-help = 你以前信任过这些设备，但它们现在不在此网络上。
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
people-found-title = 附近发现的设备
people-found-help = 这些设备在你的网络中，但你还没有信任它们。确认对方身份后再信任。
found-trust = 信任此设备
found-trust-ask = 信任 { $name } 之前，请把此指纹与该设备上显示的指纹（连接设备，然后详情）进行对比。只有两者相同才信任。
found-trust-yes = 是，信任
found-trust-cancel = 暂不
found-subtitle = 尚未信任 · { $fingerprint }
found-review = 查看
