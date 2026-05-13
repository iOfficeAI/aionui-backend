# Aion CLI 命令接口设计

## 概述

单一二进制 `aion`，基于子命令路由。无子命令时进入交互式 TUI（未来）。统一聚合所有 agent 平台（claude、codex、gemini、aionrs、codebuddy）。

**架构定位：** CLI 是前端客户端，通过 HTTP API 与后端通信。数据持久化、agent 调度、会话管理全部由后端负责。本地仅管理配置文件（credentials、MCP、config）。

---

## 全局选项

```
aion [OPTIONS] [COMMAND]

OPTIONS:
  --data-dir <PATH>     数据目录（默认: ~/.aion/data）
  --config <PATH>       指定配置文件
  --log-level <LEVEL>   日志级别（error/warn/info/debug/trace）
  --quiet               静默输出
  --version             打印版本
  --help                打印帮助
```

---

## 顶层命令

| 命令     | 用途                        |
|----------|----------------------------|
| (无)     | 交互式 TUI（未来）          |
| serve    | 启动 HTTP API 服务器        |
| init     | 初始化项目配置              |
| chat     | 单 agent 聊天会话           |
| team     | 团队管理与协作              |
| mcp      | MCP 服务器管理              |
| config   | 配置管理                    |
| auth     | 认证管理                    |

---

## `aion serve`

启动 HTTP 后端服务器。

```
aion serve [OPTIONS]

OPTIONS:
  --host <ADDR>         监听地址（默认: 127.0.0.1）
  --port <PORT>         监听端口（默认: 3456）
  --local               本地模式，跳过认证
  --app-version <VER>   覆盖应用版本号
  --log-dir <PATH>      日志文件目录（默认: {data-dir}/logs）
```

---

## `aion init`

在当前项目目录初始化 `.aion/` 配置。

```
aion init [OPTIONS]

OPTIONS:
  --agent <TYPE>        设置项目默认 agent
  --model <MODEL>       设置项目默认模型
  --no-mcp             跳过 MCP 配置步骤
```

无参数时进入交互引导流程。

---

## `aion chat`

单 agent 聊天会话管理。

```
aion chat [OPTIONS] [MESSAGE]
aion chat <SUBCOMMAND>

ARGS:
  [MESSAGE]             初始消息（省略则进入交互模式）

OPTIONS:
  --agent <TYPE>        Agent 后端（claude/codex/gemini/aionrs/codebuddy）
  --model <MODEL>       模型覆盖
  --resume <ID>         恢复已有对话
  --print               非交互模式：输出回复后退出

SUBCOMMANDS:
  list      列出历史对话
  delete    删除指定对话
```

### `aion chat list`

```
aion chat list [OPTIONS]

OPTIONS:
  --format <FMT>        输出格式（table/json，默认: table）
  --limit <N>           最多显示条数（默认: 20）
```

### `aion chat delete`

```
aion chat delete <ID> [OPTIONS]

OPTIONS:
  --yes                 跳过确认提示
```

---

## `aion team`

团队生命周期与协作。

```
aion team <SUBCOMMAND>

SUBCOMMANDS:
  create    创建团队
  list      列出所有团队
  show      查看团队详情
  status    查看团队运行状态（成员 Working/Idle/Error）
  delete    删除团队
  rename    重命名团队
  chat      进入团队聊天会话

  agent     管理团队成员（嵌套子命令）
```

### `aion team create`

```
aion team create [OPTIONS] <NAME>

ARGS:
  <NAME>                团队名称

OPTIONS:
  --agent <SPEC>...     添加初始成员（格式: "name:type:model"）
```

### `aion team list`

```
aion team list [OPTIONS]

OPTIONS:
  --format <FMT>        输出格式（table/json，默认: table）
```

### `aion team show`

```
aion team show <ID>

ARGS:
  <ID>                  团队 ID
```

### `aion team status`

```
aion team status <ID> [OPTIONS]

ARGS:
  <ID>                  团队 ID

OPTIONS:
  --format <FMT>        输出格式（table/json，默认: table）
  --watch               持续监听状态变化
```

### `aion team delete`

```
aion team delete <ID> [OPTIONS]

OPTIONS:
  --yes                 跳过确认提示
```

### `aion team rename`

```
aion team rename <ID> <NAME>
```

### `aion team chat`

```
aion team chat <ID> [MESSAGE]

ARGS:
  <ID>                  团队 ID
  [MESSAGE]             初始消息（省略则进入交互模式）
```

### `aion team agent`

```
aion team agent <SUBCOMMAND>

SUBCOMMANDS:
  add       添加成员到团队
  remove    移除团队成员
  rename    重命名团队成员
  list      列出团队成员
```

#### `aion team agent add`

