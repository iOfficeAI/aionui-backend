# 04 - 系统设置

## 概述

管理系统偏好设置和 AI 模型提供商配置。包括通用键值设置存取、模型列表获取与协议自动检测。

**源码位置**：`process/bridge/systemSettingsBridge.ts`、`process/bridge/modelBridge.ts`、`common/config/`

## 架构设计

### 功能分区

```
系统设置
├── 偏好设置          → 键值存取（通知、语言、上传位置、命令队列等）
└── 模型提供商管理    → 模型列表获取、配置 CRUD、协议检测
```

### 设置分类

| 分类 | 设置项 | 存储键 | 默认值 | 迁移策略 |
|------|--------|--------|--------|----------|
| 后端设置 | 任务完成通知 | `system.notificationEnabled` | `true` | 迁移：后端可据此决定是否推送通知 |
| 后端设置 | 定时任务通知 | `system.cronNotificationEnabled` | `false` | 迁移：同上 |
| 后端设置 | 命令队列 | `system.commandQueueEnabled` | `false` | 迁移：影响会话命令处理逻辑 |
| 后端设置 | 上传保存到工作区 | `upload.saveToWorkspace` | `false` | 迁移：影响文件上传路径 |
| 后端设置 | 语言 | `language` | `en-US` | 迁移：存储用户语言偏好，前端据此加载翻译 |
| 客户端设置 | 最小化到托盘 | `system.closeToTray` | `false` | 不迁移：Electron 窗口行为 |
| 客户端设置 | 防止息屏 | `system.keepAwake` | `false` | 不迁移：Electron 电源管理 |
| 客户端设置 | 桌面宠物相关 | `pet.*` | 见下文 | 不迁移：Electron 窗口功能 |

> **设计决策**：原实现中所有设置都通过 IPC 在同一个 bridge 中管理。Rust 重写时拆分为两层：后端自身需要的设置通过 REST API 管理并持久化到数据库；纯客户端偏好通过通用键值存储 API 透传，后端不解读其语义。

## REST API

### GET /api/settings

获取所有系统设置。

**需要认证**：是

**成功响应** `200`：

```json
{
  "success": true,
  "data": {
    "language": "zh-CN",
    "notificationEnabled": true,
    "cronNotificationEnabled": false,
    "commandQueueEnabled": false,
    "saveUploadToWorkspace": false
  }
}
```

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 403 | 未认证 |
| 500 | 服务器内部错误 |

---

### PATCH /api/settings

更新系统设置（部分更新）。

**需要认证**：是

**请求体**：

```json
{
  "language": "zh-CN",
  "notificationEnabled": false
}
```

只需包含要修改的字段。允许的字段：

| 字段 | 类型 | 校验规则 |
|------|------|---------|
| `language` | `string` | 必须为支持的语言代码 |
| `notificationEnabled` | `boolean` | — |
| `cronNotificationEnabled` | `boolean` | — |
| `commandQueueEnabled` | `boolean` | — |
| `saveUploadToWorkspace` | `boolean` | — |

**成功响应** `200`：

```json
{
  "success": true,
  "data": { /* 更新后的完整设置 */ }
}
```

**副作用**：
- 修改 `language` 时，通过 WebSocket 广播 `languageChanged` 事件，通知所有已连接客户端同步

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 400 | 字段校验失败（不支持的语言、类型错误等） |
| 403 | 未认证 |
| 500 | 服务器内部错误 |

---

### GET /api/settings/client

获取客户端偏好设置（通用键值存储）。

**需要认证**：是

**查询参数**：

| 参数 | 类型 | 说明 |
|------|------|------|
| `keys` | `string` | 可选，逗号分隔的键名列表。不传则返回所有客户端设置 |

**成功响应** `200`：

```json
{
  "success": true,
  "data": {
    "system.closeToTray": false,
    "system.keepAwake": false,
    "pet.enabled": false,
    "pet.size": 280,
    "pet.dnd": false,
    "pet.confirmEnabled": true,
    "theme": "dark",
    "ui.zoomFactor": 1.0
  }
}
```

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 403 | 未认证 |
| 500 | 服务器内部错误 |

---

### PUT /api/settings/client

批量更新客户端偏好设置。

