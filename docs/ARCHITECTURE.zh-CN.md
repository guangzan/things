# 架构

English: [ARCHITECTURE.md](ARCHITECTURE.md)

## 包（Crates）

`things-core` 包含领域模型、只读数据库访问、URL scheme 构建器、`AppleScript` 自动化、校验，以及共享的应用服务。

`things`（位于 `crates/things-cli`）是基于 `clap` 的命令行前端。它把 CLI 参数映射为核心查询与写入请求，并渲染 text、JSON 或 NDJSON。

`things-mcp` 通过 MCP stdio 暴露同一套服务。每次请求都会打开自己的只读 SQLite 连接，避免跨线程共享 `rusqlite::Connection`。

## 安全边界

实时 Things 数据库被当作只读事实来源：

1. 每个连接都使用 `SQLITE_OPEN_READ_ONLY`。
2. 变更通过 `things:///` URL 或 `osascript` 发送。
3. 破坏性操作需要显式确认（`--yes` 或 `options.yes: true`）。
4. 不支持直接写入 SQLite，包括重复规则内部实现。

URL 与 `AppleScript` 是受支持的公开自动化接口。它们仍可能异步：Things 可以在提交到 SQLite 之前先接受请求。因此校验会在有界超时内轮询。

## 写入校验

校验器不会假设已发送的 URL 一定成功。

- 创建会先快照全部已有任务 / 项目 id，只接受新出现的 id。
- 编辑会对照请求字段比较写入前后的条目。
- 状态变更会轮询到精确的目标状态。
- 删除会轮询到条目被标记为已入废纸篓。
- 标签操作会轮询存在性、缺失，或新标题。

`verification.verified = false` 表示请求已发送，但在超时内没有观测到期望的数据库状态。这并不等于断言写入失败。

## 查询模型

`ItemQuery` 由 CLI 与 MCP 共享。视图应用与 Things 侧边栏相同的基础谓词；过滤条件再通过 SQL `AND` 组合。

列表与搜索分页只携带最多 200 字符、且 UTF-8 安全的备注预览；文本被截断时会设置 `notes_truncated`。`get` 会绕过预览，返回完整备注。

`ItemQuery::sort` 默认是 `index`，表示“该视图的自然顺序”。对 `today`、`upcoming` 和 `deadlines`，存储的 `TMTask."index"` 只是列表内位置，没有全局意义，因此查询会改用应用自身排序（见 [COMMANDS.zh-CN.md](COMMANDS.zh-CN.md)）。`Service::list` 中的待办 + 项目合并列表会用同一键在内存中重新排序，因此一个视图不会混用两套排序。

正则过滤在 Rust 中求值，因为 SQLite 默认不提供 regexp 函数。得到的 UUID 集合会以转义后的标识符注入为 `IN` 谓词。

解析顺序：

1. 精确 id。
2. 在待办、项目与标题分组中做不区分大小写的精确标题匹配。
3. 在待办、项目与标题分组中做标题子串匹配。
4. 区域或标签的标题子串，或区域 / 标签 id 前缀。

若仍有多个候选，调用会以 `ambiguous` 失败，并返回稳定 id。

## Things 数据编码

- 状态：`0 = open`，`2 = canceled`，`3 = completed`
- 类型：`0 = todo`，`1 = project`，`2 = heading`
- 计划桶（`start`）：`0 = inbox`，`1 = anytime`，`2 = someday`
- `startBucket` 是单独一列：`1` 表示 This Evening（今晚），不是计划桶
- Today 位置：`todayIndex` 与 `todayIndexReferenceDate`
- 打包日期：`year << 16 | month << 12 | day << 7`
- 提醒时间：`hour << 26 | minute << 20`
- 时间戳：Unix epoch 秒

若配置或环境未提供 URL 认证令牌，会从 `TMSettings` 读取。

## 错误模型

核心错误有稳定类别，例如 `not_found`、`ambiguous`、`invalid_input`、`conflict`、`permission_denied`、`auth_required`、`app_unavailable` 和 `verification_failed`。CLI 会把它们映射为 JSON 错误对象与进程退出码。

MCP 会区分传输层错误与工具结果。未知工具和 schema 解码失败属于协议错误。核心业务失败会返回成功的工具响应，但带有 `isError: true` 与结构化错误数据，这样 agent 可以通过检查候选或修改参数来恢复，而不必把 MCP 会话视为损坏。只读模式会从 `tools/list` 中移除写入工具，也会拒绝直接调用这些工具。
