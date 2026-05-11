# Phase1 影响面分析

> 每个模块的改动是否只影响 team 业务？是否对其他功能零侵入？
>
> **验证标准**：每个模块由不同的人独立检查，确认"旧路径完全不碰"。
>
> **相关文档**：[modules.md](./modules.md) · [interface-contracts.md](./interface-contracts.md) · [backend-audit.md](./backend-audit.md)

---

## 1. 逐模块影响面

| 模块 | 涉及 crate | 改动类型 | 对其他功能的影响 | 隔离机制 |
|------|-----------|---------|----------------|---------|
| D1 | aionui-api-types | **新增** `team_mcp.rs` + lib.rs 导出 | 零影响 — 纯新增文件和 pub use，不改现有类型 | 新文件，不碰旧代码 |
| D2 | aionui-ai-agent | **加 1 个字段** `AcpBuildExtra.team_mcp_stdio_config` | 零影响 — `#[serde(default)]` + `Option`，旧 JSON 反序列化为 `None` | serde default |
| D3 | aionui-team | **新增** `TeamMcpStdioServerSpec` | 零影响 — 纯新增 struct，不改现有 bridge.rs 的 `TeamMcpStdioConfig` | 新增 struct |
| D4 | aionui-team | **修改** `tools.rs` + `server.rs` — 加 2 个工具 | 零影响 — 只追加 descriptor 和 dispatch 分支，不改现有 8 个工具的行为 | 追加分支 |
| D5a/b-1/b-2/c | aionui-team | **重写** `prompts.rs` | ⚠️ **影响 team prompt** — 现有 `build_lead_prompt` / `build_teammate_prompt` / `build_wake_payload` 被替换。但这三个函数**只在 team 单测里调用**（生产零调用，backend-audit P0#8/#9），所以不影响任何运行中功能 | 生产路径无调用者 |
| D6 | aionui-app | **新增** `bridge.rs` + main.rs argv 分支 | 零影响 — 只在 `args == "mcp-bridge"` 时走新路径，正常 `aionui-backend` 启动不走此分支 | argv 门禁 |
| D7 | aionui-team | **修改** `session.rs` — 加 3 个方法 + 改 2 个 send 方法 | ⚠️ **影响 team send_message** — 但这两个方法在当前分支已被删除（本 session 前半段的改动），不影响其他功能 | team-only 方法 |
| D8 | aionui-team | **修改** `scheduler.rs` — 扩展 mark_idle / try_wake / maybe_wake | 零影响 — scheduler 只被 team session 调用，不被其他 crate 引用 | team 内部模块 |
| D9 | aionui-team + aionui-conversation | **修改** `service.rs` + 可能新增 `ConversationService::update_extra` | ⚠️ `update_extra` 是**新增公开方法**，不改现有方法。但需确认不会被意外调用 | 纯新增方法 |
| D10 | aionui-ai-agent | **修改** `acp_agent.rs :: session_new_and_prompt` | 零影响 — `if config.team_mcp_stdio_config.is_some()` 才注入，`None` 时**原有 payload 完全不变** | Option 门禁 |
| D11 | aionui-app | **修改** `state_builders.rs :: build_team_state` 签名 | 零影响 — 只加参数，不改其他 builder | 签名扩展 |

---

## 2. 跨 crate 改动汇总

| 被改的 crate | 改动 | 是否只影响 team？ |
|-------------|------|:---:|
| aionui-api-types | 新增 `TeamMcpStdioConfig` struct | ✅ 纯新增 |
| aionui-ai-agent | `AcpBuildExtra` 加 Option 字段 + `session_new` 加 if 分支 | ✅ Option + if 门禁，单聊不触发 |
| aionui-conversation | 可能新增 `update_extra` 方法 | ✅ 纯新增方法，不改现有 |
| aionui-app | `build_team_state` 加参数 + `mcp-bridge` 子命令 | ✅ 签名扩展 + argv 分支 |
| aionui-team | 多文件改动 | ✅ team 自己的 crate |

**结论**：所有跨 crate 改动都走 `新增` / `Option` / `if some` / `argv 分支` 模式，旧路径零改动。

---

## 3. 风险点（需要开工前/后验证）

| # | 风险 | 验证方法 | 责任模块 |
|---|------|---------|---------|
| R1 | `AcpBuildExtra` 加字段后，旧 conversation.extra 反序列化是否真的不报错 | 写一条单测：用不含 `team_mcp_stdio_config` 的 JSON 反序列化 `AcpBuildExtra`，断言成功且字段为 None | D2 |
| R2 | `session_new` 加 mcpServers 后，ACP CLI 不认这个字段会不会报错 | smoke test：不带 team config 的单聊 conversation 仍能正常 send_message | D11 |
| R3 | `mcp-bridge` 子命令是否增加主二进制体积 | 构建后 `ls -lh target/release/aionui-backend`，对比 rebase 前 | D6 |
| R4 | `build_team_state` 签名变了，其他调用点是否都更新了 | `cargo build` 通过 = 编译器保证 | D11 |
| R5 | D5 重写 prompts.rs 后，现有 team 单测是否仍通过 | `cargo test -p aionui-team` | D5 |
| R6 | `ConversationService::update_extra` 新增方法是否影响 trait object 兼容 | 不是 trait 方法，是 impl 上的方法，不影响 `dyn IConversationRepository` | D9 |

---

## 4. 验证任务（开工前由独立质检员逐模块检查）

**每个模块 PR 合入前，必须由非该模块开发者的人检查以下清单**：

### 通用检查项（每个模块都要过）

- [ ] `cargo build` 通过
- [ ] `cargo test -p <涉及的 crate>` 通过
- [ ] `cargo clippy -p <涉及的 crate> -- -D warnings` 通过
- [ ] 只改了模块描述的文件，没有越界改其他文件
- [ ] 新增代码 ≤ 200 行（有例外的标注了）

### 跨 crate 模块的额外检查（D2 / D6 / D9 / D10 / D11）

- [ ] 单聊回归：用不含 teamId 的 conversation 调 `send_message`，确认行为不变
- [ ] 现有 team CRUD 回归：`POST /api/teams` + `GET /api/teams` + `DELETE /api/teams` 行为不变
- [ ] `cargo test --workspace`（全量回归，在 PR 合入前跑一次）

### Phase1 完工后的全局回归

- [ ] 全量 `cargo test --workspace` 通过
- [ ] 单聊 e2e：建 conversation → 发消息 → 收到回复（确认没被 team 改动干扰）
- [ ] channel 模块不受影响：`cargo test -p aionui-channel` 通过
- [ ] cron 模块不受影响：`cargo test -p aionui-cron` 通过
- [ ] MCP 配置模块不受影响：`cargo test -p aionui-mcp` 通过

---

## 5. 一句话结论

**Phase1 全部改动只激活 team 路径，单聊 / channel / cron / mcp / auth / file / office / shell 等功能零侵入。** 隔离手段：`Option` 字段 + `if some` 门禁 + 纯新增方法/文件 + argv 分支。
