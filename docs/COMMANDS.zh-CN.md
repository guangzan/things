# 命令参考

English: [COMMANDS.md](COMMANDS.md)

全局选项可以出现在子命令之前或之后：

```text
--db <PATH>          显式指定 Things main.sqlite
--read-only          禁用所有写入
--json               JSON 输出
--format <FORMAT>    text、json 或 ndjson
--dry-run            构建写入但不发送
--no-verify          跳过写入后校验
--foreground         写入时把 Things 带到前台
-y, --yes            确认破坏性或项目级操作
--create-tags        使用未知标签前先创建它们
--strict-tags        拒绝未知标签
```

## 视图

```sh
things today [FILTERS]
things inbox [FILTERS]
things upcoming [FILTERS]
things anytime [FILTERS]
things someday [FILTERS]
things logbook [FILTERS]
things trash [FILTERS]
things deadlines [FILTERS]
things repeating [FILTERS]
things projects [FILTERS]
things list --view today [FILTERS]
```

## 过滤条件

```text
--status open,completed,canceled
--type todo,project,heading
--project <REF>              项目 id、前缀、标题或子串
--area <REF>                 区域 id、前缀、标题或子串
--heading <REF>              标题分组 id 或标题
--tag <NAME>                 要求包含一个标签
--tags-all a,b               要求包含全部标签
--tags-any a,b               要求至少包含一个标签
--tags-none a,b              排除标签
--text <TEXT>                标题 / 备注子串
--title-regex <REGEX>        Rust 正则
--notes-regex <REGEX>        Rust 正则
--created-from/--created-to  YYYY-MM-DD
--modified-from/--modified-to
--completed-after/--completed-before   完成 / 取消日期（Logbook）
--start-from/--start-to
--deadline-from/--deadline-to
--overdue
--has-deadline / --no-deadline
--has-reminder / --no-reminder
--has-tags / --no-tags
--has-checklist / --no-checklist
--trashed / --not-trashed
--closed-projects            包含留在已关闭或已入废纸篓项目中的未完成待办
--repeating / --not-repeating
--sort index|title|created|modified|start_date|deadline|stop_date
--desc
--limit N --offset N
--notes                      包含完整备注（列表默认只给 200 字符预览）
--checklist                  包含清单项
```

没有 `--view`、也没有显式 `--trashed` / `--not-trashed` 的列表只覆盖活跃条目；传入 `--trashed`（或使用 `trash` 视图）才会包含 Things Trash 中的条目。

重复模板，以及存储在重复项目模板内部的任务，属于 `things repeating`，不属于普通活跃列表。无论是否显式传入 `--not-trashed` 都成立：`things list --status open --not-trashed` 仍会省略它们。显式 `--trashed` 列表和 `logbook` 视图会按存储行原样报告，因此其中的模板会被包含。

当生成任务与其重复模板共享同一标题时，基于标题的查找（`show`、`edit`、`done`、`cancel` 和 `reopen`）会优先选择生成任务。要用 id 才能直接定位模板；只有模板独有的标题仍可解析到模板。

活跃视图会隐藏那些在其项目被完成、取消或移入废纸篓后留下的未完成待办，行为与 Things 一致。传入 `--closed-projects` 可包含这些已归档上下文，例如审计已删除项目时。`trash` 视图同样只列出应用会显示的已入废纸篓项目与独立待办，而不是某个已入废纸篓项目下的每一条子行。

`--type heading` 会返回所有项目中的标题分组。标题分组永远不会出现在隐式列表中；需要按类型显式请求。

列表、搜索和视图输出对每条备注最多携带 200 字符，并在文本被截断时设置 `notes_truncated: true`。传入 `--notes` 可在列表中拿到完整备注；`things show` 始终返回完整备注。

### 默认排序

`--sort index` 是默认值，表示“Things 自身对该选定视图展示的顺序”，而不是原始数据库列：

| 视图 | 自然顺序 |
|---|---|
| `today` | 普通条目在前，然后是 This Evening；组内先按最新的 `todayIndexReferenceDate`，再按 `todayIndex` |
| `upcoming` | 最近的开始日期，然后是最近的截止日期；无日期条目排最后 |
| `deadlines` | 最近的截止日期优先 |
| `logbook` | 最近完成或取消的优先（`stopDate` 降序） |
| 其他视图 | 存储的列表位置 |

每种顺序最后都会用稳定 id 做并列打破，因此用 `--limit` / `--offset` 分页时，相等键不会被重排。`--desc` 返回选定顺序的反序。

## 读取与搜索

```sh
things search "query" [FILTERS]
things show <ID|PREFIX|TITLE>
things area
things area list
things headings <PROJECT>
things stats
things agent-brief
things doctor
```

歧义引用会返回带候选的错误。请使用更长的 id 前缀或完整 id。最多返回 25 个候选；`candidates_truncated: true` 表示还有更多匹配，应收窄引用，而不是假定列表完整。引用按完整 id、至少四位的 id 前缀、精确标题或唯一标题子串解析；Things Trash 中的条目仍可通过 id 访问，但永远不会按标题匹配。

过滤引用是按类型限定的：`--project` 只匹配项目，`--area` 只匹配区域，标签过滤只匹配标签。当区域与标签同名时，这一点尤其重要。

## 创建与编辑

```sh
things add "Title" \
  --notes "Details" \
  --when tomorrow \
  --reminder 09:30 \
  --deadline 2026-10-24 \
  --tags work,focus \
  --checklist "First step" \
  --project <PROJECT> \
  --heading <HEADING>

things edit <TARGET> \
  --title "New title" \
  --append-notes "More detail" \
  --when tomorrow \
  --reminder 09:30 \
  --deadline 2026-10-24 \
  --add-tags urgent \
  --append-checklist "Another step" \
  --complete-checklist "First step"
```

