# 02 - 数据模型与存储

## 概述

AionUi 后端的持久化层，使用 SQLite 作为嵌入式数据库。负责所有业务数据的增删改查，包括用户、会话、消息、通道（Channel）、远程 Agent、定时任务、团队等。

**源码位置**：`process/services/database/`

## 架构设计

### 驱动抽象层

原实现通过 `ISqliteDriver` trait 抽象底层 SQLite 引擎，支持 `better-sqlite3`（Node.js）和 `bun:sqlite`（Bun）两种驱动。运行时根据环境自动选择。

**Rust 对应方案**：使用 `rusqlite` 或 `sqlx`（SQLite 模式）。无需多驱动抽象，Rust 原生编译即可。

### 数据库生命周期

| 操作 | 说明 |
|------|------|
| 创建/打开 | 异步单例模式（`getDatabase()`），首次调用时初始化 |
| Schema 初始化 | 建表 + 索引，幂等执行（`CREATE TABLE IF NOT EXISTS`） |
| 迁移 | 版本号驱动（`user_version` pragma），逐版本升级 |
| 损坏恢复 | 检测到初始化失败时，备份损坏文件后重建空库 |
| 关闭 | 同步关闭，安全用于 `process.exit` 回调 |

### SQLite 优化配置

| Pragma | 值 | 作用 |
|--------|-----|------|
| `foreign_keys` | ON | 启用外键约束 |
| `busy_timeout` | 5000 | 并发写入等待 5 秒，避免 "database is locked" |
| `journal_mode` | WAL | Write-Ahead Logging，提升并发读写性能 |

## 数据表

### users（用户表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 用户 ID（如 `user_1712345678000`） |
| username | TEXT | UNIQUE NOT NULL | 用户名 |
| email | TEXT | UNIQUE | 邮箱（可选） |
| password_hash | TEXT | NOT NULL | 密码哈希（bcrypt） |
| avatar_path | TEXT | | 头像文件路径 |
| jwt_secret | TEXT | | 用户独立的 JWT secret |
| created_at | INTEGER | NOT NULL | 创建时间（Unix ms） |
| updated_at | INTEGER | NOT NULL | 更新时间（Unix ms） |
| last_login | INTEGER | | 最后登录时间 |

索引：`username`、`email`

系统启动时自动创建一个 `system_default_user` 占位用户（空密码），用于单用户模式。

### conversations（会话表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 会话 ID |
| user_id | TEXT | FK → users.id | 所属用户 |
| name | TEXT | NOT NULL | 会话名称 |
| type | TEXT | NOT NULL | 会话类型（`acp`/`gemini`/`codex`/`openclaw-gateway`/`nanobot`/`aionrs`/`remote`） |
| extra | TEXT | NOT NULL | JSON，类型相关的扩展数据 |
| model | TEXT | | JSON，AI 模型配置（`TProviderWithModel`） |
| status | TEXT | CHECK | `pending` / `running` / `finished` |
| source | TEXT | | 会话来源（`aionui` / `telegram` / `lark` / `dingtalk` / `weixin`） |
| channel_chat_id | TEXT | | 通道聊天隔离 ID（如 `user:xxx`、`group:xxx`） |
| created_at | INTEGER | NOT NULL | 创建时间 |
| updated_at | INTEGER | NOT NULL | 更新时间 |

索引：`user_id`、`updated_at`、`type`、`(user_id, updated_at DESC)`、`source`、`(source, updated_at DESC)`

CASCADE 删除：用户删除时级联删除其所有会话。

### messages（消息表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 消息 ID |
| conversation_id | TEXT | FK → conversations.id | 所属会话 |
| msg_id | TEXT | | 消息来源 ID（用于流式消息合并标识） |
| type | TEXT | NOT NULL | 消息类型（`text`/`image`/`file`/`card` 等） |
| content | TEXT | NOT NULL | JSON，消息内容 |
| position | TEXT | CHECK | `left` / `right` / `center` / `pop` |
| status | TEXT | CHECK | `finish` / `pending` / `error` / `work` |
| hidden | INTEGER | | 0 或 1，是否隐藏 |
| created_at | INTEGER | NOT NULL | 创建时间 |

索引：`conversation_id`、`created_at`、`type`、`msg_id`、`(conversation_id, created_at)`

