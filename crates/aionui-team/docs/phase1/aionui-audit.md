# AionUi Team 模块后端能力审计（Phase 1）

> **目的**：对照 AionUi 前端 main 分支（`/Volumes/Macintosh HD/Users/zhuqingyu/project/AionUi/`，commit `ed8a6bcd3`），列出 aionui-backend 后端需要复刻的所有 team 能力。
>
> **范围约束**：
> - 只关注后端需要实现的能力，不关注前端渲染（例如 React 页面、SWR 缓存、ipcBridge 事件消费）。
> - 只考虑 ACP agent type（范围内：claude / codex / aionrs / 其他 ACP MCP stdio-capable backend；Gemini 非 ACP 但也出现在 team-capable 白名单里，本文档按原样记录不剔除）。
> - 禁止推测。源码未覆盖到的字段在表格中标 "未在源码找到"。
>
> **相关文档**：
> - [三层提示词详细文本](../team-prompts.md)
> - [内部调度说明（旧版）](../internals.md)（参考价值，未逐字验证）
> - [MCP 通信（旧版）](../mcp.md)（参考价值，未逐字验证）
>
> **AionUi 源码路径索引**（下文中所有路径均相对前端根目录 `/Volumes/Macintosh HD/Users/zhuqingyu/project/AionUi/`）：
>
> | 模块 | 路径 | 行数 |
> |------|------|------|
> | 生命周期服务 | `src/process/team/TeamSessionService.ts` | 843 |
> | Session 协调器 | `src/process/team/TeamSession.ts` | 234 |
> | Teammate 引擎 | `src/process/team/TeammateManager.ts` | 614 |
> | Mailbox | `src/process/team/Mailbox.ts` | 52 |
> | Task 板 | `src/process/team/TaskManager.ts` | 107 |
> | Team 内部 MCP | `src/process/team/mcp/team/TeamMcpServer.ts` | 635 |
> | Team stdio bridge | `src/process/team/mcp/team/teamMcpStdio.ts` | 307 |
> | Team Guide MCP | `src/process/team/mcp/guide/TeamGuideMcpServer.ts` | 262 |
> | Team Guide stdio bridge | `src/process/team/mcp/guide/teamGuideMcpStdio.ts` | 131 |
> | Guide 单例 | `src/process/team/mcp/guide/teamGuideSingleton.ts` | 45 |
> | model list 共享处理器 | `src/process/team/mcp/modelListHandler.ts` | 53 |
> | TCP framing 工具 | `src/process/team/mcp/tcpHelpers.ts` | 206 |
> | MCP ready 等待 | `src/process/team/mcpReadiness.ts` | 50 |
> | Leader prompt | `src/process/team/prompts/leadPrompt.ts` | 188 |
> | Teammate prompt | `src/process/team/prompts/teammatePrompt.ts` | 114 |
> | Team Guide prompt | `src/process/team/prompts/teamGuidePrompt.ts` | 108 |
> | Team Guide 能力判断 | `src/process/team/prompts/teamGuideCapability.ts` | 22 |
> | Leader label 解析 | `src/process/team/prompts/teamGuideAssistant.ts` | 48 |
> | spawn 工具描述 | `src/process/team/prompts/toolDescriptions.ts` | 19 |
> | Role prompt 分发 | `src/process/team/prompts/buildRolePrompt.ts` | 45 |
> | 消息格式化 | `src/process/team/prompts/formatHelpers.ts` | 15 |
> | Repository trait | `src/process/team/repository/ITeamRepository.ts` | 43 |
> | Team IPC 绑定 | `src/process/bridge/teamBridge.ts` | 126 |
> | 共享类型 | `src/common/types/teamTypes.ts` | 148 |
> | 内部 event bus | `src/process/team/teamEventBus.ts` | 17 |

---

## 1. Team 生命周期

### 1.1 能力清单

| 能力 | AionUi 实现 | 关键代码路径 |
|------|-------------|--------------|
| 创建 team（REST） | `TeamSessionService.createTeam({userId, name, workspace, workspaceMode, agents, sessionMode})`：生成 teamId、逐个 agent 创建对应 conversation（或复用已有 conversationId 实现单聊→team），回填 workspace，落库到 `repo.create(team)` | `TeamSessionService.ts:475-559` |
| 创建 team（MCP spawn） | `TeamGuideMcpServer.handleCreateTeam(args, backend, callerConversationId)`：1) 从 `AION_MCP_BACKEND` env 拿 agent type 2) 校验 team-capable 3) 复用 callerConversationId 作为 leader 4) 调 `teamSessionService.createTeam` 5) emit `team.listChanged` + `conversation.listChanged` + `deepLink.received` 6) 异步 `getOrStartSession` 并向 leader 发 summary message | `TeamGuideMcpServer.ts:165-261` |
| 单聊→team 的 conversation 复用 | `createTeam` 内如果 `agent.conversationId` 存在且 `getConversation` 找到就复用，只更新 `extra.teamId`（和可选 workspace）；否则按 `buildConversationParams` 新建 conversation | `TeamSessionService.ts:488-528` |
| 列出 team | `listTeams(userId) → repo.findAll(userId)` | `TeamSessionService.ts:567-569` |
| 获取 team（带 agent 修复） | `getTeam(id)`：先 `repo.findById` 再 `repairTeamAgentsIfMissing`。后者在 `agents` 为空但能从 conversation `extra.teamId` 反推出 agent 时，根据 `teamMcpStdioConfig.env[TEAM_AGENT_SLOT_ID]` 和 conversation `type/extra` 回填 `TeamAgent`，保证升级后仍能恢复 | `TeamSessionService.ts:392-473, 561-565` |
| 删除 team（级联） | `deleteTeam(id)`：1) `workerTaskManager.kill(conversationId, 'team_deleted')` 杀所有 agent 进程 2) `session.dispose()` 关 MCP server 3) `conversationService.deleteConversation(conversationId)` 删每个 agent 的 conversation 4) `repo.deleteMailboxByTeam(id)` 5) `repo.deleteTasksByTeam(id)` 6) `repo.delete(id)` | `TeamSessionService.ts:571-611` |
| 启动 session | `getOrStartSession(teamId)`：懒启动，若 map 里已有直接返回；否则 new `TeamSession` → `session.startMcpServer()`（起 TCP MCP server）→ 遍历 agents 写回 `teamMcpStdioConfig` 到每个 conversation 的 `extra` → `workerTaskManager.getOrBuildTask(conversationId, { skipCache: true })` 强制重建缓存 agent task；全部成功后才 `sessions.set(teamId, session)`（避免 MCP 启动失败被缓存为坏 session） | `TeamSessionService.ts:757-833` |
| 停止单个 session | `stopSession(teamId)` → `session.dispose()` 并从 map 移除。`TeamSession.dispose()` 内部：1) 遍历 agents 调 `workerTaskManager.kill(conversationId)` 2) `teammateManager.dispose()` 清所有 IPC 监听和 timer 3) `mcpServer.stop()` 关 TCP 4) `removeAllListeners()` | `TeamSessionService.ts:835-842`, `TeamSession.ts:218-233` |
| 停止全部 session | `stopAllSessions()` 并发调 `stopSession`；由 `disposeAllTeamSessions()` 在 app quit 时调用 | `TeamSessionService.ts:840-842`, `teamBridge.ts:123-125` |
| 重命名 team | `renameTeam(id, name)`：只在 repo 改 `name` + `updatedAt`（不影响运行中 session） | `TeamSessionService.ts:710-714` |
| 设置 session mode | `setSessionMode(teamId, sessionMode)`：存库供新 spawn agent 继承 | `TeamSessionService.ts:716-718` |
| 更新 workspace | `updateWorkspace(teamId, newWorkspace)`：更新 team 表 + 所有 agent conversation 的 `extra.workspace/customWorkspace` + `modifyTime` | `TeamSessionService.ts:720-738` |

