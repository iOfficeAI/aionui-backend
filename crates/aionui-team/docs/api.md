# Team HTTP API

所有端点前缀 `/api/teams`，全部需要 JWT。响应统一 `ApiResponse<T>` / `ErrorResponse`。DTO 定义在 `crates/aionui-api-types/src/team.rs`。

## 端点一览

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/teams` | 创建团队（首个 agent 自动成为 lead） |
| GET | `/api/teams` | 列出所有团队（注意：当前不按 user 过滤，见 bug #5） |
| GET | `/api/teams/{id}` | 获取单个团队详情 |
| DELETE | `/api/teams/{id}` | 删除团队（级联删 agents / mailbox / tasks） |
| PATCH | `/api/teams/{id}/name` | 重命名团队 |
| POST | `/api/teams/{id}/agents` | 新增 agent |
| DELETE | `/api/teams/{id}/agents/{slot_id}` | 移除 agent |
| PATCH | `/api/teams/{id}/agents/{slot_id}/name` | 重命名 agent |
| POST | `/api/teams/{id}/session` | 启动/确保 session 在跑（幂等） |
| DELETE | `/api/teams/{id}/session` | 停止 session |

**Team 模块不提供消息收发端点。** 用户→agent 发消息、拉历史全部走单聊 API，见下方"发消息与拉历史"。

**其它不存在的端点**（前端需知道）：
- `GET /api/teams/{id}/tasks` — 任务板**只能通过 MCP tool** 操作（agent 自己调用），没有 HTTP 入口
- `GET /api/teams/{id}/mailbox` — 邮箱纯内部使用

## 关键字段

### CreateTeamRequest
```json
{ "name": "string",
  "agents": [{ "name": "Alice", "role": "lead", "backend": "claude",
               "model": "claude-opus", "custom_agent_id": null }] }
```
- `agents` 至少 1 个；第一个自动成为 lead（无论 role 写什么）
- `backend`：`acp / claude / gemini / qwen / nanobot / aionrs / remote / openclaw-gateway`
- `role`：`lead / leader / teammate`（大小写敏感）

### TeamResponse
```json
{
  "id": "t_xxx",
  "name": "string",
  "agents": [ TeamAgentResponse ],
  "lead_agent_id": "slot_xxx",
  "created_at": 1730000000000,
  "updated_at": 1730000000000
}
```

### TeamAgentResponse
```json
{
  "slot_id": "slot_xxx",
  "name": "string",
  "role": "lead | teammate",
  "conversation_id": "conv_xxx",
  "backend": "string",
  "model": "string",
  "custom_agent_id": null,
  "status": "idle | working | thinking | tool_use | completed | error"
}
```
**`conversation_id` 是发消息和拉历史的唯一钥匙。**

## 发消息与拉历史

走 `aionui-conversation` 模块的单聊端点，跟普通单聊**完全一致**：

| 动作 | 端点 |
|------|------|
| 用户给 agent 发消息 | `POST /api/conversations/{conversation_id}/messages` |
| 拉 agent 消息历史 | `GET /api/conversations/{conversation_id}/messages` |

`conversation_id` 从 `TeamAgentResponse.conversation_id` 取。请求/响应格式、WS 事件、错误码，全部沿用单聊定义，不在本文档重复。

前置条件：发消息前 team session 必须已启动（调用 `POST /api/teams/{id}/session`，幂等）。

## 错误码

| 错误 | HTTP |
|------|------|
| `TeamNotFound` / `AgentNotFound` | 404 |
| `SessionNotFound` | 404 |
| `InvalidRequest` | 400 |