CASCADE 删除：会话删除时级联删除其所有消息。

### teams（团队表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 团队 ID |
| user_id | TEXT | FK → users.id | 所有者用户 |
| name | TEXT | NOT NULL | 团队名称 |
| workspace | TEXT | NOT NULL | 工作区路径 |
| workspace_mode | TEXT | NOT NULL DEFAULT 'shared' | 工作区模式 |
| lead_agent_id | TEXT | NOT NULL DEFAULT '' | 主导 Agent ID |
| agents | TEXT | NOT NULL DEFAULT '[]' | JSON，Agent 列表 |
| created_at | INTEGER | NOT NULL | 创建时间 |
| updated_at | INTEGER | NOT NULL | 更新时间 |

### mailbox（团队消息邮箱）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 邮件 ID |
| team_id | TEXT | FK → teams.id | 所属团队 |
| to_agent_id | TEXT | NOT NULL | 收件 Agent |
| from_agent_id | TEXT | NOT NULL | 发件 Agent |
| type | TEXT | NOT NULL DEFAULT 'message' | 消息类型 |
| content | TEXT | NOT NULL | 消息内容 |
| summary | TEXT | | 摘要 |
| read | INTEGER | NOT NULL DEFAULT 0 | 是否已读 |
| created_at | INTEGER | NOT NULL | 创建时间 |

### team_tasks（团队任务）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 任务 ID |
| team_id | TEXT | FK → teams.id | 所属团队 |
| subject | TEXT | NOT NULL | 任务主题 |
| description | TEXT | | 任务描述 |
| status | TEXT | NOT NULL DEFAULT 'pending' | 状态 |
| owner | TEXT | | 负责人 |
| blocked_by | TEXT | NOT NULL DEFAULT '[]' | JSON，阻塞任务列表 |
| blocks | TEXT | NOT NULL DEFAULT '[]' | JSON，被阻塞任务列表 |
| metadata | TEXT | NOT NULL DEFAULT '{}' | JSON，元数据 |
| created_at | INTEGER | NOT NULL | 创建时间 |
| updated_at | INTEGER | NOT NULL | 更新时间 |

### assistant_plugins（通道插件表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 插件 ID |
| type | TEXT | NOT NULL | 插件类型（`telegram` / `lark` / `dingtalk` / `weixin`） |
| name | TEXT | NOT NULL | 插件名称 |
| enabled | INTEGER | NOT NULL DEFAULT 0 | 是否启用 |
| config | TEXT | | JSON，包含加密的 credentials 和 config |
| status | TEXT | | 插件状态（`running` / `stopped` / `error`） |
| last_connected | INTEGER | | 最后连接时间 |
| created_at | INTEGER | NOT NULL | 创建时间 |
| updated_at | INTEGER | NOT NULL | 更新时间 |

注：`config` 字段内的 `credentials` 部分使用 AES 加密存储（通过 `encryptCredentials` / `decryptCredentials`）。

### assistant_users（通道授权用户表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 用户 ID |
| platform_user_id | TEXT | NOT NULL | 平台用户 ID |
| platform_type | TEXT | NOT NULL | 平台类型 |
| display_name | TEXT | | 显示名称 |
| authorized_at | INTEGER | NOT NULL | 授权时间 |
| last_active | INTEGER | | 最后活跃时间 |
| session_id | TEXT | | 关联的会话 ID |

### assistant_sessions（通道会话表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 会话 ID |
| user_id | TEXT | NOT NULL | 关联的授权用户 |
| agent_type | TEXT | NOT NULL | Agent 类型 |
| conversation_id | TEXT | | 对应的内部会话 ID |
| workspace | TEXT | | 工作区 |
| chat_id | TEXT | | 平台聊天 ID |
| created_at | INTEGER | NOT NULL | 创建时间 |
| last_activity | INTEGER | | 最后活动时间 |

### assistant_pairing_codes（配对码表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| code | TEXT | PK | 配对码 |
| platform_user_id | TEXT | NOT NULL | 平台用户 ID |
| platform_type | TEXT | NOT NULL | 平台类型 |
| display_name | TEXT | | 显示名称 |
| requested_at | INTEGER | NOT NULL | 请求时间 |
| expires_at | INTEGER | NOT NULL | 过期时间 |
| status | TEXT | NOT NULL | 状态（`pending` / `approved` / `rejected`） |