### 1.2 建团流程时序图（MCP spawn）

```
solo agent (claude/codex)
   │ aion_create_team(summary, [name], [workspace])
   ▼
[team-guide-mcp-stdio.js]
   │ TCP → { tool:"aion_create_team", args, backend, conversation_id, auth_token }
   ▼
TeamGuideMcpServer.handleCreateTeam
   │ 1. backend 白名单校验
   │ 2. workspace 缺省 → 从 callerConversation.extra.workspace 继承
   │ 3. teamName 缺省 → summary 前 5 词
   │
   ▼
TeamSessionService.createTeam({ userId:'system_default_user',
                                 workspaceMode:'shared',
                                 sessionMode:'yolo',
                                 agents:[{
                                   role:'leader', agentType, agentName:'Leader',
                                   conversationId: callerConversationId, // 复用
                                   status:'pending'
                                 }] })
   │ - leader 复用 conversation: 写入 extra.teamId
   │ - 落库 repo.create(team)
   ▼
(返回 TeamGuideMcpServer)
   │ emit ipcBridge.conversation.listChanged  // 侧栏从单聊隐藏
   │ emit ipcBridge.team.listChanged
   │ emit ipcBridge.deepLink.received { action:'navigate', params:{ route:'/team/<id>' } }
   │
   │ void async:
   │    session = await teamSessionService.getOrStartSession(team.id)
   │    await session.sendMessageToAgent(leader.slotId, summary, { silent: leaderIsReused })
   │
   │ 返回 JSON { teamId, name, route, leadAgent, status:'team_created',
   │            next_step:'The team page has been opened automatically. End your turn now.' }
   ▼
solo agent 读 next_step，结束 turn
```

### 1.3 session 启动内部序列

```
getOrStartSession(teamId)
  │
  ├─ session = new TeamSession(team, repo, workerTaskManager, spawnAgent)
  │   └─ 内部构造 Mailbox / TaskManager / TeammateManager / TeamMcpServer
  │
  ├─ await session.startMcpServer()
  │   └─ TeamMcpServer.start(): net.createServer, listen(0,'127.0.0.1')
  │      → emit team.mcpStatus { phase:'tcp_ready', port }
  │
  ├─ for each agent in team.agents:
  │   ├─ stdioConfig = session.getStdioConfig(agent.slotId)   // 带 TEAM_AGENT_SLOT_ID env
  │   ├─ conversationService.updateConversation(id, { extra:{ teamMcpStdioConfig } })
  │   ├─ workerTaskManager.getOrBuildTask(id, { skipCache:true })  // 强制重建 agent task
  │   └─ 失败 → emit team.mcpStatus { phase:'config_write_failed', error }
  │
  └─ sessions.set(teamId, session)   // 全部成功才缓存
```

### 1.4 删除时序图

```
deleteTeam(id)
   │
   ├─ repo.findById(id) → team
   │
   ├─ Promise.allSettled(
   │     team.agents.map(a => workerTaskManager.kill(a.conversationId,'team_deleted'))
   │ )
   │
   ├─ sessions.get(id)?.dispose()        // MCP server / timer / listener 释放
   ├─ sessions.delete(id)
   │
   ├─ Promise.allSettled(
   │     team.agents.map(a => conversationService.deleteConversation(a.conversationId))
   │ )
   │
   ├─ repo.deleteMailboxByTeam(id)
   ├─ repo.deleteTasksByTeam(id)
   └─ repo.delete(id)
```

---

## 2. Agent 生命周期

### 2.1 能力清单

