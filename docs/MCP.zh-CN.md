# MCP 服务器

English: [MCP.md](MCP.md)

通过 stdio 运行服务器：

```sh
things mcp
```

Codex 配置示例：

```toml
[mcp_servers.things]
command = "/absolute/path/to/things"
args = ["mcp"]
```

通用 JSON 配置示例：

```json
{
  "mcpServers": {
    "things": {
      "command": "/absolute/path/to/things",
      "args": ["mcp"]
    }
  }
}
```

只读 agent：

```json
{
  "mcpServers": {
    "things": {
      "command": "/absolute/path/to/things",
      "args": ["mcp", "--read-only"]
    }
  }
}
```

## 工具

### 读取工具

| 工具 | 用途 |
|---|---|
| `list` | 使用布尔 / 正则过滤列出任意视图 |
| `list_todos` | 只列出待办 |
| `list_projects` | 只列出项目 |
| `search` | 按标题与备注搜索，并支持列表式过滤 |
| `get` | 解析单个条目并返回完整详情 |
| `list_areas` | 区域，含标签与计数 |
| `list_tags` | 标签，含使用计数 |
| `list_headings` | 某项目中的标题分组 |
| `stats` | schema、计数与数据库元数据 |
| `get_agent_brief` | 面向 AI 的操作说明 |

### 列表输入

`list`、`list_todos` 和 `list_projects` 共用一个输入对象。所有过滤条件都是可选的，并以 AND 组合：

```json
{
  "view": "logbook",
  "project": "Q4 Planning",
  "tags_all": ["focus"],
  "text": "report",
  "created_from": "2026-01-01",
  "modified_to": "2026-10-19",
  "completed_after": "2026-10-01",
  "completed_before": "2026-10-19",
  "sort": "stop_date",
  "desc": true,
  "limit": 50
}
```

日期边界使用包含端点的 `YYYY-MM-DD` 值。`completed_after` 与 `completed_before` 匹配完成或取消时间戳，因此最适合与 `view: "logbook"` 一起使用。`heading` 会把结果限制到选定项目中的一个标题分组。没有 `view`、也没有 `trashed` 的调用只覆盖活跃条目；设置 `"trashed": true` 或使用 `trash` 视图才会包含 Things Trash。

重复模板，以及重复项目模板内部的任务，会被排除在普通活跃列表之外，即使显式设置了 `"trashed": false`。使用 `"view": "repeating"` 列出模板。显式 `"trashed": true` 列表与 logbook 会按存储行原样报告，模板也会包含在内。当生成实例与其模板共享同一标题时，`get` 以及所有基于标题的写入工具都会解析到生成实例；要直接定位模板，请使用模板的 UUID。

`types` 接受 `"todo"`、`"project"` 和 `"heading"`。标题分组永远不是隐式列表的一部分；只有显式请求 `"heading"` 时才会出现。

`list` 与 `search` 最多返回 200 字符的备注，并在文本被缩短时设置 `notes_truncated: true`。`get` 始终返回完整备注。设置 `include_notes: true` 可在列表中请求完整备注。

`search` 接受 `query`，以及与 `list` 相同的范围过滤：`view`、`status`、`types`、`project`、`area`、`heading`、`tag`、`tags_all`、`tags_any`、`tags_none`、`include_closed_project_tasks`、`sort`、`desc`、`limit`、`offset`、`include_notes` 和 `include_checklist`。这可以在不必先取回全部匹配的情况下收窄文本搜索。`query` 必填，且必须包含非空白文本。

活跃列表会隐藏留在已完成、已取消或已入废纸篓项目中的未完成待办，行为与 Things 侧边栏一致。设置 `"include_closed_project_tasks": true` 可包含这些已归档上下文。

引用（`target`、`project`、`area`、`heading` 与标签名）按完整 id、至少四位的 id 前缀、精确标题或唯一标题子串解析。Things Trash 中的条目可通过 id 访问，但永远不会按标题匹配，因此模糊引用不会静默命中用户已删除的内容。容器引用按类型限定：`project` 只匹配项目，`area` 只匹配区域，标签过滤只匹配标签。对标签过滤而言，解析后的规范标签标题会发送给 `SQLite`，因此 id 前缀与标签名的行为一致。

### 写入工具

当 `read_only = true` 或使用 `things mcp --read-only` 时，这些工具会被隐藏。

| 工具 | 用途 |
|---|---|
| `add_todo` | 创建并校验待办 |
| `add_project` | 创建并校验项目 |
| `edit` | 部分编辑待办或项目 |
| `set_status` | 打开、完成或取消 |
| `delete` | 移入 Things Trash；需要 `options.yes` |
| `empty_trash` | 永久清空 Things Trash；需要 `options.yes` |
| `add_tag` | 创建标签 |
| `rename_tag` | 重命名标签 |
| `delete_tag` | 删除标签；需要 `options.yes` |
| `add_area` | 创建区域 |
| `rename_area` | 重命名区域 |
| `delete_area` | 删除区域；需要 `options.yes` |
| `import` | 发送 Things JSON 数组并回读已创建条目 |
| `open` | 显示条目或视图 |
| `open_repeat_editor` | 打开官方重复规则编辑器 |

`edit` 接受清单替换与追加字段，以及三个清单状态数组：

```json
{
  "target": "<todo id>",
  "complete_checklist": ["Collect numbers"],
  "cancel_checklist": ["Obsolete step"],
  "reopen_checklist": ["Retry import"]
}
```

清单项按精确标题定位。每个写入工具都接受如下所述的 `options`。