使用 `--clear-when` 或 `--clear-deadline` 可清除这些字段。仅在合适时使用 `--status completed` 或 `--status canceled`；状态专用变更更适合用 `done`、`cancel` 和 `reopen`。

`--reminder HH:MM` 会为待办或项目设置或替换提醒。值必须是严格的 24 小时制时间；`9:00` 和 `24:00` 会被拒绝。它会在 Things URL 中编码为 `when=date@HH:MM`。若带有 `--when`，提醒使用该日期。创建时若不带 `--when`，默认是今天。编辑时则保留条目当前的计划日期，因此设置提醒不会把条目挪到今天。`anytime`、`someday` 和未排程条目不能携带提醒。仅清除提醒不受 URL scheme 支持；请改用 Things 应用。重复条目会拒绝提醒变更。

清单项按精确标题定位：

```text
--complete-checklist <TITLE>   将清单项标记为已完成
--cancel-checklist <TITLE>     将清单项标记为已取消
--reopen-checklist <TITLE>     重新打开已完成或已取消的清单项
```

每个选项都可以重复。缺失或歧义的清单标题会在发送写入前失败。

清单状态变更会作为单独的 Things JSON 更新发送，以便从 `TMChecklistItem` 回读结果。它们不能与其他编辑合并到同一条命令中。

## 项目

```sh
things project add "Launch" \
  --area Work \
  --todo "Draft plan" \
  --todo "Review plan"

things project edit <PROJECT> --append-notes "Kickoff notes"
```

关闭或重新打开项目需要 `--yes`，因为它会影响项目内的每项任务。

## 状态与删除

```sh
things done <TARGET>
things cancel <TARGET>
things reopen <TARGET>
things delete <TARGET> --yes
things empty-trash --yes
```

`delete` 会把条目移入 Things Trash。它不会永久抹除数据库行。`empty-trash` 会永久删除 Trash 中的全部内容且无法撤销，因此始终需要 `--yes`。

删除已经在 Trash 中的条目会返回 `conflict` 错误。Things 无法单独定位已入废纸篓的条目，因此 `empty-trash` 是移除它们的唯一方式。

重复条目会拒绝 `when`、`reminder`、`deadline` 和状态变更；请使用 `things repeat <ITEM>` 在 Things 中编辑规则。

## 标签

```sh
things tag list
things tag add "deep-work"
things tag rename <TAG> "new-name"
things tag delete <TAG> --yes
```

标签名不能包含逗号，因为 Things 用它作分隔符。

## 区域

```sh
things area
things area add "Work"
things area rename <AREA> "Deep work"
things area delete <AREA> --yes
```

`things areas` 可作为 `things area list` 的别名。`area add` 在名称已匹配时会返回现有区域，因此不会创建重复项。删除区域会把它移入 Things Trash；其中的项目和待办不会被删除。

## 导入

```sh
things import payload.json
cat payload.json | things import -
things import payload.json --dry-run --json
```

载荷必须是 `things:///json` 可接受的 JSON 数组。

在默认启用校验时，`things import --json` 会为顶层与嵌套的待办 / 项目创建返回 `created` 数组，并为 `operation: update` 条目返回 `updated` 数组，二者都按载荷顺序排列。创建条目包含 `path`、`type`、`title`、`confirmed`，以及找到时的 `id`。更新条目还包含 `id`，以及在请求状态变更时的 `wanted_status` 与 `observed_status`。

创建条目可能报告 `not-found`、`ambiguous`、`no-verify` 或 `dry-run`。更新条目还可能额外报告 `target-not-found`、`status-not-applied`、`fields-not-verified` 或 `not-verified`。对创建而言，`confirmed` 只校验身份：它证明写入前快照之后出现了期望类型和标题的新行，并不证明每个属性都已应用。

对更新而言，`confirmed` 覆盖仅改状态的更新。如果更新把 `completed` 或 `canceled` 与另一个字段组合在一起，状态可能落地，但条目仍会报告 `fields-not-verified`；此时顶层 `verification.verified` 为 false。没有请求状态变更的更新报告 `not-verified`。清单项更新只支持状态，并从 `TMChecklistItem` 校验；其他清单属性会作为 `unsupported` 被拒绝。标题分组和清单项不属于 `created`。

更新目标会独立于条目当前位置做预检，因此合法的移动不会仅仅因为指向别处而失败。待办接受 `list` / `list-id`（项目或区域）以及 `heading` / `heading-id`；项目接受 `area` / `area-id`。id 必须命名一个已存在且类型兼容的 Things id。空 id、对条目类型不适用的字段，以及未知或歧义目标，都会在发送载荷前返回 `conflict`。空的 `list-id` 不会被当作 Inbox。嵌套待办与清单更新必须属于其外层项目或待办。

当条目未确认时，请先按标题搜索，再重试，以免重复创建。

## 打开与重复

```sh
things open <TARGET>
things open --view today
things repeat <TARGET>
```

`repeat` 会在 Things 中打开条目。直接写入重复规则是故意不支持的。

## MCP 与配置

```sh
things mcp
things config path
things config show
```

## 退出码

| 码 | 含义 |
|---:|---|
| 0 | 成功 |
| 1 | 数据库或内部失败 |
| 2 | 无效输入、日期、冲突或不支持的操作 |
| 3 | 权限、认证或 Things 应用不可用 |
| 4 | 未找到或歧义目标 |
| 5 | 写入已发送但校验失败 |