| 能力 | AionUi 实现 | 关键代码路径 |
|------|-------------|--------------|
| REST addAgent | `TeamSessionService.addAgent(teamId, agent)`：每个 teamId 用 `addAgentLocks` 串行化（防止并发 spawn 时 agents 数组 read-modify-write 竞态）；调 `addAgentUnsafe`：继承 workspace + sessionMode，`buildConversationParams` 造 conversation，生成 `slotId = 'slot-' + uuid(8)`，更新 `repo.update({agents, updatedAt})`，调 `session.addAgent(newAgent)` 写入内存。emit `team.listChanged { action:'agent_added' }` | `TeamSessionService.ts:613-678` |
| MCP spawn | `TeamMcpServer.handleSpawnAgent(args, callerSlotId)`：1) 校验 caller 必须是 leader（非 leader 直接抛错） 2) 若提供 `custom_agent_id`，从 `assistants` 配置查 preset，校验 enabled，`agent_type` 被 preset.backend 覆盖 3) `isTeamCapableBackend(agentType)` 校验 4) 若提供 `model`，校验在 `acp.cachedModels[agentType].availableModels` 内（不在只 warn，不拒绝） 5) 调外部 `spawnAgent(name, agentType, model, customAgentId)` 6) 向新 agent mailbox 写入 "You have been spawned as X" 7) `safeWake(newAgent.slotId)` | `TeamMcpServer.ts:373-450` |
| `spawnAgent` 回调 | 在 `getOrStartSession` 闭包中定义：调 `this.addAgent(teamId, {...})`，并在创建后把 session 的 stdioConfig（携带新 slotId）写回新 conversation 的 `extra.teamMcpStdioConfig` | `TeamSessionService.ts:763-787` |
| 启动 agent（MCP 注入） | AcpAgent 创建 session 时 `loadBuiltinSessionMcpServers()` 把 `extra.teamMcpStdioConfig` 包装成 `AcpSessionMcpServer` 注入 `session/new` | `src/process/agent/acp/index.ts:1605-1656` |
| 启动 agent（prompt 注入） | 首次 wake 时，`TeammateManager.wake` 判 `agent.status in {pending, failed}` → `buildRolePrompt` 产生完整 role prompt，拼接 "## Unread Messages" 后一次性 `agentTask.sendMessage({ content / input, msg_id, silent:true })` 下发 | `TeammateManager.ts:94-259` |
| pending → idle 转换 | wake 时 `agent.status === 'pending'` 先 `setStatus(slotId, 'idle')` 再 `setStatus(slotId, 'active')` | `TeammateManager.ts:117-122` |
| active 期间输入字节流 | `mailboxMessages` 读出后：**leader** 的消息写进 prompt `## Unread Messages` 段一起发；**teammate** 的消息除了写进 prompt，还会逐条 emit 为 `acpConversation.responseStream { type:'teammate_message' }` 并 `addMessage` 到 conversation，让 UI 显示左气泡 | `TeammateManager.ts:127-161` |
| 消息格式 | `formatHelpers.formatMessages`：`[From <senderName\|User>] <content>\nFiles: <joined>` | `formatHelpers.ts:1-14` |
| turn 完成 | 监听 `teamEventBus.on('responseStream')`，type=`finish` 或 `error` → `finalizeTurn(conversationId)`：去重 5s、清 wake 锁、active→idle；非 leader → 写 `idle_notification` 给 leader，并在**所有非 leader 都 settle 时**才唤 leader | `TeammateManager.ts:283-452` |
| 全员 idle 唤 leader | `maybeWakeLeaderWhenAllIdle`：遍历 nonLeadAgents，全部在 `{idle,completed,failed,pending}` 才 `wake(leadSlotId)`；避免 idle notification 死循环 | `TeammateManager.ts:440-452` |
| shutdown 协议 | `team_shutdown_agent(agent)`：leader 调用 → `mailbox.write({ type:'shutdown_request', content:'...Reply "shutdown_approved" ...' })` → `safeWake(targetSlotId)` → teammate 用 `team_send_message` 回 `shutdown_approved` 或 `shutdown_rejected: <reason>` → `handleSendMessage` 识别并：approved 时 `removeAgent(fromSlotId)` 并写反馈给 leader，rejected 时写 reason 给 leader | `TeamMcpServer.ts:281-371, 508-536` |
| Leader 不可 shutdown | `handleShutdownAgent` 发现 target role=leader 抛错 `Cannot shut down the team leader.` | `TeamMcpServer.ts:519-521` |
| REST removeAgent | `TeamSessionService.removeAgent(teamId, slotId)` → 活 session 走 `session.removeAgent`（`TeammateManager.removeAgent`）；否则直接改 repo。emit `team.listChanged { action:'agent_removed' }` | `TeamSessionService.ts:740-755` |
| TeammateManager.removeAgent | 1) 若 role=leader 拒绝（log warn） 2) `workerTaskManager.kill(conversationId)` 3) 清 wakeTimeouts、activeWakes、ownedConversationIds、finalizedTurns 4) 从 in-memory agents 过滤 5) emit `team.agentRemoved` 6) 调 `onAgentRemovedFn` 回调持久化 | `TeammateManager.ts:545-581` |
| renameAgent | `TeammateManager.renameAgent`：trim + 去不可见字符 + 小写后唯一性校验；只存**首次** original name 到 `renamedAgents` map，后续 prompt 里显示 `[formerly: X]`。emit `team.agentRenamed` | `TeammateManager.ts:583-613` |
| 状态集合 | `TeammateStatus = 'pending' \| 'idle' \| 'active' \| 'completed' \| 'failed'` | `src/common/types/teamTypes.ts:48` |
| crash recovery | `handleAgentCrash`：侦测 `finish { agentCrash:true }` 或 error 含 `process exited unexpectedly`/`Session not found`；非 leader → 写 testament 给 leader + 杀进程 + 清 wake + `setStatus 'failed'` + wake leader；leader crash 只标 failed，不 auto-remove | `TeammateManager.ts:283-309, 454-543` |
| 429 / 限流识别 | error 文本正则 `/429\|rate.?limit\|quota\|too many requests/i` → `setStatus 'failed'`（不 crash 处理） | `TeammateManager.ts:304-308` |
| inactivity watchdog | wake 后 arm `WAKE_TIMEOUT_MS=60s` 定时器；流中任何 text/tool/thought chunk 都 reset 定时器；无 finish 事件超时 → `handleInactivityTimeout`：setStatus=failed + 写 idle_notification 给 leader（解释 stuck 原因 + 建议）+ wake leader。leader 自己 stuck 时只 setStatus，不递归通知 | `TeammateManager.ts:52, 251-344, 347-385` |

### 2.2 Agent 生命周期状态机

```
         ┌────────────────────────────────────────────────────┐
         ▼                                                    │
     [pending] ──wake()──► [idle]  (仅首次 wake)              │
         │                  │                                 │
         │                  ├── wake()─► [active] ──finish──►─┤
         │                  │                  │              │
         │                  │                  │              │
         │                  │      stream 静默 60s            │
         │                  │      无 finish ─► [failed]      │
         │                  │                                 │
         │                  │      agentCrash / 429          │
         │                  └───────────► [failed]            │
         │                                                    │
         └─────────── removeAgent ─► (从 agents 移除)         │
                                                              │
                                              (re-wake 可重入) ┘
```

---

## 3. MCP 子系统

### 3.1 能力清单

