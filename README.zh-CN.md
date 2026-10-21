<div align="center">

# things

English: [README.md](README.md)

**面向 AI 的 macOS Things 3 CLI 与 MCP 服务器**

[![CI](https://img.shields.io/github/actions/workflow/status/guangzan/things/ci.yml?style=flat-square&label=CI)](https://github.com/guangzan/things/actions)
[![Rust](https://img.shields.io/badge/Rust-1.89+-dea584?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/Platform-macOS-black?style=flat-square&logo=apple&logoColor=white)](https://culturedcode.com/things/)
[![License](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE)

[功能](#功能) · [安装](#安装) · [快速开始](#快速开始) · [MCP](#mcp) · [配置](#配置) · [文档](#文档)

</div>

`things` 是面向生产使用的 [Things 3](https://culturedcode.com/things/) 命令行工具与 MCP 服务器。它用 Rust 编写，并围绕严格的安全边界设计：

- **读取**对实时 Things SQLite 数据库以只读方式查询
- **写入**通过官方 Things URL scheme 或 AppleScript 发送，再对照数据库校验
- **不会直接写入数据库**
- 重复规则编辑会在 Things 中打开，而不会写入未公开的内部表

> [!NOTE]
> 本项目为非官方项目，与 Cultured Code 无关联。

## 功能

- 侧边栏视图：Inbox（收件箱）、Today（今天）、Upcoming（即将到来）、Anytime（随时）、Someday（某天）、Logbook（日志）、Trash（废纸篓）、Deadlines（截止日期）、Repeating（重复）和 Projects（项目）
- 支持状态、类型、项目、区域、标题分组、标签、日期范围、逾期、清单、提醒、重复项与废纸篓状态等布尔过滤
- 不区分大小写的文本搜索，以及基于 Rust 正则的标题 / 备注过滤
- 稳定 UUID 解析：完整 id、4 位及以上前缀、精确标题或唯一标题子串；歧义引用返回候选，不会猜测
- 支持创建、部分编辑、完成、取消、重新打开、在项目 / 区域 / 标题分组间移动，以及移入 Trash
- 基于严格 `HH:MM` 输入的时间提醒
- 标签与区域的创建 / 重命名 / 删除，并提供使用计数
- 从文件或标准输入批量导入 Things JSON，并按条目回读身份
- 基于写入前快照与观测到的数据库状态做写入校验
- JSON 与 NDJSON 输出，错误码稳定
- 通过 stdio 提供 MCP，可选只读模式，并返回结构化工具结果
- 面向 AI 的列表结果：备注预览截断到 200 字符，完整文本可通过 `show` / `get` 获取

## 安装

需要 **macOS** 和 **Things 3**。

### Homebrew

```sh
brew install guangzan/tap/things
```

安装的是面向 Apple Silicon 与 Intel Mac 的预编译版本。后续使用
`brew upgrade things` 升级。

### Shell 安装脚本

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/guangzan/things/releases/latest/download/things-installer.sh | sh
```

### 从源码构建

源码构建需要 **Rust 1.89+**：

```sh
cargo install --path crates/things-cli
```

或构建 release 二进制：

```sh
cargo build --release -p things
./target/release/things --help
```

## macOS 权限

1. 为运行 `things` 的终端或应用授予 **Full Disk Access**（完全磁盘访问权限）。
   Things 数据库位于 `~/Library/Group Containers`。
2. 打开 **Things → Settings → General**（设置 → 通用），启用 **Things URLs**。
3. 如果会使用 `set_status`、`delete` 或标签变更，请在 macOS 提示时允许对 Things 的自动化访问。

运行内置诊断：

```sh
things doctor
things doctor --json
```

## 快速开始

```sh
# 读取
things today --json
things search "quarterly" --json
things show <id> --json
things list --status open --tag focus --overdue --json

# 先检查写入内容，不真正发送
things add "Review proposal" \
  --when tomorrow --reminder 09:30 --deadline 2026-10-24 --tags work \
  --dry-run --json

# 写入并校验
things add "Review proposal" --when tomorrow --reminder 09:30 --deadline 2026-10-24
things edit <id> --append-notes "Ask legal for feedback"
things edit <id> --reminder 08:45
things done <id>
things reopen <id>

# 标签、区域与导入
things tag add "deep-work"
things area add "Work"
things import ./payload.json
cat payload.json | things import -

# 破坏性操作始终需要 --yes
things delete <id> --yes
things empty-trash --yes
```

列表输出若需便于流式处理，可使用 `--format ndjson`。
使用 `--read-only` 可在整个进程中禁用所有变更操作。

## MCP

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

只读 agent：

```sh
things mcp --read-only
```

完整工具参考见 [docs/MCP.zh-CN.md](docs/MCP.zh-CN.md)。

## 导入

```sh
things import payload.json
cat payload.json | things import -
things import payload.json --dry-run --json
```

载荷必须是 `things:///json` 可接受的 JSON 数组。

在默认启用校验时，`things import --json` 会按载荷顺序返回两个按条目划分的数组：

- `created`：每个顶层或嵌套的待办 / 项目
- `updated`：每个 `operation: update`

对创建而言，`confirmed` 只证明身份；对更新而言，它覆盖仅改状态的更新。混合字段更新会明确保持未确认。若某条目未确认，请先按标题搜索，再重试，以免重复创建。

> [!TIP]
> 批量导入前建议先用 `--dry-run --json`。完整校验语义见
> [docs/COMMANDS.zh-CN.md](docs/COMMANDS.zh-CN.md#导入)。

## 配置

配置文件：`~/.config/things/config.toml`

```toml
read_only = false

[output]
format = "text"   # text、json 或 ndjson

[write]
verify_timeout_ms = 8000
verify_poll_ms = 250
create_missing_tags = false

[mcp]
read_only = false
default_page_size = 20
max_page_size = 100
```

环境变量会覆盖配置文件：

| 变量 | 用途 |
|---|---|
| `THINGSDB` | 显式指定 `main.sqlite` 路径 |
| `THINGS_AUTH_TOKEN` | URL scheme 认证令牌 |
| `THINGS_READ_ONLY` | `1` / `true` 禁用写入 |
| `THINGS_VERIFY_TIMEOUT_MS` | 校验超时 |
| `THINGS_VERIFY_POLL_MS` | 校验轮询间隔 |

如果存在多个 `ThingsData-*` 目录，数据库发现流程会拒绝猜测。请设置 `--db` 或 `THINGSDB`。

## 文档

- [命令参考](docs/COMMANDS.zh-CN.md)
- [MCP 配置与工具参考](docs/MCP.zh-CN.md)
- [架构与安全模型](docs/ARCHITECTURE.zh-CN.md)
- [兼容性与限制](docs/COMPATIBILITY.zh-CN.md)
- [AI skill](skills/things/SKILL.md)

## 开发

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Fixture 测试不需要 Things 或个人数据。若 Things 数据库不可用，实时只读测试会被跳过。