```
aion team agent add <TEAM_ID> [OPTIONS]

OPTIONS:
  --name <NAME>         成员显示名
  --type <TYPE>         Agent 后端类型
  --model <MODEL>       使用的模型
  --role <ROLE>         角色（leader/member，默认: member）
```

#### `aion team agent remove`

```
aion team agent remove <TEAM_ID> <SLOT_ID>
```

#### `aion team agent rename`

```
aion team agent rename <TEAM_ID> <SLOT_ID> <NAME>
```

#### `aion team agent list`

```
aion team agent list <TEAM_ID> [OPTIONS]

OPTIONS:
  --format <FMT>        输出格式（table/json，默认: table）
```

---

## `aion mcp`

管理可注入到 agent 会话的 MCP 服务器。

```
aion mcp <SUBCOMMAND>

SUBCOMMANDS:
  add       注册 MCP 服务器
  remove    移除 MCP 服务器
  list      列出已配置的 MCP 服务器
  test      测试 MCP 服务器连通性
```

### `aion mcp add`

```
aion mcp add <NAME> [OPTIONS]

ARGS:
  <NAME>                服务器标识名

OPTIONS:
  --command <CMD>       Stdio 传输：执行的命令
  --args <ARGS>...      Stdio 传输：命令参数
  --url <URL>           HTTP/SSE 传输：服务器地址
  --transport <TYPE>    传输类型（stdio/sse/http，默认: 自动检测）
  --env <K=V>...        服务器进程环境变量
  --scope <SCOPE>       作用域（global/project，默认: project）
```

### `aion mcp remove`

```
aion mcp remove <NAME> [OPTIONS]

OPTIONS:
  --scope <SCOPE>       作用域（global/project，默认: project）
```

### `aion mcp list`

```
aion mcp list [OPTIONS]

OPTIONS:
  --scope <SCOPE>       按作用域过滤（global/project/all，默认: all）
  --format <FMT>        输出格式（table/json，默认: table）
```

### `aion mcp test`

```
aion mcp test <NAME>
```

---

## `aion config`

管理 Aion 配置。

```
aion config <SUBCOMMAND>

SUBCOMMANDS:
  set       设置配置项
  get       获取配置项
  list      列出所有配置项
  reset     重置为默认配置
  path      打印配置文件路径
  edit      用 $EDITOR 打开配置文件
```

### `aion config set`

```
aion config set <KEY> <VALUE> [OPTIONS]

OPTIONS:
  --scope <SCOPE>       作用域（global/project/local，默认: project）
```

### `aion config get`

```
aion config get <KEY>
```

### `aion config list`

```
aion config list [OPTIONS]

OPTIONS:
  --scope <SCOPE>       按作用域过滤（global/project/local/all，默认: all）
  --format <FMT>        输出格式（table/json，默认: table）
```

### `aion config edit`

```
aion config edit [OPTIONS]

OPTIONS:
  --scope <SCOPE>       作用域（global/project/local，默认: project）
```

---

## `aion auth`

认证管理。

```
aion auth <SUBCOMMAND>

SUBCOMMANDS:
  login       Aion 平台登录
  logout      清除 Aion 平台令牌
  status      查看当前认证状态
  token       打印当前访问令牌（用于脚本）
  set-key     设置 provider API Key
  remove-key  移除 provider API Key
  list-keys   列出已配置的 provider
```

### `aion auth login`

```
aion auth login [OPTIONS]

OPTIONS:
  --provider <P>        认证提供商（如支持多种）
```

### `aion auth set-key`

```
aion auth set-key <PROVIDER> <API_KEY>

ARGS:
  <PROVIDER>            提供商名称（claude/openai/gemini/custom）
  <API_KEY>             API Key 值

EXAMPLES:
  aion auth set-key claude sk-ant-xxx
  aion auth set-key openai sk-xxx
```

### `aion auth remove-key`

```
aion auth remove-key <PROVIDER>
```

### `aion auth list-keys`

```
aion auth list-keys

输出示例:
  PROVIDER    STATUS
  claude      ✓ configured (sk-ant-...xxx)
  openai      ✓ configured (sk-...xxx)
  gemini      ✗ not configured
```

---

## 输入约定

- **文本消息**：直接输入
- **文件附加**：粘贴文件绝对路径，作为普通字符串传给 agent
- **图片粘贴**：终端粘贴图片显示为 `[Image #N]`，作为图片附件发送

---

## 设计原则

1. **前端客户端定位**：CLI 不内嵌 agent 逻辑，通过 HTTP API 调后端
2. **能力门控**：MCP 服务器仅注入到支持 MCP 的 agent（复用 `has_mcp_capability()` 检查）
3. **配置分层**：global → project → local，就近优先
4. **输出模式**：默认人类可读，`--format json` 用于脚本对接
5. **非破坏性默认**：`delete` 类命令默认提示确认，除非传入 `--yes`
6. **增量交付**：`serve` 立即可用（现有行为）；其他命令逐步添加