| 能力 | AionUi 实现 | 关键代码路径 |
|------|-------------|--------------|
| Team Guide MCP 生命周期 | App 启动时 `initTeamGuideService(teamSessionService)` 新建 `TeamGuideMcpServer` 并 `start()`（TCP listen 随机端口、auth token=`crypto.randomUUID()`）；`getTeamGuideStdioConfig()` 暴露给 ACP agent；app quit `stopTeamGuideService()` | `teamGuideSingleton.ts:24-45`, `TeamGuideMcpServer.ts:45-77`, `initBridge.ts:40-43` |
| Team Guide 工具集 | 2 个：`aion_create_team`、`aion_list_models`（共享 `handleListModels`） | `TeamGuideMcpServer.ts:155-163` |
| Team Guide 注入时机 | ACP agent `loadBuiltinSessionMcpServers()` 里：**不在 team 里** 且 `shouldInjectTeamGuideMcp(backend)` 通过时，取 `getTeamGuideStdioConfig()`，追加 env `AION_MCP_BACKEND` + `AION_MCP_CONVERSATION_ID` 然后 `buildTeamMcpServer` 包装注入 | `acp/index.ts:1623-1640` |
| Gemini / Aionrs 路径 | Gemini 有独立 `GeminiAgentManager.getMcpServers`；Aionrs 有 `AionrsManager` 额外字段 `awaitReady:true` + `notifyMcpReady(slotId)` | `GeminiAgentManager.ts:341-391`, `AionrsManager.ts:149-201` |
| Team Guide 能力判断 | `shouldInjectTeamGuideMcp(backend)` → `isTeamCapableBackend(backend, cachedInitResults)`：硬编码白名单 `{gemini, claude, codex, aionrs}`，其他 backend 靠 `cachedInitResults.capabilities.mcpCapabilities.stdio===true` | `teamGuideCapability.ts:18-21`, `teamTypes.ts:16-30` |
| Team Guide Prompt 注入 | ACP / Gemini / Aionrs 的 `AgentManager` 在构建 system instructions 时检查 `!isInTeam && shouldInjectTeamGuideMcp(backend)` → `getTeamGuidePrompt({ backend, leaderLabel })` 拼到 `instructions` 数组 | `AcpAgentManager.ts:1024-1052`, `GeminiAgentManager.ts:248-254`, `task/agentUtils.ts:61/165/219` |
| Team Guide Prompt 参数 | `backend`、`leaderLabel`（preset assistant 显示名，由 `resolveLeaderAssistantLabel(presetAssistantId)` 解析，从 `assistants` 配置或 `ASSISTANT_PRESETS` 取按 locale 本地化后的名字） | `teamGuidePrompt.ts:26-50`, `teamGuideAssistant.ts:25-48` |
| Team 内部 MCP 启动 | `TeamSession` 构造时 new `TeamMcpServer({ teamId, getAgents, mailbox, taskManager, spawnAgent, renameAgent, removeAgent, wakeAgent })`；`startMcpServer` 调 `start()` 起 TCP server 并返回 `StdioMcpConfig`；每次 `getStdioConfig(slotId)` 返回带 `TEAM_AGENT_SLOT_ID` env 的 config | `TeamSession.ts:45-92`, `TeamMcpServer.ts:56-118` |
| `StdioMcpConfig` 形状 | `{ name: 'aionui-team-<teamId>', command: 'node', args: [scriptPath], env: [{name:'TEAM_MCP_PORT',value},{name:'TEAM_MCP_TOKEN',value},{name:'TEAM_AGENT_SLOT_ID',value}] }` | `TeamMcpServer.ts:44-49, 101-118` |
| TCP 传输协议 | 4-byte big-endian length header + UTF-8 JSON body；`MAX_MCP_MESSAGE_SIZE=64MB` 防 OOM；buffer 读 O(N)（一次 concat）；`sendTcpRequest` 默认 `timeoutMs=300_000` | `tcpHelpers.ts:18-183` |
| auth | 每个 MCP server 启动时 `authToken = crypto.randomUUID()`；stdio bridge 每次 TCP 请求 payload 带 `auth_token`；服务端校验不符直接 `{ error:'Unauthorized' }` + `socket.end()` | `TeamMcpServer.ts:60, 186-190`, `TeamGuideMcpServer.ts:37, 108-112` |
| MCP ready 握手 | stdio bridge 在 `server.connect(transport)` 后 fire-and-forget 发 `{ type:'mcp_ready', slot_id:TEAM_AGENT_SLOT_ID, auth_token }`；服务端收到调 `notifyMcpReady(slotId)` 解锁 `waitForMcpReady`；AcpAgent `createOrResumeSession` 里 `await waitForMcpReady(slotId, 30_000)`（超时也 resolve，不 reject，graceful degrade） | `teamMcpStdio.ts:286-302`, `TeamMcpServer.ts:193-202`, `mcpReadiness.ts:21-50`, `acp/index.ts:1598-1602` |
| caller 身份传递 | stdio bridge 发请求时附 `from_slot_id = TEAM_AGENT_SLOT_ID`；TeamMcpServer 用来判断 `team_spawn_agent` caller 是否 leader、`team_send_message` 的 fromAgentId | `teamMcpStdio.ts:52-72`, `TeamMcpServer.ts:244-277` |

### 3.2 Team 内部 10 个工具

