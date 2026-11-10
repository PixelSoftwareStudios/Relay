# Relay desktop UI catalog, zh-CN, section: Storage and limits (limits.* settings). Same ids and $variables as localizations/en/limits.ftl. See docs/i18n.md.

settings-limits = 存储与限制
settings-limits-sub = 缓存大小和保留时长
limits-intro = 这些限制决定此设备保留多少内容。所有内容都只保存在此设备上。默认值适合大多数人。更改会立即生效，正在使用的内容不会被截断。
limits-group-pictures = 图片
limits-group-chats = 聊天和文件
limits-group-history = 通话记录
limits-unit-mib = MiB
limits-unit-entries =
    { $count ->
       *[other] 条
    }
limits-unit-days =
    { $count ->
       *[other] 天
    }
limits-unit-transfers =
    { $count ->
       *[other] 个传输
    }
limits-unit-calls =
    { $count ->
       *[other] 次通话
    }
limits-unit-kib = KiB
limits-unit-seconds =
    { $count ->
       *[other] 秒
    }
limits-avatar-label = 图片缓存
limits-avatar-help = 与你通话的人的图片会保存在这里，以便快速显示。缓存满时，会优先删除最久未使用的图片；你自己的图片不会被删除。调低上限会立即删除多出的图片。
limits-avatar-used = 已用：{ $used } MiB
limits-session-label = 单个进行中聊天的内存
limits-session-help = 单个进行中通话的聊天最多可占用的内存。用满后，该聊天将拒收新消息和文件；已有内容不会被删除。
limits-session-used = 最大的进行中聊天：{ $used } MiB
limits-retained-label = 已结束聊天的内存
limits-retained-help = 已结束通话的聊天在 Relay 退出前仍可查看。占用超过此值时，会优先删除最早结束的聊天。
limits-retained-used = 已结束聊天占用 { $used } MiB
limits-entries-label = 通话记录条数
limits-entries-help = 通话记录最多保留的通话数。超出时会删除最早的记录。0 表示不保留。
limits-age-label = 通话保留时长
limits-age-help = 超过此时长的通话会从记录中删除。
limits-transfers-label = 同时传输的文件数
limits-transfers-help = 一次通话中可同时发送或接收的文件数。超出时，新文件会被拒绝，直到有文件传输完成。
limits-calls-label = 保留的已结束聊天
limits-calls-help = 内存中保留最近这么多次已结束通话的聊天，直到你退出 Relay；更早的会优先删除。聊天永远不会保存到磁盘。0 表示不保留。
limits-ringtone-size-label = 铃声文件大小上限
limits-ringtone-size-help = 可设为铃声的音频文件大小上限。超过此大小的文件会被拒绝。
limits-ringtone-length-label = 铃声时长上限
limits-ringtone-length-help = 可设为铃声的声音时长上限。超过的会被拒绝，而不会被截断。
limits-check-range = { $label }必须在 { $min } 到 { $max } { $unit }之间
limits-check-pair = 已结束聊天的内存不能小于单个进行中聊天的内存。
limits-reset = 恢复默认值
limits-nothing = 没有需要更改的内容。
limits-applied = { $label }：已立即生效
limits-saved = { $label }：已保存，重启后生效
limits-saved-reason = { $label }：已保存，重启后生效（{ $reason }）
limits-rejected = { $label }：已拒绝
limits-rejected-reason = { $label }：已拒绝（{ $reason }）