**需要认证**：是

**请求体**：

```json
{
  "system.closeToTray": true,
  "pet.size": 360
}
```

值类型为 `string | number | boolean | null`（`null` 表示删除该键）。

**成功响应** `200`：

```json
{
  "success": true
}
```

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 400 | 键名不合法（空字符串、超长等） |
| 403 | 未认证 |
| 500 | 服务器内部错误 |

---

### GET /api/providers

获取所有已配置的模型提供商。

**需要认证**：是

**成功响应** `200`：

```json
{
  "success": true,
  "data": [
    {
      "id": "uuid-xxx",
      "platform": "anthropic",
      "name": "Anthropic",
      "baseUrl": "https://api.anthropic.com",
      "apiKey": "sk-ant-***",
      "models": ["claude-sonnet-4-20250514", "claude-opus-4-20250514"],
      "enabled": true,
      "capabilities": [
        { "type": "text" },
        { "type": "vision" },
        { "type": "function_calling" }
      ],
      "modelEnabled": {
        "claude-sonnet-4-20250514": true,
        "claude-opus-4-20250514": true
      },
      "modelHealth": {
        "claude-sonnet-4-20250514": {
          "status": "healthy",
          "lastCheck": 1712345678000,
          "latency": 320
        }
      }
    }
  ]
}
```

> **设计决策**：`apiKey` 在响应中应脱敏显示（仅返回前缀和最后 4 位），完整 API Key 仅在创建/更新时接收。原实现直接暴露完整 Key，属于安全缺陷。

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 403 | 未认证 |
| 500 | 服务器内部错误 |

---

### POST /api/providers

创建模型提供商。

**需要认证**：是

**请求体**：

```json
{
  "platform": "anthropic",
  "name": "Anthropic",
  "baseUrl": "https://api.anthropic.com",
  "apiKey": "sk-ant-api03-...",
  "models": [],
  "enabled": true,
  "bedrockConfig": null
}
```

**请求字段**：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `platform` | `string` | 是 | 平台标识 |
| `name` | `string` | 是 | 显示名称 |
| `baseUrl` | `string` | 是 | API 基础 URL |
| `apiKey` | `string` | 是 | API 密钥（支持逗号/换行分隔的多密钥） |
| `models` | `string[]` | 否 | 模型 ID 列表 |
| `enabled` | `boolean` | 否 | 是否启用，默认 `true` |
| `capabilities` | `ModelCapability[]` | 否 | 模型能力列表 |
| `contextLimit` | `number` | 否 | 上下文 Token 限制 |
| `bedrockConfig` | `BedrockConfig` | 否 | AWS Bedrock 专属配置 |

**成功响应** `201`：

```json
{
  "success": true,
  "data": { /* 创建后的完整提供商对象（含生成的 id） */ }
}
```

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 400 | 必填字段缺失、URL 格式错误 |
| 403 | 未认证 |
| 500 | 服务器内部错误 |

---

### PUT /api/providers/:id

更新模型提供商。

**需要认证**：是

**请求体**：同 POST，所有字段可选（部分更新）。

**成功响应** `200`：

```json
{
  "success": true,
  "data": { /* 更新后的完整提供商对象 */ }
}
```

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 400 | 字段校验失败 |
| 403 | 未认证 |
| 404 | 提供商不存在 |
| 500 | 服务器内部错误 |

---

### DELETE /api/providers/:id

删除模型提供商。

**需要认证**：是

**成功响应** `200`：

```json
{
  "success": true
}
```

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 403 | 未认证 |
| 404 | 提供商不存在 |
| 500 | 服务器内部错误 |

---

### POST /api/providers/:id/models

获取指定提供商的可用模型列表（从远程 API 拉取）。

**需要认证**：是

**请求体**：