（工具 description/schema 原文见 [team-prompts.md §5](../team-prompts.md#5-mcp-tool-description-原文后端必须原样复用)。本节只列运行时行为。）

| 工具 | 参数 | 运行时行为 | 关键路径 |
|------|------|------------|----------|
| `team_send_message` | `to`（name 或 `*`）、`message`、`summary?` | 1) 解析 fromAgent：caller slotId → 否则 leader → 否则第一个 agent 2) `to='*'` 广播到除自己之外所有 agent（写 mailbox + safeWake） 3) 单播：`resolveSlotId(to)`（先按 slotId，再按 name 规范化匹配） 4) 识别消息 `"shutdown_approved"` → `removeAgent(fromSlotId)` + 通知 leader；`"shutdown_rejected: <reason>"` → 通知 leader 5) 普通消息：mailbox.write + safeWake | `TeamMcpServer.ts:281-371` |
| `team_spawn_agent` | `name`、`agent_type?`、`custom_agent_id?`、`model?` | 如 §2.1 spawn 行为 | `TeamMcpServer.ts:373-450` |
| `team_task_create` | `subject`、`description?`、`owner?` | `taskManager.create({teamId,subject,description,owner})`，返回 `Task created: [<id前8位>] "<subject>"[ (assigned to X)]` | `TeamMcpServer.ts:452-460` |
| `team_task_update` | `task_id`、`status?`、`owner?` | 校验 status 在 `{pending,in_progress,completed,deleted}`；调 `taskManager.update`；status=completed 时额外 `checkUnblocks(taskId)`（下游 blockedBy 去引用该 taskId，全清空的返回） | `TeamMcpServer.ts:462-482` |
| `team_task_list` | — | `taskManager.list(teamId)` 格式化 `- [<id8>] <subject> (<status>, owner: <X> \| unassigned)` | `TeamMcpServer.ts:484-494` |
| `team_members` | — | 返回 `- <agentName> (type: <T>, role: <R>, status: <S>[, model: <M>])` | `TeamMcpServer.ts:496-506` |
| `team_rename_agent` | `agent`、`new_name` | 委托 `renameAgent` 回调（`TeammateManager.renameAgent`），返回 `Agent renamed: "<old>" → "<new>"` | `TeamMcpServer.ts:615-634` |
| `team_shutdown_agent` | `agent` | 如 §2.1 shutdown 协议；leader 拒绝；否则写 `shutdown_request` 到 mailbox 并 safeWake target | `TeamMcpServer.ts:508-536` |
| `team_describe_assistant` | `custom_agent_id`、`locale?` | 查 `assistants` 配置里 `id===customAgentId && isPreset`；合并 `ASSISTANT_PRESETS` 的 `nameI18n/descriptionI18n/promptsI18n`；按 `resolveLocaleKey(locale ?? language ?? 'en-US')` 本地化；输出固定 markdown 模板（# title / Backend / Description / Skills / Example tasks / 末尾 spawn 指引） | `TeamMcpServer.ts:538-613` |
| `team_list_models` | `agent_type?` | 共享 `handleListModels`：读 `acp.cachedModels` + `getMergedModelProviders()` + `hasGeminiOauthCreds()`，调 `getTeamAvailableModels(backend, cachedModels, providers, isGoogleAuth)`；不传 `agent_type` 时枚举所有 team-capable backend 的 model | `TeamMcpServer.ts:272-273`, `modelListHandler.ts:19-53` |

### 3.3 stdio ↔ TCP 桥架构

```
                  AionUi Electron main 进程
┌────────────────────────────────────────────────────────────────┐
│  TeamGuideMcpServer (单例)             TeamMcpServer (每 team) │
│  net.createServer('127.0.0.1', *)      net.createServer(...)   │
│  authToken=UUID                         authToken=UUID         │
└───────▲────────────────────────────────▲───────────────────────┘
        │ 4-byte length + JSON           │ 4-byte length + JSON
        │ { tool, args, auth_token,      │ { tool, args, auth_token,
        │   backend, conversation_id }   │   from_slot_id }
        │                                │ { type:'mcp_ready',
        │                                │   slot_id, auth_token }
┌───────┴────────┐                ┌──────┴────────┐
│ team-guide-    │                │ team-mcp-     │
│ mcp-stdio.js   │                │ stdio.js      │
│  McpServer     │                │  McpServer    │
│  stdio transport                │  stdio transport
│  env: AION_MCP_PORT/TOKEN       │  env: TEAM_MCP_PORT/TOKEN
│       AION_MCP_BACKEND          │       TEAM_AGENT_SLOT_ID
│       AION_MCP_CONVERSATION_ID  │
└───────▲────────┘                └──────▲────────┘
        │ stdio (JSON-RPC)               │ stdio (JSON-RPC)
┌───────┴────────┐                ┌──────┴────────┐
│ solo ACP agent │                │ team ACP agent│
│ (session/new   │                │ (session/new  │
│  mcpServers:[  │                │  mcpServers:[ │
│   guide config])│               │   team config])│
└────────────────┘                └───────────────┘
```

注意：同一个 agent 进入 team 后，**Team Guide MCP 不再注入**（`!this.extra.teamMcpStdioConfig` 才注入），避免 team 成员把 `aion_create_team` 当工具再次建团。

---

## 4. Scheduler（调度器 / 状态机）

### 4.1 wake 触发源

| 来源 | 调用点 | 备注 |
|------|--------|------|
| user 对 team 说话 | `TeamSession.sendMessage` → `wakeAfterAcceptedDelivery(leadSlotId,'team')` | 写 leader mailbox；wake 失败只 log 不 throw（不让已入箱的消息被重复发送） |
| user 对某 agent 说话 | `TeamSession.sendMessageToAgent(slotId, content, {silent?,files?})` | `silent=true` 时不写 user bubble 到目标 conversation（MCP spawn 创建 team 时对复用 leader 场景使用） |
| `team_send_message` 单播 | `TeamMcpServer.handleSendMessage` → `safeWake(targetSlotId)` | safeWake = `wakeAgent(slotId).catch(log)`，不 await |
| `team_send_message` 广播 | 除自己外所有 agent 并发 `safeWake` | 同上 |
| `team_spawn_agent` | `safeWake(newAgent.slotId, 'spawn <name>')` | spawn 后写入欢迎消息并立即唤醒 |
| `team_shutdown_agent` | `safeWake(resolvedSlotId, 'shutdown_request')` | 唤醒 target 让它处理 shutdown_request |
| shutdown approved/rejected | `safeWake(leadSlotId, 'shutdown_approved\|rejected')` | 唤醒 leader 继续 |
| crash testament | `void this.wake(leadAgent.slotId)` | 写 testament 后 wake leader 让它处理 |
| idle → 全员 settled | `maybeWakeLeaderWhenAllIdle` → `wake(leadSlotId)` | 所有非 leader 在 `{idle,completed,failed,pending}` 才 wake；避免 idle notification 死循环 |
| inactivity timeout | 60s 静默 → 写 idle_notification 给 leader + `wake(leadSlotId)` | |

### 4.2 状态迁移（TeammateStatus）

```
pending ──wake(first)──► idle ──setStatus──► active ──finish──► idle
                         ▲   ▲                │
                         │   │                ├── agentCrash ──► failed
                         │   │                ├── 429 rate-limit ► failed
                         │   │                ├── inactivity 60s ► failed
                         │   │                │
                         │   └─────wake(re)───┤
                         │                    │
                      failed                  │
                         ▲                    │
                         └────────────────────┘
```

**completed** 状态在代码里被枚举进 `TeammateStatus` 且 `maybeWakeLeaderWhenAllIdle` 把它当 settled；但运行时没有显式迁移到 completed 的路径（`setStatus(... 'completed')` 未在调用方出现）——仅作为"外部 spawn/预填"的可能值保留。

### 4.3 wake 重入与幂等