### remote_agents（远程 Agent 表）

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | Agent ID |
| name | TEXT | NOT NULL | 名称 |
| protocol | TEXT | NOT NULL | 协议（如 `a2a`） |
| url | TEXT | NOT NULL | 连接 URL |
| auth_type | TEXT | NOT NULL | 认证方式 |
| auth_token | TEXT | | 认证令牌（AES 加密存储） |
| avatar | TEXT | | 头像 |
| description | TEXT | | 描述 |
| device_id | TEXT | | 设备 ID |
| device_public_key | TEXT | | 设备公钥（加密存储） |
| device_private_key | TEXT | | 设备私钥（加密存储） |
| device_token | TEXT | | 设备令牌（加密存储） |
| allow_insecure | INTEGER | DEFAULT 0 | 是否允许不安全连接 |
| status | TEXT | | 状态（`connected` / `disconnected` / `error` / `unknown`） |
| last_connected_at | INTEGER | | 最后连接时间 |
| created_at | INTEGER | NOT NULL | 创建时间 |
| updated_at | INTEGER | NOT NULL | 更新时间 |

### cron_jobs（定时任务表）

通过 migration v10 创建：

| 字段 | 类型 | 约束 | 说明 |
|------|------|------|------|
| id | TEXT | PK | 任务 ID |
| conversation_id | TEXT | | 关联会话 |
| agent_type | TEXT | NOT NULL | Agent 类型 |
| agent_config | TEXT | | JSON，Agent 配置 |
| name | TEXT | NOT NULL | 任务名称 |
| description | TEXT | | 描述 |
| cron_expression | TEXT | NOT NULL | Cron 表达式 |
| timezone | TEXT | NOT NULL DEFAULT 'UTC' | 时区 |
| enabled | INTEGER | NOT NULL DEFAULT 1 | 是否启用 |
| execution_mode | TEXT | DEFAULT 'existing' | 执行模式 |
| prompt | TEXT | NOT NULL | 执行提示词 |
| last_run_at | INTEGER | | 上次执行时间 |
| next_run_at | INTEGER | | 下次执行时间 |
| run_count | INTEGER | NOT NULL DEFAULT 0 | 执行次数 |
| max_runs | INTEGER | | 最大执行次数（NULL = 无限） |
| created_at | INTEGER | NOT NULL | 创建时间 |
| updated_at | INTEGER | NOT NULL | 更新时间 |

## Repository 接口

### IConversationRepository

抽象会话数据访问，业务层通过此 trait 操作会话和消息：

| 方法 | 签名 | 说明 |
|------|------|------|
| getConversation | `(id) → Conversation?` | 获取单个会话 |
| createConversation | `(conversation) → void` | 创建会话 |
| updateConversation | `(id, updates) → void` | 更新会话（部分字段） |
| deleteConversation | `(id) → void` | 删除会话（级联删除消息） |
| getMessages | `(id, page, pageSize, order?) → PaginatedResult<Message>` | 获取会话消息（分页） |
| insertMessage | `(message) → void` | 插入消息 |
| getUserConversations | `(cursor?, offset?, limit?) → PaginatedResult<Conversation>` | 获取用户会话列表（分页） |
| listAllConversations | `() → Conversation[]` | 获取全部会话（无分页） |
| searchMessages | `(keyword, page, pageSize) → SearchResponse` | 全文搜索消息 |
| getConversationsByCronJob | `(cronJobId) → Conversation[]` | 获取定时任务关联的会话 |

### IChannelRepository

抽象通道数据访问：

| 方法 | 签名 | 说明 |
|------|------|------|
| getChannelPlugins | `() → ChannelPluginConfig[]` | 获取所有通道插件 |
| getPendingPairingRequests | `() → PairingRequest[]` | 获取待处理配对请求 |
| getChannelUsers | `() → ChannelUser[]` | 获取所有授权用户 |
| deleteChannelUser | `(userId) → void` | 删除授权用户 |
| getChannelSessions | `() → ChannelSession[]` | 获取所有活跃会话 |

## AionUIDatabase 完整操作清单

`AionUIDatabase` 是实际操作数据库的核心类，直接执行 SQL。以下按业务域列出所有方法：

### 用户操作

