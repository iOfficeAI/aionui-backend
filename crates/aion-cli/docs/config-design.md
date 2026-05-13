# Aion CLI 配置系统设计

## 概述

三层配置体系：全局 → 项目 → 会话运行时。逐层覆盖，就近优先。

---

## 目录结构

```
~/.aion/                            全局配置根目录
├── config.json                     全局设置
├── mcp.json                        全局 MCP 服务器
├── credentials.json                认证凭证（API Keys 等）
├── history/                        对话历史存储
│   └── {conversation_id}.json
└── data/                           运行时数据（数据库、日志等）
    ├── aion.db                     SQLite 数据库
    └── logs/                       日志文件

<project>/.aion/                    项目级配置（提交到版本控制）
├── config.json                     项目设置（覆盖全局）
├── mcp.json                        项目级 MCP 服务器
└── settings.local.json             本地覆盖（.gitignore）
```

---

## 文件详细定义

### `~/.aion/credentials.json`

存储 API Key 和认证令牌。**不可提交到版本控制。**

```json
{
  "providers": {
    "claude": {
      "api_key": "sk-ant-..."
    },
    "openai": {
      "api_key": "sk-..."
    },
    "gemini": {
      "api_key": "AIza..."
    },
    "custom": {
      "my-endpoint": {
        "api_key": "...",
        "base_url": "https://my-proxy.com/v1"
      }
    }
  },
  "aion": {
    "access_token": "...",
    "refresh_token": "..."
  }
}
```

**管理方式：**

```bash
aion auth set-key claude sk-ant-xxx        # 设置 provider API Key
aion auth set-key openai sk-xxx            # 设置 OpenAI key
aion auth set-key gemini AIzaxxx           # 设置 Gemini key
aion auth remove-key claude                # 移除 key
aion auth list-keys                        # 列出已配置的 provider（不显示 key 明文）
aion auth login                            # Aion 平台登录（获取 access_token）
aion auth logout                           # 清除 Aion 令牌
aion auth status                           # 显示当前认证状态
```

---

### `~/.aion/config.json`

全局默认设置。

```json
{
  "default_agent": "claude",
  "default_model": {
    "claude": "opus",
    "openai": "gpt-4o",
    "gemini": "gemini-2.5-pro",
    "aionrs": "default"
  },
  "theme": "dark",
  "locale": "zh-CN",
  "output": {
    "format": "pretty",
    "color": true,
    "stream": true
  },
  "chat": {
    "auto_save": true,
    "max_history": 100
  },
  "team": {
    "default_mode": "coordinate"
  }
}
```

---

### `<project>/.aion/config.json`

项目级覆盖，仅写需要覆盖的字段。

```json
{
  "default_agent": "aionrs",
  "default_model": {
    "aionrs": "deepseek-v3"
  }
}
```

---

### `<project>/.aion/settings.local.json`

本地覆盖，加入 `.gitignore`。用于个人偏好不提交到仓库的场景。

```json
{
  "default_agent": "claude",
  "output": {
    "stream": false
  }
}
```

---

### `~/.aion/mcp.json` / `<project>/.aion/mcp.json`

MCP 服务器配置。格式参考 Claude 的 `.mcp.json`。

```json
{
  "servers": {
    "memory": {
      "transport": "stdio",
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-memory"],
      "env": {}
    },
    "my-api": {
      "transport": "sse",
      "url": "http://localhost:8080/sse"
    }
  }
}
```

---

## 配置合并规则

优先级从低到高：

```
~/.aion/config.json          (全局默认)
  ↓ 覆盖
<project>/.aion/config.json  (项目级)
  ↓ 覆盖
<project>/.aion/settings.local.json  (本地个人)
  ↓ 覆盖
CLI 参数 / 环境变量           (运行时)
```

**合并策略：**
- 对象字段：深度合并，高优先级覆盖低优先级
- 数组字段：替换（不合并）
- MCP 服务器：按 name 合并，名称冲突时高优先级覆盖

---

## 环境变量

| 环境变量 | 用途 | 对应配置项 |
|---------|------|-----------|
| `AION_DATA_DIR` | 数据目录 | `--data-dir` |
| `AION_CONFIG_DIR` | 全局配置目录（默认 `~/.aion`） | — |
| `AION_DEFAULT_AGENT` | 默认 agent | `default_agent` |
| `ANTHROPIC_API_KEY` | Claude API Key（兼容已有生态） | `credentials.providers.claude.api_key` |
| `OPENAI_API_KEY` | OpenAI API Key（兼容已有生态） | `credentials.providers.openai.api_key` |
| `GEMINI_API_KEY` | Gemini API Key | `credentials.providers.gemini.api_key` |

**优先级：** 环境变量 > credentials.json（方便 CI/容器环境注入）

---

## `aion init`

在当前项目目录初始化 `.aion/` 配置。

```bash
aion init [OPTIONS]

OPTIONS:
  --agent <TYPE>        设置项目默认 agent
  --model <MODEL>       设置项目默认模型
  --no-mcp             跳过 MCP 配置步骤
```

**交互流程（无参数时）：**

```
$ aion init
? 项目默认 agent: (使用方向键)
  ❯ claude
    codex
    gemini
    aionrs

? 默认模型: opus

? 是否配置项目级 MCP 服务器? (y/N)

✓ 已创建 .aion/config.json
✓ 已添加 .aion/settings.local.json 到 .gitignore
```

---

## `aion config` 命令完善

```bash
aion config set <KEY> <VALUE> [--scope global|project|local]
aion config get <KEY>
aion config list [--scope global|project|local|all]
aion config reset [--scope project|local]
aion config path [--scope global|project]
aion config edit [--scope global|project|local]    # 用 $EDITOR 打开配置文件
```

**KEY 路径示例：**

```bash
aion config set default_agent aionrs
aion config set default_model.claude sonnet
aion config set output.stream false --scope local
aion config set chat.max_history 200
```

---

## 首次运行引导

用户首次运行 `aion`（`~/.aion/` 不存在时）：

```
$ aion
欢迎使用 Aion! 首次设置:

? 选择主要使用的 agent:
  ❯ claude
    codex
    gemini
    aionrs

? 输入 Claude API Key (可跳过，后续用 aion auth set-key 设置):
  sk-ant-... ✓

✓ 配置已保存到 ~/.aion/
提示: 运行 aion chat 开始对话，或 aion --help 查看所有命令。
```

---

## 设计原则

1. **零配置可用**：环境变量中有 API Key 即可直接 `aion chat`，不强制 init
2. **兼容已有生态**：`ANTHROPIC_API_KEY` / `OPENAI_API_KEY` 等直接识别
3. **敏感信息隔离**：credentials.json 独立存放，永远不会出现在项目配置中
4. **渐进式配置**：全局兜底，项目覆盖，本地微调，CLI 参数最终覆盖
5. **可审计**：`aion config list --scope all` 可查看最终生效配置及来源