- `activeWakes: Set<slotId>`：wake 开始时 add，消息发出后立即 delete（避免 finish 事件丢失导致死锁）
- `wakeTimeouts: Map<slotId, timer>`：每次 wake 后 60s 看门狗；任何 stream chunk reset；finish 时 clear
- `finalizedTurns: Set<conversationId>`：5s dedup 窗口，防同一 turn 多次触发 `finalizeTurn`
- wake 正在进行中的 slot 再次 wake → 直接跳过（log debug）

### 4.4 finalize_turn 流程

```
responseStream { type:'finish' | 'error' }
   │
   ├─ ownedConversationIds.has(conversation_id) ? else return
   ├─ 判 agentCrash / 'process exited'/'Session not found' → handleAgentCrash
   ├─ 判 429 rate-limit → setStatus 'failed'
   │
   ▼ finalizeTurn(conversationId)
       │
       ├─ finalizedTurns.has(id) ? return : add + 5s later delete
       ├─ activeWakes.delete(slotId)
       ├─ clearTimeout(wakeTimeouts)
       ├─ if status==='active' → setStatus 'idle'
       │
       └─ if role != 'leader':
             ├─ mailbox.write(to=leader, type='idle_notification',
             │                content='Turn completed')
             └─ maybeWakeLeaderWhenAllIdle(leader.slotId)
                   └─ all nonLead agents in {idle,completed,failed,pending}
                        ├─ yes → wake(leader)
                        └─ no  → skip
```

---

## 5. Prompt 体系

### 5.1 三层结构

（详细文本见 [team-prompts.md](../team-prompts.md)，本节验证实际实现与文档一致性。）

| 层 | 构建器 | 触发时机 | 注入位置 |
|----|--------|----------|----------|
| Layer 1 Team Guide | `getTeamGuidePrompt({backend, leaderLabel})` | solo agent 首次构建 instructions 且不在 team 里且 `shouldInjectTeamGuideMcp(backend)`=true | 拼接进 agent system instructions（非 MCP 协议） |
| Layer 2 Leader | `buildLeaderPrompt({teammates, availableAgentTypes, availableAssistants, renamedAgents, teamWorkspace})` | team leader agent 首次 wake 或 `status=='failed'`（crash recovery） | 作为 `agentTask.sendMessage` 的 content，后接 `## Unread Messages`（若有） |
| Layer 3 Teammate | `buildTeammatePrompt({agent, leader, teammates, renamedAgents, teamWorkspace})` | team 里非 leader agent 首次 wake 或 crash recovery | 同上 |

分发入口 `buildRolePrompt` (`buildRolePrompt.ts:21-44`) 按 `agent.role` 选择 leader / teammate 分支。

### 5.2 动态注入的 context

| 字段 | 来源 | 只注入给 | 备注 |
|------|------|----------|------|
| `availableAgentTypes: [{type, name}]` | `agentRegistry.getDetectedAgents()` 过滤 `isTeamCapableBackend(backend, cachedInitResults)` | leader（首次 prompt） | 仅首次 wake 计算 |
| `availableAssistants: [{customAgentId, name, backend, description, skills}]` | `ProcessConfig.get('assistants')` 过滤 `isPreset && enabled!==false && isTeamCapableBackend(backend)` | leader（首次 prompt） | 仅首次 wake 计算 |
| `renamedAgents: Map<slotId, originalName>` | `TeammateManager` 内存状态 | leader + teammate | 显示 `[formerly: <原名>]` |
| `teamWorkspace: string` | `team.workspace \| undefined` | leader + teammate | 空则不输出 `## Team Workspace / ## Workspaces` 段 |
| `leaderLabel: string` | `resolveLeaderAssistantLabel(presetAssistantId)` | solo agent 的 Guide Prompt | 有 preset 时渲染成 `Word Creator (gemini)`；无 preset 时纯 backend 名 |
| `leader: TeamAgent` | teammates 里过滤 role=leader | teammate | 找不到就用自己 fallback（兜底） |

### 5.3 Team Guide MCP 工具描述来源

- `aion_create_team` description：**动态**由 `getCreateTeamToolDescription()` 返回（`teamGuidePrompt.ts:88-108`），被 stdio bridge 在 import 时引用。
- `aion_list_models` description：**静态**写在 `teamGuideMcpStdio.ts:109-112` inline。
- `team_spawn_agent` description：`TEAM_SPAWN_AGENT_DESCRIPTION` 常量（`toolDescriptions.ts:1-18`），既被 stdio bridge 引用也被后端 prompt 引用（不，只有 stdio bridge 引用）。
- 其他 9 个 team_* 工具 description：**静态**写在 `teamMcpStdio.ts:83-283` inline。

> 这些 description 文本是面向 LLM 的产品语义（"何时/不该何时使用"、"前置条件"等），**后端实现必须原样复用**，否则 agent 行为会漂移（例如 agent 不再等用户确认就直接 spawn）。

### 5.4 验证结论（与 team-prompts.md 对比）

| 项 | team-prompts.md 记录 | 实际源码 | 一致？ |
|---|---|---|---|
| Leader prompt 先出阵容表再 spawn 的步骤 | 已描述 | `leadPrompt.ts:111-127` 有 Workflow 1-15 | 一致 |
| Teammate "Standing By" 超时防护 | 已描述 | `teammatePrompt.ts:85-97` 明确给出 300s 超时解释 | 一致 |
| 依赖串行调度 | 已描述 | `leadPrompt.ts:148-158` 明确 `## Sequencing Dependent Work (CRITICAL — avoid teammate timeouts)` | 一致 |
| Model 选择指引 | 已描述 | `leadPrompt.ts:128-134` `## Model Selection Guidelines` | 一致 |
| shutdown 规则 | 已描述 | `leadPrompt.ts:160-166, 180` 一致 | 一致 |
| 触发 shutdown 必须用户显式要求 | 已描述 | prompt 原文 `When the user explicitly asks` | 一致 |
| Preset Assistant 选择逻辑 | 已描述 | `leadPrompt.ts:60-66` "How to pick a preset" 逻辑一致 | 一致 |
| Guide Prompt 7 步流程 | 已描述 | `teamGuidePrompt.ts:68-82` 有 7 条编号步骤 | 一致 |
| Guide Prompt "最多问一次" | 已描述 | `teamGuidePrompt.ts:66` "ask at most once" | 一致 |

---

## 6. Repository / 数据层（简要）