| 方法 | 说明 |
|------|------|
| `createUser(username, email?, passwordHash)` | 创建用户 |
| `getUser(userId)` | 按 ID 获取用户 |
| `getUserByUsername(username)` | 按用户名获取（登录用） |
| `getAllUsers()` | 获取所有用户 |
| `getUserCount()` | 用户总数 |
| `hasUsers()` | 是否存在已设置密码的用户 |
| `updateUserLastLogin(userId)` | 更新最后登录时间 |
| `updateUserPassword(userId, hash)` | 更新密码 |
| `updateUserJwtSecret(userId, secret)` | 更新 JWT secret |
| `updateUserUsername(userId, username)` | 更新用户名 |
| `setSystemUserCredentials(username, hash)` | 设置系统用户凭据 |
| `getSystemUser()` | 获取系统默认用户 |

### 会话操作

| 方法 | 说明 |
|------|------|
| `createConversation(conversation, userId?)` | 创建会话 |
| `getConversation(id)` | 获取单个会话 |
| `getUserConversations(userId?, page, pageSize)` | 获取用户会话列表（分页） |
| `updateConversation(id, updates)` | 更新会话 |
| `deleteConversation(id)` | 删除会话 |
| `findChannelConversation(source, chatId, type, backend?, userId?)` | 查找通道会话（按来源+聊天 ID+类型） |
| `updateChannelConversationModel(source, type, model, userId?)` | 批量更新通道会话的模型配置 |
| `getConversationsByCronJobId(cronJobId)` | 获取定时任务关联的会话 |

### 消息操作

| 方法 | 说明 |
|------|------|
| `insertMessage(message)` | 插入消息 |
| `getConversationMessages(convId, page, pageSize, order)` | 获取会话消息（分页） |
| `updateMessage(messageId, message)` | 更新消息 |
| `deleteMessage(messageId)` | 删除单条消息 |
| `deleteConversationMessages(convId)` | 删除会话所有消息 |
| `getMessageByMsgId(convId, msgId, type)` | 按 msg_id 查找消息（流式消息合并用） |
| `searchConversationMessages(keyword, userId?, page, pageSize)` | 全文搜索消息（LIKE 匹配） |

### 通道插件操作

| 方法 | 说明 |
|------|------|
| `getChannelPlugins()` | 获取所有插件（解密 credentials） |
| `getChannelPlugin(pluginId)` | 获取单个插件 |
| `upsertChannelPlugin(plugin)` | 创建或更新插件（加密 credentials） |
| `updateChannelPluginStatus(pluginId, status, lastConnected?)` | 更新插件状态 |
| `deleteChannelPlugin(pluginId)` | 删除插件 |

### 通道用户操作

| 方法 | 说明 |
|------|------|
| `getChannelUsers()` | 获取所有授权用户 |
| `getChannelUserByPlatform(platformUserId, platformType)` | 按平台 ID 查找用户 |
| `createChannelUser(user)` | 创建授权用户 |
| `updateChannelUserActivity(userId)` | 更新最后活跃时间 |
| `deleteChannelUser(userId)` | 删除授权用户 |

### 通道会话操作

| 方法 | 说明 |
|------|------|
| `getChannelSessions()` | 获取所有活跃会话 |
| `getChannelSessionByUser(userId)` | 按用户获取会话 |
| `upsertChannelSession(session)` | 创建或更新会话 |
| `deleteChannelSession(sessionId)` | 删除会话 |

### 配对码操作

| 方法 | 说明 |
|------|------|
| `getPendingPairingRequests()` | 获取待处理请求（过滤过期） |
| `getPairingRequestByCode(code)` | 按配对码查找 |
| `createPairingRequest(request)` | 创建配对请求 |
| `updatePairingRequestStatus(code, status)` | 更新配对状态 |
| `cleanupExpiredPairingRequests()` | 清理过期请求 |

### 远程 Agent 操作

| 方法 | 说明 |
|------|------|
| `getRemoteAgents()` | 获取所有远程 Agent（解密敏感字段） |
| `getRemoteAgent(id)` | 获取单个 |
| `createRemoteAgent(config)` | 创建（加密敏感字段） |
| `updateRemoteAgent(id, updates)` | 更新（自动加密敏感字段） |
| `deleteRemoteAgent(id)` | 删除 |

### 维护操作