```json
{
  "tryFix": true
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `tryFix` | `boolean` | 是否尝试自动修正 URL（默认 `false`） |

**成功响应** `200`：

```json
{
  "success": true,
  "data": {
    "models": [
      "claude-sonnet-4-20250514",
      "claude-opus-4-20250514",
      "claude-3-7-sonnet-20250219"
    ],
    "fixedBaseUrl": "https://api.anthropic.com/v1"
  }
}
```

`models` 可能是字符串数组或 `{ id, name }` 对象数组。`fixedBaseUrl` 仅在 `tryFix=true` 且 URL 被修正时返回。

**各平台获取逻辑**：

| 平台 | 端点 | 说明 |
|------|------|------|
| `anthropic` / `claude` | `GET {baseUrl}/v1/models` | Header: `x-api-key`，失败时回退到默认模型列表 |
| `gemini` | `GET {baseUrl}/v1beta/models?key={apiKey}` | 去除 `models/` 前缀，失败时回退默认列表 |
| `bedrock` | AWS `ListInferenceProfiles` | 过滤 `anthropic.claude` 模型，按 region |
| `vertex-ai` | 硬编码 | 返回 `['gemini-2.5-pro', 'gemini-2.5-flash']` |
| `new-api` | `GET {baseUrl}/v1/models` | OpenAI SDK，确保 URL 含 `/v1` |
| `minimax` | 硬编码 | MiniMax 无 models 端点 |
| `dashscope-coding` | 硬编码 + 校验 | 返回预设模型列表，通过 `/chat/completions` 验证 Key |
| OpenAI 兼容（默认） | `GET {baseUrl}/models` | OpenAI SDK，支持 URL 自动修正 |

**URL 自动修正**（`tryFix=true`）：

对于 OpenAI 兼容平台，当原始 URL 请求失败时，尝试以下路径变体并行探测：
- 用户提供路径的变体
- `/v1`、`/api/v1`、`/openai/v1`、`/compatible-mode/v1`
- `/v2`、`/api/v3`、`/api/paas/v4`、`/compatibility/v1`

使用 `Promise.any` 并行请求，返回首个成功的结果及修正后的 URL。

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 400 | 缺少 API Key |
| 403 | 未认证 |
| 404 | 提供商不存在 |
| 502 | 远程 API 请求失败（连接超时、认证失败等） |
| 500 | 服务器内部错误 |

---

### POST /api/providers/detect-protocol

检测 API 端点的协议类型。

**需要认证**：是

**请求体**：

```json
{
  "baseUrl": "https://api.example.com",
  "apiKey": "sk-xxx",
  "timeout": 10000,
  "testAllKeys": false,
  "preferredProtocol": "openai"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `baseUrl` | `string` | 是 | API 基础 URL |
| `apiKey` | `string` | 是 | API 密钥（支持多密钥） |
| `timeout` | `number` | 否 | 请求超时（ms），默认 10000 |
| `testAllKeys` | `boolean` | 否 | 是否测试所有密钥，默认 `false` |
| `preferredProtocol` | `ProtocolType` | 否 | 优先测试的协议 |

**成功响应** `200`：

```json
{
  "success": true,
  "data": {
    "protocol": "anthropic",
    "confidence": 95,
    "fixedBaseUrl": null,
    "models": ["claude-sonnet-4-20250514"],
    "suggestion": {
      "type": "none",
      "message": "Detected Anthropic protocol",
      "i18nKey": "settings.protocolDetected"
    },
    "multiKeyResult": null
  }
}
```

**检测流程**：

1. **URL 推断**：从 URL 格式猜测协议（如包含 `anthropic` → anthropic 协议）
2. **密钥推断**：从 API Key 格式猜测协议（如 `sk-ant-` → anthropic）
3. **构建测试优先级**：优先 > URL 推断 > Key 推断 > 默认顺序
4. **逐协议测试**：每个协议尝试多个 URL 变体，首个成功即返回
5. **多密钥测试**（可选）：并发测试所有密钥（最大并发 5），返回每个密钥的有效性

**协议类型**：`openai` | `anthropic` | `gemini` | `unknown`

**置信度**：0-95，表示检测结果的可信程度

**建议类型**：

| type | 触发条件 |
|------|---------|
| `none` | 检测成功，无需额外操作 |
| `check_key` | 认证失败，建议检查 API Key |
| `switch_platform` | 检测到的协议与用户选择的平台不匹配 |

**多密钥测试结果**：

```json
{
  "total": 3,
  "valid": 2,
  "invalid": 1,
  "details": [
    { "index": 0, "maskedKey": "sk-***abcd", "valid": true, "latency": 320 },
    { "index": 1, "maskedKey": "sk-***efgh", "valid": true, "latency": 280 },
    { "index": 2, "maskedKey": "sk-***ijkl", "valid": false, "error": "Invalid API key", "latency": 150 }
  ]
}
```

**错误响应**：

| 状态码 | 场景 |
|--------|------|
| 400 | 缺少 baseUrl 或 apiKey |
| 403 | 未认证 |
| 500 | 服务器内部错误 |

## IPC 接口（Electron → 后端）

### systemSettings.getCloseToTray / setCloseToTray

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getCloseToTray` / `setCloseToTray` |
| 目标协议 | 客户端键值存储 API（`PUT /api/settings/client`） |
| get 返回 | `boolean`（默认 `false`） |
| set 参数 | `{ enabled: boolean }` |

最小化到托盘开关。设置后通知主进程更新窗口关闭行为。

> **不迁移到后端逻辑**：此为 Electron 窗口行为，Rust 后端仅透传存储。

---

### systemSettings.getKeepAwake / setKeepAwake

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getKeepAwake` / `setKeepAwake` |
| 目标协议 | 客户端键值存储 API（`PUT /api/settings/client`） |
| get 返回 | `boolean`（默认 `false`） |
| set 参数 | `{ enabled: boolean }` |

防止息屏开关。启用时调用 `platformServices.power.preventDisplaySleep()`，禁用时释放。

> **不迁移到后端逻辑**：此为 OS 电源管理 API，Rust 后端仅透传存储。

---

### systemSettings.getNotificationEnabled / setNotificationEnabled

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getNotificationEnabled` / `setNotificationEnabled` |
| 目标协议 | HTTP（`PATCH /api/settings`） |
| get 返回 | `boolean`（默认 `true`） |
| set 参数 | `{ enabled: boolean }` |

任务完成通知开关。后端据此决定是否通过 WebSocket 推送任务完成通知。

---

### systemSettings.getCronNotificationEnabled / setCronNotificationEnabled

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getCronNotificationEnabled` / `setCronNotificationEnabled` |
| 目标协议 | HTTP（`PATCH /api/settings`） |
| get 返回 | `boolean`（默认 `false`） |
| set 参数 | `{ enabled: boolean }` |

定时任务通知开关。

---

### systemSettings.changeLanguage

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.changeLanguage` |
| 目标协议 | HTTP（`PATCH /api/settings`） |
| 参数 | `{ language: string }` |

切换系统语言。后端存储语言偏好，通过 WebSocket 广播 `languageChanged` 事件通知所有客户端同步。翻译由前端自行处理。

---

### systemSettings.getSaveUploadToWorkspace / setSaveUploadToWorkspace

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getSaveUploadToWorkspace` / `setSaveUploadToWorkspace` |
| 目标协议 | HTTP（`PATCH /api/settings`） |
| get 返回 | `boolean`（默认 `false`） |
| set 参数 | `{ enabled: boolean }` |

上传文件保存到工作区开关。影响文件上传时的存储路径选择。

---

### systemSettings.getCommandQueueEnabled / setCommandQueueEnabled

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getCommandQueueEnabled` / `setCommandQueueEnabled` |
| 目标协议 | HTTP（`PATCH /api/settings`） |
| get 返回 | `boolean`（默认 `false`） |
| set 参数 | `{ enabled: boolean }` |

会话命令队列开关。启用后，用户在 AI 回复过程中发送的消息会排入队列而非丢弃。

---

### systemSettings.getPetEnabled / setPetEnabled

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getPetEnabled` / `setPetEnabled` |
| 目标协议 | 客户端键值存储 API |
| get 返回 | `boolean`（默认 `false`） |
| set 参数 | `{ enabled: boolean }` |

桌面宠物开关。set 时检查 `isPetSupported()`，然后创建/销毁宠物窗口。

> **不迁移到后端逻辑**：Electron 窗口功能。另有专门的 `15-pet.md` 模块。

---

### systemSettings.getPetSize / setPetSize

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getPetSize` / `setPetSize` |
| 目标协议 | 客户端键值存储 API |
| get 返回 | `number`（默认 `280`，可选值：200、280、360） |
| set 参数 | `{ size: number }` |

---

### systemSettings.getPetDnd / setPetDnd

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getPetDnd` / `setPetDnd` |
| 目标协议 | 客户端键值存储 API |
| get 返回 | `boolean`（默认 `false`） |
| set 参数 | `{ dnd: boolean }` |

宠物免打扰模式。启用后宠物保持静止，不响应 AI 事件。

---

### systemSettings.getPetConfirmEnabled / setPetConfirmEnabled

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.systemSettings.getPetConfirmEnabled` / `setPetConfirmEnabled` |
| 目标协议 | 客户端键值存储 API |
| get 返回 | `boolean`（默认 `true`） |
| set 参数 | `{ enabled: boolean }` |

AI 工具调用确认是否路由到宠物气泡窗口。

---

### mode.fetchModelList

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.mode.fetchModelList` |
| 目标协议 | HTTP（`POST /api/providers/:id/models`） |
| 参数 | `{ base_url, api_key, try_fix, platform, bedrockConfig? }` |
| 返回 | `{ success, msg?, data?: { mode: Array<string \| { id, name }>, fix_base_url? } }` |

从远程 API 拉取可用模型列表。详见 REST API 中 `POST /api/providers/:id/models` 的说明。

---

### mode.saveModelConfig

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.mode.saveModelConfig` |
| 目标协议 | HTTP（`POST /api/providers` + `PUT /api/providers/:id`） |
| 参数 | `IProvider[]`（完整的提供商配置数组） |
| 返回 | `{ success, msg? }` |

保存整个模型提供商配置。

> **设计决策**：原实现一次性保存整个配置数组。Rust 重写时改为 RESTful 的单个提供商 CRUD 操作，避免并发写冲突。

---

### mode.getModelConfig

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.mode.getModelConfig` |
| 目标协议 | HTTP（`GET /api/providers`） |
| 返回 | `IProvider[]` |

获取模型提供商配置。原实现包含：
1. 从旧格式（`selectedModel`）迁移到新格式（`useModel`）
2. 补全缺失的 `id` 和 `capabilities` 字段
3. 合并扩展系统注入的提供商（用户覆盖优先）

---

### mode.detectProtocol

| 属性 | 值 |
|------|-----|
| 通道 | `ipcBridge.mode.detectProtocol` |
| 目标协议 | HTTP（`POST /api/providers/detect-protocol`） |
| 参数 | `ProtocolDetectionRequest` |
| 返回 | `{ success, msg?, data?: ProtocolDetectionResponse }` |

检测 API 协议类型。详见 REST API 中 `POST /api/providers/detect-protocol` 的说明。

## 数据模型

### SystemSettings

系统设置的核心字段：

```
SystemSettings {
  language: string                  // BCP 47 语言代码，默认 "en-US"
  notification_enabled: boolean     // 任务完成通知，默认 true
  cron_notification_enabled: boolean // 定时任务通知，默认 false
  command_queue_enabled: boolean    // 命令队列，默认 false
  save_upload_to_workspace: boolean // 上传保存到工作区，默认 false
}
```

### ClientPreference

客户端偏好的通用键值对：

```
ClientPreference {
  key: string           // 键名（如 "system.closeToTray"）
  value: string         // JSON 序列化的值
  updated_at: number    // 最后更新时间
}
```

### IProvider

模型提供商配置：

```
IProvider {
  id: string                        // UUID
  platform: string                  // 平台标识
  name: string                      // 显示名称
  base_url: string                  // API 基础 URL
  api_key: string                   // API 密钥（加密存储）
  models: string[]                  // 可用模型 ID 列表
  enabled: boolean                  // 是否启用
  capabilities: ModelCapability[]   // 模型能力列表
  context_limit: number | null      // 上下文 Token 限制
  model_protocols: Map<string, string> | null  // 每模型协议覆盖
  model_enabled: Map<string, boolean> | null   // 每模型启用状态
  model_health: Map<string, ModelHealthStatus> | null  // 每模型健康状态
  bedrock_config: BedrockConfig | null  // AWS Bedrock 专属配置
  created_at: number
  updated_at: number
}
```

### ModelCapability

```
ModelCapability {
  type: ModelType          // 能力类型
  is_user_selected: boolean | null  // 是否用户手动标记
}
```

**ModelType 枚举**：`text` | `vision` | `function_calling` | `image_generation` | `web_search` | `reasoning` | `embedding` | `rerank` | `excludeFromPrimary`

### ModelHealthStatus

```
ModelHealthStatus {
  status: "unknown" | "healthy" | "unhealthy"
  last_check: number | null     // 最后检查时间
  latency: number | null        // 响应延迟 (ms)
  error: string | null          // 错误信息
}
```

### BedrockConfig

```
BedrockConfig {
  auth_method: "accessKey" | "profile"
  region: string
  access_key_id: string | null
  secret_access_key: string | null   // 加密存储
  profile: string | null
}
```

### ProtocolType

```
ProtocolType = "openai" | "anthropic" | "gemini" | "unknown"
```

## 模块依赖

- **依赖**：
  - `02-database`：设置持久化（系统设置表、客户端偏好表、提供商配置表）
  - `03-auth`：API 认证中间件
  - `07-realtime`：语言变更广播（WebSocket 事件）
  - `13-extension`：扩展系统注入的模型提供商合并

- **被依赖**：
  - `05-conversation`：获取模型提供商配置用于发起 AI 对话
  - `06-ai-agent`：使用提供商配置连接 AI 后端
  - `09-channel`：各通道的默认模型配置
  - `11-cron`：定时任务通知设置
  - `14-app-lifecycle`：系统启动时的初始化

## 候选公共类型

| 类型 | 来源 | 说明 |
|------|------|------|
| `IProvider` | model config | 模型提供商配置，多个模块共用 |
| `ModelCapability` / `ModelType` | model config | 模型能力描述 |
| `ProtocolType` | protocol detection | 协议类型枚举 |
| `BedrockConfig` | model config | AWS Bedrock 配置（若多处使用） |

## 常量

### 文件处理

| 常量 | 值 | 说明 |
|------|-----|------|
| `AIONUI_TIMESTAMP_SEPARATOR` | `_aionui_` | 文件名中的时间戳分隔符 |
| `AIONUI_FILES_MARKER` | `[[AION_FILES]]` | 消息中的文件占位标记 |

### 图片格式

支持的图片扩展名：`.jpg`、`.jpeg`、`.png`、`.gif`、`.webp`、`.bmp`、`.tiff`、`.svg`

MIME 类型双向映射（扩展名 ↔ MIME 类型）。

### WebUI 端口

| 环境 | 默认端口 |
|------|---------|
| 生产 | 25808 |
| 开发（单实例） | 25809 |
| 开发（多实例） | 25810 |

### 功能标志

| 标志 | 值 | 说明 |
|------|-----|------|
| `TEAM_MODE_ENABLED` | `true` | 团队模式总开关 |
| `GOOGLE_AUTH_PROVIDER_ID` | `google-auth-gemini` | Google Auth 提供商 ID |

## Rust 迁移备注

1. **设置存储**：使用数据库表代替 `ProcessConfig`（Electron store）。后端设置和客户端偏好分两张表
2. **API Key 安全**：API Key 使用加密存储（如 `aes-gcm` crate），读取时脱敏；原实现明文存储在本地文件中
3. **协议检测**：使用 `reqwest` 进行 HTTP 探测，`tokio::select!` 实现超时控制和并行竞赛
4. **模型列表获取**：各平台的 API 调用可抽象为 `trait ModelFetcher`，每个平台一个实现
5. **AWS Bedrock**：使用 `aws-sdk-bedrock` crate，注意 credential provider 的异步初始化
6. **语言设置**：后端仅存储用户语言偏好并通过 WebSocket 广播变更。翻译完全由前端处理，后端不需要 i18n 框架
7. **配置迁移**：首次启动时从旧 Electron store 导入配置（如有）。迁移逻辑（`selectedModel` → `useModel`）写入迁移脚本
8. **扩展提供商合并**：在 `GET /api/providers` 的 service 层处理，合并扩展注入的提供商，用户配置优先
9. **URL 自动修正**：使用 `futures::select_ok` 或 `tokio::select!` 实现多 URL 并行探测