interface `ITeamRepository` 三合一（`ITeamRepository.ts`）：

- `ITeamCrudRepository`：`create / findById / findAll / update / delete / deleteMailboxByTeam / deleteTasksByTeam`
- `IMailboxRepository`：`writeMessage / readUnread / readUnreadAndMark(原子) / markRead / getMailboxHistory`
- `ITaskRepository`：`createTask / findTaskById / updateTask / findTasksByTeam / findTasksByOwner / deleteTask / appendToBlocks(原子) / removeFromBlockedBy(原子)`

`MailboxMessage.type`：`'message' | 'idle_notification' | 'shutdown_request'`。
`TeamTask.status`：`'pending' | 'in_progress' | 'completed' | 'deleted'`。
Task 依赖是双向链（`blockedBy` + `blocks`）。

---

## 7. 后端必须实现的能力清单（汇总）

### 7.1 REST / IPC 等价入口（backend 需要暴露的 API）

> AionUi 走 ipcBridge；后端要包掉这些 team 逻辑意味着提供等价 HTTP/WebSocket 端点。具体 endpoint 命名/路径由后端决定，此处只列能力。

| 能力 | 参数 | 行为契约 |
|------|------|----------|
| 创建 team（后台 REST） | `{userId, name, workspace, workspaceMode, agents[], sessionMode?}` | 对每个 agent 创建或复用 conversation，写 `extra.teamId`；落 team 表；返回 `TTeam` |
| 创建 team（agent 调 MCP） | `aion_create_team(summary, name?, workspace?)` + system-injected `backend` + `conversation_id` | 复用 caller conversation 作 leader；`sessionMode='yolo'`, `workspaceMode='shared'`；返回 `{teamId, route, leadAgent, next_step}` |
| 列 team | `userId` | `TTeam[]` |
| 获取 team（含 agent 修复） | `id` | 从 conversation `extra.teamId/teamMcpStdioConfig` 反推补 agents |
| 删除 team（级联） | `id` | kill 所有 agent 进程 → dispose session → 删 conversation → 删 mailbox → 删 tasks → 删 team |
| 改 team 名 | `id, name` | 仅落库 |
| 改 session mode | `teamId, sessionMode` | 仅落库（新 spawn 继承） |
| 改 workspace | `teamId, workspace` | 落库 + 更新所有 agent conversation 的 extra.workspace/customWorkspace |
| 加 agent（REST） | `teamId, agent` | per-team mutex 串行化；新 conversation + slotId + teamId 写入；发 `listChanged/agent_added` |
| 移 agent（REST） | `teamId, slotId` | kill 进程 + 清 session 内存 + 持久化 |
| 改 agent 名 | `teamId, slotId, newName` | 规范化 + 唯一性校验 + 落库；若活 session 走 session.renameAgent；记录 originalName 供 prompt |
| 启动 session | `teamId` | 懒启动：起 TCP MCP server → 写回所有 agent 的 `teamMcpStdioConfig` → 重建 agent task → 注册 session |
| 停止 session | `teamId` | kill 所有 agent 进程 + dispose MCP server + 清监听/timer |
| 对 team 发话 | `teamId, content, files?` | 写 leader mailbox（非 silent 时写 user bubble）+ wake leader |
| 对 agent 发话 | `teamId, slotId, content, silent?, files?` | 写 target mailbox（非 silent 时写 user bubble）+ wake target |

### 7.2 MCP 工具（共 12 个）

后端必须同时实现：

**Team Guide MCP（solo agent 用，对应 backend 白名单 agent 注入）**
1. `aion_create_team(summary, name?, workspace?)` — `system.backend`、`system.conversation_id` 由 bridge 注入
2. `aion_list_models(agent_type?)`

**Team 内部 MCP（team 成员用，带 `from_slot_id`）**
3. `team_send_message(to, message, summary?)` — 含 `*` 广播、`shutdown_approved/rejected` 拦截
4. `team_spawn_agent(name, agent_type?, custom_agent_id?, model?)` — leader-only
5. `team_task_create(subject, description?, owner?)`
6. `team_task_update(task_id, status?, owner?)` — completed 自动 `checkUnblocks`
7. `team_task_list`
8. `team_members`
9. `team_rename_agent(agent, new_name)`
10. `team_shutdown_agent(agent)` — leader 不可 shutdown
11. `team_describe_assistant(custom_agent_id, locale?)`
12. `team_list_models(agent_type?)`

**MCP 通信机制必须实现**：
- TCP transport + 4-byte length prefix + UTF-8 JSON（或后端等价机制）
- per-server auth token 校验
- stdio bridge 进程（由 agent CLI 按 `StdioMcpConfig` 启动）
- `mcp_ready` 握手：bridge `server.connect` 后发 `{type:'mcp_ready', slot_id, auth_token}`；service `waitForMcpReady(slotId, 30s)`，超时 resolve 而非 reject
- `MAX_MCP_MESSAGE_SIZE` 保护
- 请求/响应 `timeout 300s`（`sendTcpRequest`）

### 7.3 Scheduler / 状态机必须实现

- `TeammateStatus` 枚举 `pending / idle / active / completed / failed`
- 5 种触发 wake 的来源（§4.1）
- 首次 wake 注入完整 role prompt（`pending/failed`），后续 wake 只发 mailbox messages；mailbox 空时直接设 idle 并释放
- `activeWakes` 重入锁 + `wakeTimeouts` 60s 看门狗 + `finalizedTurns` 5s dedup
- `finalizeTurn`：finish/error → active→idle → 非 leader 写 `idle_notification` → `maybeWakeLeaderWhenAllIdle`
- crash 识别：`finish.agentCrash` / error 含 `process exited unexpectedly` 或 `Session not found` → testament 给 leader + kill + failed + wake leader（leader crash 只 failed 不 remove）
- 429 / rate-limit 识别 → failed
- inactivity 60s 无 stream → failed + idle_notification 给 leader
- leader 不可 shutdown / remove
- 每 team 一个 `addAgentLocks` 互斥串行化并发 spawn

### 7.4 Prompt 必须实现