`add_todo`、`add_project` 和 `edit` 接受作为严格 24 小时制 `HH:MM` 字符串的 `reminder`。提醒会编码进 `when`，写作 `date@HH:MM`：创建使用提供的日期或默认今天，编辑则保留条目已有的计划日期。`anytime`、`someday` 和未排程条目返回 `conflict`。设置提醒会替换已有提醒；仅清除提醒返回 `unsupported`。因此 `edit` 暴露了 `clear_reminder`，让 agent 得到明确的类型化失败，而不是静默丢弃请求。

### 导入输入

`import` 在 `data` 中以字符串形式接收 Things JSON URL scheme 载荷：

```json
{
  "data": "[{\"type\":\"to-do\",\"attributes\":{\"title\":\"Buy milk\"}}]",
  "options": {"verify": true}
}
```

启用校验时，结果会按载荷顺序包含创建用的 `created` 数组，以及 `operation: update` 条目用的 `updated` 数组。每个创建条目会标识载荷 `path`、`type`、`title`、`confirmed`，以及确认后的数据库 `id`：

```json
{
  "action": "import",
  "sent": true,
  "verification": {"verified": true},
  "created": [
    {
      "path": "[0].attributes.items[0]",
      "type": "todo",
      "title": "Buy milk",
      "confirmed": true,
      "id": "..."
    }
  ]
}
```

更新条目携带 `path`、`type`、`id`、`title`、`confirmed`、`wanted_status` 和 `observed_status`。缺失目标报告 `target-not-found`；请求的状态未落地报告 `status-not-applied`；状态已落地但还有未校验字段报告 `fields-not-verified`；没有请求状态变更的更新报告 `not-verified`。跳过校验返回 `no-verify`，dry-run 返回 `dry-run`。

创建校验证明身份，而不是完整字段校验：它确认写入前快照之后出现了相同类型与精确标题的新行。当载荷创建多个同类型同标题条目时，它们按创建顺序配对。多余行或相同创建时间戳返回带候选的 `ambiguous`；行数不足返回 `not-found`。更新校验覆盖仅改状态的请求。混合字段更新即使状态已落地也保持未确认，并且顶层 `verification.verified` 为 false。清单项更新只能更改 completed / canceled，并从 `TMChecklistItem` 回读。不要盲目重试未确认条目。标题分组和清单项不包含在 `created` 中。

更新目标会在发送前检查。待办接受 `list` / `list-id`（项目或区域）以及 `heading` / `heading-id`；项目接受 `area` / `area-id`。id 必须命名一个已存在且类型兼容的 Things id。该检查校验的是目标，而不是条目当前父级，因此合法移动会被接受。空 id、对条目类型不适用的字段，以及歧义或缺失目标，都会返回 `conflict`。嵌套待办 / 清单更新必须属于其外层项目 / 待办。

## 写入选项

写入工具接受一个 `options` 对象：

```json
{
  "dry_run": false,
  "verify": true,
  "foreground": false,
  "create_missing_tags": false,
  "strict_tags": false,
  "yes": false
}
```

## 结果形态

成功的写入返回：

```json
{
  "action": "edit_todo",
  "type": "todo",
  "id": "...",
  "title": "...",
  "sent": true,
  "url": "things:///update?...",
  "verification": {
    "verified": true,
    "attempts": 3,
    "elapsed_ms": 742,
    "observed": "..."
  },
  "item": {}
}
```

请把 `verification.verified = false` 视为**已发送但未确认**，而不是确定失败。在重试非幂等写入之前，请先再次读取该条目。

## 错误语义

协议错误保留给畸形 JSON-RPC、未知或已禁用工具，以及不符合工具输入 schema 的参数。后者会作为代码为 `-32602` 的 JSON-RPC 错误返回；它不是工具结果。

业务失败是带有 `isError: true` 与结构化内容的成功 MCP 工具调用：

```json
{
  "code": "ambiguous",
  "message": "multiple items match \"o\"",
  "candidates_truncated": false,
  "candidates": [
    {
      "id": "...",
      "type": "todo",
      "title": "Write report"
    }
  ]
}
```

最多返回 25 个候选。当 `candidates_truncated` 为 `true` 时，实际匹配数多于响应携带的数量；应收窄引用或使用过滤列出，而不是把返回的 id 当作完整集合。

这包括 `not_found`、`ambiguous`、`invalid_input`、`invalid_date`、`conflict`、`unsupported`、`permission_denied`、`auth_required`、`app_unavailable`、`verification_failed`，以及内部或数据库失败。客户端应检查 `isError` 与 `structuredContent.code`，而不是假定传输层失败。

工具注解会给每个读取工具标记 `readOnlyHint: true`。`delete`、`empty_trash`、`delete_tag` 和 `delete_area` 还会设置 `destructiveHint: true`；调用方应请求显式确认。除非 `options.yes` 为 `true`，这些工具会以 `conflict` 失败。

## Things 限制

- 提醒可以设置或替换。它们需要具体日期、`today`、`tomorrow` 或 `evening`；`anytime` 与 `someday` 会被拒绝。
- 创建时若不带 `when`，提醒默认到今天。编辑会保留条目已有的计划日期，并拒绝未排程条目。
- 仅清除提醒不受支持，因为 URL scheme 无法在不改变计划的情况下表达它。请使用 Things 应用。
- 重复条目会拒绝 `when`、`reminder`、`deadline` 和状态变更。
- URL scheme 无法把待办移到 Inbox。
- 重复规则在 Things UI 中编辑。
- 除非 `create_missing_tags` 为 true，否则未知标签会被忽略。
- 标题分组不能创建、重命名或删除；Things 没有提供对应脚本命令。
- `empty_trash` 无法撤销。
- 在不改变计划的前提下移除提醒，以及其他没有脚本命令的属性，都需要使用 Things UI。

详情见 [COMPATIBILITY.zh-CN.md](COMPATIBILITY.zh-CN.md)。
