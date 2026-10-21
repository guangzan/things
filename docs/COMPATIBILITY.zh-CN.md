# 兼容性与限制

English: [COMPATIBILITY.md](COMPATIBILITY.md)

## 测试环境

- 搭载 Things 3 `3.24.1` 的 macOS
- Things 数据库 schema 版本 `29`
- Rust 1.89+（2024 edition）

Things 并未公布稳定的 SQLite schema。读取器使用了防御性的列默认值，但未来的 Things 版本仍可能更改表名或列名。升级 Things 后请运行 `things doctor`。

启动时，读取器会校验它所依赖的每张表和每一列是否存在。若不匹配，会快速失败并返回命名缺失表 / 列的 `unsupported` 错误，而不是静默返回空结果或部分结果。

## 视图排序

`today`、`upcoming` 和 `deadlines` 会忽略存储的 `"index"` 列；那只是列表内位置，对这些按日期划分的视图没有意义。它们会改用应用自身排序（见 [COMMANDS.zh-CN.md](COMMANDS.zh-CN.md) 中的“默认排序”表）。Today 中的 This Evening（今晚）区段由 `startBucket = 1` 标识；Today 还会包含计划日期已经过去的 Someday 条目。

## 受支持的写入路径

| 操作 | 机制 | 已校验 |
|---|---|---|
| 创建待办 | `things:///add` | 观测到新 id，含日期 / 时间提醒 |
| 创建项目 | `things:///add-project` | 观测到新 id，含日期 / 时间提醒 |
| 编辑待办 | `things:///update` | 写入前后字段，含日期 / 时间提醒 |
| 编辑项目 | `things:///update-project` | 写入前后字段，含日期 / 时间提醒 |
| 完成 / 取消 / 重新打开 | `AppleScript` 状态 | 精确状态 |
| 删除 | `AppleScript` delete | 已入废纸篓标志 |
| 清空 Trash | `AppleScript` empty trash | Trash 计数归零 |
| 创建标签 | `AppleScript` | 标签出现 |
| 重命名标签 | `AppleScript` | 新标题出现 |
| 删除标签 | `AppleScript` | 标签消失 |
| 创建区域 | `AppleScript` | 区域出现 |
| 重命名区域 | `AppleScript` | 新标题出现 |
| 删除区域 | `AppleScript` | 区域消失 |
| 批量导入 | `things:///json` | 预校验；按条目回读身份 |

## 已知限制

### 提醒

Things URL scheme 没有单独的 `reminder` 参数，但接受把时间追加到 `when`，写作 `date@HH:MM`。CLI 与 MCP 接受严格的 24 小时制 `HH:MM` 提醒，并按这种方式编码，因此对待办和项目设置或替换提醒是受支持的。

提醒需要具体日期、`today`、`tomorrow` 或 `evening`。`anytime` 与 `someday` 没有可挂提醒的日期，会返回 `conflict`。创建时若省略 `when`，默认使用当前日期。编辑时会保留条目已有的计划日期；若条目没有具体日期，编辑会返回 `conflict`。给已有提醒的条目再设置提醒会替换它。

仅清除提醒不受支持：URL scheme 无法在不改变计划的情况下表达这一点，因此 CLI / MCP 会返回 `unsupported`，并引导用户到 Things 应用中操作。重复条目会拒绝提醒变更，以及 `when`、截止日期和状态变更。

### 重复条目

Things 不允许通过 URL scheme 修改重复条目的 `when`、`deadline` 或状态。工具会返回 `unsupported`，并引导用户到重复规则编辑器。

CLI / MCP 可以从 `rt1_recurrenceRule` 读取重复状态；若列存在，也可以读取遗留的 `repeater` blob。它不会写入这两个未公开字段。

### Inbox

公开 URL scheme 无法把现有待办移回 Inbox。工具不会尝试通过数据库变更来模拟这一点。

### 标签

Things 在 URL 中用逗号分隔标签。包含逗号的标签名会被拒绝。除非启用了标签创建，否则未知标签会被 Things 忽略。

### 删除

删除是 `AppleScript` 操作，会进入 Things Trash。单个条目永远不会被永久抹除；`things empty-trash --yes` 是唯一的永久删除方式，并且始终需要显式确认。

Things 的 `AppleScript` 字典无法定位已经在 Trash 中的条目：`to do id "<id>"` 会以错误 `-1728` 失败，也没有命令能单独暴露 Trash 内容。因此，删除一个已经在 Trash 中的条目会返回 `conflict` 错误，并指向 `things empty-trash --yes`，而不是继续尝试该操作。

### 标题分组（Headings）

Things 没有提供针对标题分组的脚本命令，因此它们可以被读取并用作过滤条件，但不能创建、重命名或删除。标题分组，以及在不改变条目计划的前提下移除提醒，都需要在 Things UI 中编辑。

### 校验

Things 可能异步提交。校验受 `verify_timeout_ms` 限制。超时会报告 `verified = false`；这并不能证明写入失败。

### 批量导入校验

导入载荷在派发前会先校验：根节点必须是 JSON 数组，条目必须有受支持的 `type`，必需字符串字段必须非空，会检查字段类型与重复 JSON 键，且每批最多 200 个创建操作。随后官方 JSON URL 会作为一批发送。

Things 不会为 JSON 导入返回按条目的标识符，因此本工具会在写入后读取数据库。每个被创建的待办和项目都会按类型与精确标题，匹配那些在写入前快照中不存在的行。嵌套待办也会匹配到其父项目（更新项目时按项目 id，新建项目时按标题）。同标题条目按创建顺序配对；缺失、多余或时间戳并列的行会报告为 `not-found` 或带候选的 `ambiguous`。`no-verify` 会把每个期望条目都返回为未确认。

`operation: update` 条目会放在单独的 `updated` 数组中返回。状态变更会轮询到精确请求的状态；缺失行报告 `target-not-found`，状态未落地报告 `status-not-applied`，没有请求状态变更的更新报告 `not-verified`。混合更新（例如 `completed: true` 外加 `notes`）即使观测到状态，也会报告 `fields-not-verified`，并且顶层校验保持为 false。只有仅改状态的更新才能被完全确认。若更新触及 `when`、`deadline`、`completed` 或 `canceled`，重复条目会在派发前被拒绝。

清单项更新通过其父待办解析，并且只能请求 `completed` 或 `canceled`；其状态直接从 `TMChecklistItem` 读取。其他清单字段会作为 `unsupported` 被拒绝。更新目标会在派发前做引用检查。待办接受 `list` / `list-id`（项目或区域）以及 `heading` / `heading-id`；项目接受 `area` / `area-id`。id 必须是已存在且类型兼容的 Things id。空 id，以及对条目类型不适用的目标字段，会是 `conflict` 错误，而不是移入 Inbox 或静默空操作。嵌套待办 / 清单更新必须属于其外层容器。目标不必等于条目当前父级，因为移动条目本身是受支持的更新。

这是尽力而为的身份校验，不是完整字段校验。一条已确认的创建结果只证明出现了具有该类型和标题的新行；并不能证明载荷中的每个属性都已应用。标题分组和清单项不包含在内。请把 `confirmed: false` 视为重试前需要先搜索，而不是条目不存在的证明。

## 只读模式

只读模式会从 MCP 中移除写入工具，并拒绝 CLI 中的写入命令。它也会阻止打开会变更数据的 URL，同时仍允许读取。

## 隐私

本工具读取本地 Things 数据库，从不上传数据。MCP stdio 流量会到达已连接的客户端，因此请自行检查该客户端的隐私设置。