| 方法 | 说明 |
|------|------|
| `vacuum()` | 回收空间 |
| `close()` | 关闭连接 |
| `getDriver()` | 获取底层驱动（高级用途） |

## 流式消息缓冲（StreamingMessageBuffer）

优化 AI 流式响应的数据库写入性能。

**核心策略**：

- 不是每个 token chunk 都写库，而是按时间间隔（300ms）或累积数量（20 个 chunk）批量写入
- 每个消息独立维护一个缓冲区，支持 `accumulate`（追加）和 `replace`（覆盖）两种模式
- 性能提升约 100 倍（1000 次 → ~10 次 UPDATE）

**接口**：

| 方法 | 说明 |
|------|------|
| `append(id, messageId, conversationId, chunk, mode)` | 追加流式 chunk 到缓冲区 |

内部自动根据策略刷入数据库。写入时会检查消息是否已存在（`getMessageByMsgId`），存在则 UPDATE，否则 INSERT。

## 迁移系统

- 版本号存储在 SQLite `user_version` pragma 中
- 当前版本：22
- 每次启动时检查：若 `user_version < CURRENT_DB_VERSION` 则执行中间所有迁移
- 迁移支持 up/down 两个方向（虽然 down 实际很少使用）

## 数据模型关系

```
users
  ├── conversations (1:N, CASCADE DELETE)
  │     └── messages (1:N, CASCADE DELETE)
  ├── teams (1:N, CASCADE DELETE)
  │     ├── mailbox (1:N, CASCADE DELETE)
  │     └── team_tasks (1:N, CASCADE DELETE)
  └── (implicit) assistant_*

assistant_plugins (独立)
assistant_users (独立)
  └── assistant_sessions (逻辑关联)
assistant_pairing_codes (独立)

remote_agents (独立)
cron_jobs (独立，通过 conversation_id 逻辑关联)
```

## 加密策略

敏感数据在写入前加密、读取后解密：

| 数据 | 加密方式 | 涉及表 |
|------|---------|-------|
| 通道插件凭据（API token 等） | AES（`encryptCredentials`/`decryptCredentials`） | assistant_plugins.config |
| 远程 Agent 认证信息 | AES（`encryptString`/`decryptString`） | remote_agents.auth_token/device_* |

## 模块依赖

- **被依赖**：几乎所有业务模块（认证、会话、通道、团队、定时任务等）都依赖数据库层
- **依赖**：
  - `process/utils`：路径工具（`getDataPath`、`ensureDirectory`）
  - `common/config/storage`：`TChatConversation`、`ConversationSource` 等业务类型
  - `common/chat/chatLib`：`TMessage` 消息类型
  - `common/types/database`：`IMessageSearchItem`、`IMessageSearchResponse`
  - `process/channels/types`：通道相关类型定义
  - `process/channels/utils/credentialCrypto`：凭据加密工具
  - `process/agent/remote`：`RemoteAgentConfig` 类型

## 候选公共类型

以下类型在多模块间共享，候选归入 `aionui-common` crate：

| 类型 | 来源 | 说明 |
|------|------|------|
| `IQueryResult<T>` | database/types | 通用查询结果包装 |
| `IPaginatedResult<T>` | database/types | 通用分页结果 |
| `PaginatedResult<T>` | IConversationRepository | 简化分页结果（无 page/pageSize） |
| `IUser` | database/types | 用户模型 |
| `IMessageSearchItem` / `IMessageSearchResponse` | common/types/database | 消息搜索结果 |

## Rust 迁移备注

1. **驱动选择**：统一使用 `rusqlite`（同步）或 `sqlx-sqlite`（异步），无需多驱动适配
2. **Repository trait**：保留 `IConversationRepository` 和 `IChannelRepository` 的 trait 设计，便于测试和后续替换存储引擎
3. **迁移框架**：可使用 `refinery` 或 `sqlx::migrate!` 替代手工迁移管理
4. **类型序列化**：原实现大量使用 JSON 字段（`extra`、`content`、`agents` 等），Rust 中通过 `serde_json` 处理
5. **加密**：凭据加密逻辑迁入独立模块，使用 `aes-gcm` 或 `ring` crate
6. **StreamingMessageBuffer**：可使用 `tokio` 的 debounce/batch 机制重新实现
7. **ID 生成**：原实现用 `user_${Date.now()}`，建议改用 UUID v7（时间有序 + 全局唯一）