- `getTeamGuidePrompt({backend, leaderLabel})` — 完整 7 步流程
- `buildLeaderPrompt({teammates, availableAgentTypes, availableAssistants, renamedAgents, teamWorkspace})` — 动态节 4 段
- `buildTeammatePrompt({agent, leader, teammates, renamedAgents, teamWorkspace})` — Standing By + Shutdown 协议原文
- `getCreateTeamToolDescription()` — `aion_create_team` 工具描述（3 PRECONDITIONS + STRICT 流程）
- `TEAM_SPAWN_AGENT_DESCRIPTION` 常量 — `team_spawn_agent` 工具描述
- 10 个 team_* 工具的描述原文（见 team-prompts.md §5.2）
- `resolveLeaderAssistantLabel(presetAssistantId)` — 按 locale 解析 preset 显示名
- `shouldInjectTeamGuideMcp(backend)` — 复用 `isTeamCapableBackend` 白名单 `{gemini, claude, codex, aionrs}` + `cachedInitResults.capabilities.mcpCapabilities.stdio`
- `formatMessages(messages, agents)` — mailbox messages 格式化

### 7.5 数据层必须实现

- Tables：teams / team_agents（或 teams.agents JSON）/ team_mailbox / team_tasks
- `writeMessage / readUnreadAndMark(原子) / getMailboxHistory`
- `createTask / updateTask / findTasksByTeam / findTasksByOwner / appendToBlocks(原子) / removeFromBlockedBy(原子) / findTaskById`
- `deleteMailboxByTeam / deleteTasksByTeam`（级联删除）
- `MailboxMessage.type`：`'message' | 'idle_notification' | 'shutdown_request'`
- `TeamTask.status`：`'pending' | 'in_progress' | 'completed' | 'deleted'`
- Task 双向依赖：`blockedBy[] + blocks[]`

### 7.6 事件 / IPC（后端等价需提供 WebSocket 或 SSE）

AionUi `ipcBridge.team.*` 集合（前端消费）：
- `team.listChanged { teamId, action: 'created' | 'removed' | 'agent_added' | 'agent_removed' }`
- `team.agentSpawned { teamId, agent }`
- `team.agentStatusChanged { teamId, slotId, status, lastMessage? }`
- `team.agentRemoved { teamId, slotId }`
- `team.agentRenamed { teamId, slotId, oldName, newName }`
- `team.mcpStatus { teamId, slotId?, phase, serverCount?, port?, error? }` — phase: `tcp_ready / tcp_error / session_injecting / session_ready / session_error / load_failed / degraded / config_write_failed / mcp_tools_waiting / mcp_tools_ready`

此外 team 内部也会 emit `conversation.responseStream` / `acpConversation.responseStream`（`type: 'user_content' / 'teammate_message'`）驱动 UI 左右气泡——后端实现会变成 "由 backend 主动推给前端的 WS 事件"。

---

## 8. 源码中发现的硬约束（agent 行为易坏点）

这些是 AionUi 代码里写死的、前后端分离必须保留的行为契约：

1. **wake 并发去重**：同一 slotId 正在 wake 时再次 wake 必须跳过（log debug 即可），否则 mailbox 会被双读。
2. **wake 锁释放时机**：消息发出后立即 delete activeWakes（不等 finish），否则 finish 事件丢失会永久死锁。
3. **finalizedTurns 5s 去重窗口**：防同一 turn 多次触发 finalize；但被 re-wake 的 agent 必须先 `finalizedTurns.delete(conversationId)`（`TeammateManager.ts:107-110`）否则新 turn 的 finish 会被吞掉。
4. **maybeWakeLeaderWhenAllIdle 是必须的**：每个 idle_notification 都 wake leader 会造成 leader 反复重派。
5. **leader 首次 prompt 动态注入 availableAgentTypes / availableAssistants**：否则 agent 不知道可以 spawn 什么。
6. **leader 不可 shutdown / remove**：crash 只标 failed 保留槽位；shutdown_agent 显式拒绝。
7. **team_spawn_agent 只有 leader 可调**：非 leader 调用需抛 `Only the team leader can spawn new agents` 错。
8. **shutdown 协议识别消息**：`team_send_message` 收到 `"shutdown_approved"` / `"shutdown_rejected: xxx"` 必须拦截，不能当普通消息入箱。
9. **Standing By = 结束 turn**：这是 prompt 层约定，且后端 60s 看门狗会把"假等待"的 agent 标 failed。
10. **agent 首次 prompt 包含 Unread Messages**：对 teammate 还会把 mailbox messages **额外**作为 UI 左气泡 emit（leader 不 emit，因为消息已在 prompt 里）。
11. **MCP ready 30s 超时 resolve 不 reject**：让 session 降级继续而不是卡死。
12. **auth token 校验**：任何 TCP 请求必须带 `auth_token`；不带直接 `Unauthorized + socket.end()`。
13. **`sessions.set` 必须在 MCP server 启好 + stdio config 全部写回后**：否则失败的 session 会被缓存成坏值。
14. **addAgent 必须 per-team 串行化**：否则并发 spawn 会丢 agent（agents 数组 last-writer-wins）。
15. **userId 常量 `'system_default_user'`**：MCP spawn 建 team 时硬编码（`TeamGuideMcpServer.ts:195`）——后端 multi-tenancy 设计时要考虑如何替换。
16. **`sessionMode='yolo'`、`workspaceMode='shared'`**：MCP spawn 建 team 时硬编码（`TeamGuideMcpServer.ts:200-201`）。
17. **Guide MCP 不会注入进 team 成员**：`!this.extra.teamMcpStdioConfig` 才注入；team 内部 MCP 和 Guide MCP 互斥。

---

## 9. 未在源码找到的事项

- AionUi 没有显式 REST 端点做 "aion_create_team"——它完全依赖 MCP + ipcBridge。后端要暴露 REST 需要后端自己设计。
- `TeammateStatus.completed` 状态在代码里没有显式 setter（仅 type 里有）；`completed` 只被 `maybeWakeLeaderWhenAllIdle` 当 "settled" 条件之一读。后端是否要实现这个状态需要业务决策。
- `workspaceMode: 'shared' | 'isolated'` 中 `isolated` 的实际分支行为未在源码中找到（只有 `shared` 路径）；看 `buildAgentConversationParams` 可能按 `customWorkspace` 差异化处理，但本次审计未追踪到。

---

## 10. 版本锚定

- AionUi 源码 commit：`ed8a6bcd3 fix(bundled-bun): add baseline variant for linux-x64 to support non-AVX2 CPUs (#2654)`（main 分支，2026-04-29 拉取）
- aionui-backend 所在 worktree：`/Users/zhuqingyu/.superset/worktrees/aionui-backend/repeated-algebra`，分支 `docs/api-for-frontend`

