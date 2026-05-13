# Aion CLI 指令系统设计

## 概述

交互会话内通过 `/` 前缀触发指令。指令分两类：

1. **平台指令** — Aion 内置，所有会话通用
2. **Agent 指令** — 从当前 agent 动态获取（类似 agent 注册的 skills）

---

## 平台指令（内置）

| 指令                | 用途                                      |
|--------------------|------------------------------------------|
| `/agent <type>`    | 切换当前 agent 后端（claude/codex/gemini/aionrs/codebuddy） |
| `/model <name>`    | 切换当前模型                              |
| `/new`             | 开始新对话（保留当前 agent 设置）          |
| `/history`         | 列出历史对话                              |
| `/resume <id>`     | 恢复历史对话                              |
| `/delete-chat <id>` | 删除指定对话                             |
| `/team`            | 列出可用团队 / 进入团队会话                |
| `/mcp`             | 列出当前会话已加载的 MCP 服务器            |
| `/config`          | 查看/修改当前会话配置                      |
| `/clear`           | 清屏                                      |
| `/help`            | 显示可用指令列表                           |
| `/exit`            | 退出交互会话                              |

### `/agent` 详细

```
/agent              列出可用 agent 后端及当前选中
/agent claude       切换到 claude
/agent aionrs       切换到 aionrs
```

切换 agent 时：
- 当前对话结束，自动保存
- 新 agent 开始新对话上下文
- MCP 根据新 agent 的能力重新注入（capability gating）

### `/model` 详细

```
/model              列出当前 agent 可用模型及当前选中
/model opus         切换模型
```

---

## Agent 指令（动态）

Agent 自身可以注册指令，在会话建立时动态加载。格式统一为 `/` 前缀，通过 agent 的 skills/tools 机制暴露。

### 加载机制

1. 会话启动时，向 agent 查询其注册的指令列表
2. 返回格式：`{ name, description, parameters? }`
3. 平台指令优先级高于 agent 指令（名称冲突时平台指令生效）

### 展示

```
/help

平台指令:
  /agent       切换 agent 后端
  /model       切换模型
  /new         新对话
  ...

Agent 指令 (claude):
  /compact     压缩上下文
  /think       切换扩展思考
  ...
```

---

## 指令解析规则

1. 输入以 `/` 开头 → 进入指令解析
2. 匹配平台指令 → 执行
3. 匹配 agent 指令 → 转发给 agent 执行
4. 无匹配 → 提示"未知指令，输入 /help 查看可用指令"
5. `/` 后紧跟空格或无内容 → 弹出指令补全列表（TUI 模式）

---

## 指令补全（TUI 模式）

输入 `/` 后触发自动补全面板：
- 显示所有可用指令（平台 + agent）
- 支持模糊搜索过滤
- 上下键选择，Tab/Enter 确认
- 显示指令描述作为提示

---

## 设计原则

1. **平台优先**：平台指令不可被 agent 覆盖
2. **动态发现**：agent 指令按需加载，不硬编码
3. **统一前缀**：所有指令统一 `/` 前缀，不区分来源
4. **无状态**：指令执行不依赖前序指令状态
5. **可扩展**：新增平台指令只需注册，不影响 agent 指令加载逻辑
