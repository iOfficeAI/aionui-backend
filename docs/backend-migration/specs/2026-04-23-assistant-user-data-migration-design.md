# Assistant User Data Migration — Backend Design Spec

**Date:** 2026-04-23
**Scope:** Backend implementation details (new `aionui-assistant` crate, SQLite
schema, HTTP contract, built-in loading, rule-md dispatch) for migrating
user-authored assistants from the Electron frontend to the Rust backend as
the single source of truth.

**Companion spec (frontend-side refactor scope + team plan):**
[`AionUi/docs/backend-migration/specs/2026-04-23-assistant-user-data-migration-design.md`](../../../../AionUi/docs/backend-migration/specs/2026-04-23-assistant-user-data-migration-design.md)

---

## 1. Context & Current State

The backend currently exposes:

- `GET /api/extensions/assistants` — returns assistants contributed by
  installed extensions (`contributes.assistants[]`), resolved live via
  `aionui-extension::resolvers::assistant`. No DB persistence; always live.
- `POST/POST/DELETE /api/skills/assistant-rule/*` — rule-md CRUD against
  `~/.aionui/assistant-rules/{id}.{locale}.md`. Source-agnostic — it just
  reads/writes files by `assistantId`.

**What is not in the backend:**

- Built-in assistants — hard-coded in the frontend
  (`src/common/config/presets/assistantPresets.ts`).
- User-authored assistants — stored in `ConfigStorage.get('assistants')`
  inside Electron's `aionui-config.txt`. Never reaches the backend.

The merge (built-in + user + extension) happens in the frontend on every
`useAssistantList` load.

**Goal of this spec:** move built-in seed data and user-authored storage into
the backend; make `GET /api/assistants` the single merged authoritative list.

## 2. New Crate — `aionui-assistant`

Follows the `aionui-system` + `aionui-mcp` pattern (strongly typed domain
crate). Four modules:

```
crates/aionui-assistant/
├── Cargo.toml
└── src/
    ├── lib.rs              // module exports only
    ├── builtin.rs          // BuiltinAssistantRegistry, JSON manifest loading
    ├── service.rs          // AssistantService — merge + CRUD + dispatch
    ├── state.rs            // AssistantRouterState
    ├── routes.rs           // HTTP handlers
    └── migrations_notes.rs // (optional, dev-only scratch)

tests/
├── service.rs              // integration tests (in-memory DB)
└── builtin.rs              // loader edge cases
```

**Dependencies:**

- `aionui-common` (AppError, pagination if needed)
- `aionui-api-types` (AssistantResponse, request types)
- `aionui-db` (IAssistantRepository, IAssistantOverrideRepository)
- `aionui-auth` (CurrentUser extractor)
- `aionui-extension` (ExtensionRegistry for reading extension-contributed
  assistants — read-only dependency)
- `axum`, `serde`, `tracing`, etc.

Must not depend on `aionui-ai-agent`, `aionui-conversation`, or any other
domain crate. The assistant module is a pure data-layer crate; agents consume
it, not the other way around.

## 3. Data Model

### 3.1 Wire type (`aionui-api-types::AssistantResponse`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantResponse {
    pub id: String,
    pub source: AssistantSource,
    pub name: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub name_i18n: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub description_i18n: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    pub enabled: bool,
    pub sort_order: i32,
    pub preset_agent_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_skill_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_builtin_skills: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub context_i18n: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub prompts_i18n: HashMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AssistantSource {
    Builtin,
    User,
    Extension,
}
```

### 3.2 SQLite schema (migration `003_assistants.sql`)

> The exact migration version is determined by the current head of
> `crates/aionui-db/migrations/`; confirm at implementation time.

```sql
-- User-authored assistants only. Built-ins and extension-contributed are
-- resolved in memory and not stored here.
CREATE TABLE assistants (
    id                        TEXT PRIMARY KEY,
    name                      TEXT NOT NULL,
    description               TEXT,
    avatar                    TEXT,
    preset_agent_type         TEXT NOT NULL DEFAULT 'gemini',
    enabled_skills            TEXT,  -- JSON: string[]
    custom_skill_names        TEXT,  -- JSON: string[]
    disabled_builtin_skills   TEXT,  -- JSON: string[]
    prompts                   TEXT,  -- JSON: string[]
    models                    TEXT,  -- JSON: string[]
    name_i18n                 TEXT,  -- JSON: {locale: string}
    description_i18n          TEXT,  -- JSON: {locale: string}
    prompts_i18n              TEXT,  -- JSON: {locale: string[]}
    created_at                INTEGER NOT NULL,
    updated_at                INTEGER NOT NULL
);

CREATE INDEX idx_assistants_updated_at ON assistants (updated_at DESC);

-- Per-assistant user state. Rows may reference built-in or user ids; never
-- extension ids (extension assistants are read-only). No FK because the
-- referent may live in memory (built-in) rather than a table.
CREATE TABLE assistant_overrides (
    assistant_id   TEXT PRIMARY KEY,
    enabled        INTEGER NOT NULL DEFAULT 1,
    sort_order     INTEGER NOT NULL DEFAULT 0,
    last_used_at   INTEGER,
    updated_at     INTEGER NOT NULL
);
```

### 3.3 Row models (`aionui-db/src/models/assistant.rs`)

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AssistantRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub preset_agent_type: String,
    pub enabled_skills: Option<String>,
    pub custom_skill_names: Option<String>,
    pub disabled_builtin_skills: Option<String>,
    pub prompts: Option<String>,
    pub models: Option<String>,
    pub name_i18n: Option<String>,
    pub description_i18n: Option<String>,
    pub prompts_i18n: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AssistantOverrideRow {
    pub assistant_id: String,
    pub enabled: bool,
    pub sort_order: i32,
    pub last_used_at: Option<i64>,
    pub updated_at: i64,
}
```

### 3.4 Repository traits (`aionui-db/src/repository/assistant.rs`)

```rust
#[async_trait]
pub trait IAssistantRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<AssistantRow>, sqlx::Error>;
    async fn get(&self, id: &str) -> Result<Option<AssistantRow>, sqlx::Error>;
    async fn create(&self, params: &CreateAssistantParams) -> Result<AssistantRow, sqlx::Error>;
    async fn update(&self, id: &str, params: &UpdateAssistantParams) -> Result<Option<AssistantRow>, sqlx::Error>;
    async fn delete(&self, id: &str) -> Result<bool, sqlx::Error>;
    async fn upsert(&self, params: &CreateAssistantParams) -> Result<AssistantRow, sqlx::Error>;
}

#[async_trait]
pub trait IAssistantOverrideRepository: Send + Sync {
    async fn get(&self, assistant_id: &str) -> Result<Option<AssistantOverrideRow>, sqlx::Error>;
    async fn get_all(&self) -> Result<Vec<AssistantOverrideRow>, sqlx::Error>;
    async fn upsert(&self, params: &UpsertOverrideParams) -> Result<AssistantOverrideRow, sqlx::Error>;
    async fn delete(&self, assistant_id: &str) -> Result<bool, sqlx::Error>;
    async fn delete_orphans(&self, valid_ids: &[&str]) -> Result<u64, sqlx::Error>;
}
```

Concrete `SqliteAssistantRepository` / `SqliteAssistantOverrideRepository`
follow the standard pattern.

## 4. Built-in Assistants

### 4.1 Directory layout

Shipped with the backend binary:

```
{backend_exe_dir}/assets/builtin-assistants/
├── assistants.json          # manifest (all built-in metadata)
├── rules/
│   ├── office.en-US.md
│   ├── office.zh-CN.md
│   ├── coding.en-US.md
│   └── coding.zh-CN.md
└── assets/
    ├── avatar-office.svg
    └── avatar-coding.svg
```

### 4.2 `assistants.json` schema

```json
{
  "version": "1.0.0",
  "assistants": [
    {
      "id": "builtin-office",
      "name": "Office Assistant",
      "nameI18n": { "zh-CN": "办公助手", "en-US": "Office Assistant" },
      "description": "...",
      "avatar": "assets/avatar-office.svg",
      "presetAgentType": "gemini",
      "enabledSkills": ["git-workflow"],
      "customSkillNames": [],
      "disabledBuiltinSkills": [],
      "ruleFile": "rules/office.{locale}.md",
      "prompts": ["Write a proposal..."],
      "promptsI18n": { "zh-CN": ["..."] },
      "models": []
    }
  ]
}
```

Field semantics are identical to `AssistantResponse` so merge logic is
straightforward.

### 4.3 Loader (`builtin.rs`)

```rust
pub struct BuiltinAssistantRegistry {
    assistants: HashMap<String, BuiltinAssistant>,
    assets_dir: PathBuf,
}

impl BuiltinAssistantRegistry {
    pub fn load() -> Result<Self, AppError> {
        let assets_dir = resolve_builtin_assets_dir()?;
        let manifest_path = assets_dir.join("assistants.json");

        let content = fs::read_to_string(&manifest_path).map_err(|e| {
            AppError::Internal(format!("Built-in manifest missing: {e}"))
        })?;

        let manifest: BuiltinManifest = serde_json::from_str(&content)?;

        let mut assistants = HashMap::new();
        for a in manifest.assistants {
            // Soft-validate ruleFile existence (log + skip if missing);
            // hard-fail on deserialization errors upstream.
            assistants.insert(a.id.clone(), a);
        }

        Ok(Self { assistants, assets_dir })
    }

    pub fn all(&self) -> impl Iterator<Item = &BuiltinAssistant> { ... }
    pub fn get(&self, id: &str) -> Option<&BuiltinAssistant> { ... }
    pub fn rule_path(&self, id: &str, locale: &str) -> Option<PathBuf> { ... }
    pub fn avatar_path(&self, id: &str) -> Option<PathBuf> { ... }
}

fn resolve_builtin_assets_dir() -> Result<PathBuf, AppError> {
    // Priority:
    //   1. AIONUI_BUILTIN_ASSISTANTS_PATH env var (tests, dev)
    //   2. {exe_dir}/assets/builtin-assistants/
    //   3. {CARGO_MANIFEST_DIR}/../aionui-app/assets/builtin-assistants/ (cargo run)
    ...
}
```

### 4.4 Build-time asset placement

Option A — `build.rs` in `aionui-app` copies `assets/builtin-assistants/` to
`target/{profile}/assets/builtin-assistants/`. Verified on all target platforms
before shipping.

Option B — use `include_dir!` macro to embed at compile time; registry writes
to a temp dir on first load. Higher complexity, avoided unless A fails.

**Decision:** Option A. If it causes issues on a specific platform, fall back
to B for that case.

### 4.5 No DB persistence for built-ins

Built-ins are loaded from disk on every backend start. No seed-into-DB step.
Consequences:

- Changing built-in content = edit files in `assets/builtin-assistants/` and
  restart backend. No migration, no version reconciliation.
- Users never accumulate stale built-in rows.
- Built-in state (`enabled` / `sort_order`) lives in `assistant_overrides`
  indexed by the built-in id; survives built-in content changes as long as
  the id is stable.

## 5. Service Layer

### 5.1 `AssistantService::list()`

Three-source merge algorithm:

```rust
pub async fn list(&self) -> Result<Vec<AssistantResponse>, AppError> {
    let builtin = self.builtin_registry.all();
    let user_rows = self.repo.list().await?;
    let extensions = self.extension_registry.get_assistants().await;
    let overrides = self.override_repo.get_all().await?;

    let overrides_map: HashMap<String, AssistantOverrideRow> = overrides
        .into_iter()
        .map(|o| (o.assistant_id.clone(), o))
        .collect();

    let mut result = Vec::new();

    for b in builtin {
        result.push(AssistantResponse::from_builtin(b, overrides_map.get(&b.id)));
    }
    for u in user_rows {
        result.push(AssistantResponse::from_user(&u, overrides_map.get(&u.id))?);
    }
    for e in extensions {
        result.push(AssistantResponse::from_extension(&e));
    }

    // Sort by sort_order asc, then updated_at desc. Extensions sort_order=0.
    result.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| b.last_used_at.cmp(&a.last_used_at))
    });

    // Opportunistic zombie cleanup
    let valid_ids: Vec<&str> = result.iter().map(|a| a.id.as_str()).collect();
    let _ = self.override_repo.delete_orphans(&valid_ids).await;

    Ok(result)
}
```

### 5.2 Other methods

| Method | Behavior |
| --- | --- |
| `get(id)` | Dispatch by source; returns `AssistantResponse` or `AppError::NotFound`. |
| `create(req)` | Insert user row. Rejects if `req.id` clashes with built-in/extension id. |
| `update(id, req)` | Update user row. `NotFound` → 404; source != user → 403. |
| `delete(id)` | Delete user row + `assistant_overrides.assistant_id=id` + cascade fs (best-effort). |
| `set_state(id, req)` | Upsert `assistant_overrides`. Source=extension → 400. |
| `import(req)` | Bulk insert-only; skip built-in/extension/existing-user id collisions; accumulate per-row errors. See §6.3 for authoritative semantics. |
| `read_rule(id, locale)` | Dispatch: built-in → `{assets}/rules/{ruleFile}`; extension → extension dir; user → `~/.aionui/assistant-rules/{id}.{locale}.md`. |
| `write_rule(id, locale, content)` | User only. Built-in/extension → 400. |
| `delete_rule(id)` | User only. |
| `avatar_path(id)` | Built-in/user dispatch. Extension avatar served via `aion-asset://`. |

### 5.3 Dispatch helpers

```rust
impl AssistantService {
    fn classify(&self, id: &str) -> AssistantSource {
        if self.builtin_registry.get(id).is_some() { return AssistantSource::Builtin; }
        if self.extension_registry.has_assistant(id) { return AssistantSource::Extension; }
        AssistantSource::User
    }
}
```

Note: `User` is the default — a user-authored assistant with a given id is
only confirmed when the repository returns `Some`. Routes that need to verify
existence (PUT, DELETE) perform `repo.get(id)` and return 404 on `None`.

## 6. HTTP Contract

### 6.1 Route table

```rust
pub fn assistant_routes(state: AssistantRouterState) -> Router {
    Router::new()
        .route("/api/assistants", get(list).post(create))
        .route("/api/assistants/:id", put(update).delete(delete_one))
        .route("/api/assistants/:id/state", patch(set_state))
        .route("/api/assistants/:id/avatar", get(get_avatar).post(upload_avatar))
        .route("/api/assistants/import", post(import))
        .with_state(state)
}
```

Rule-md routes (existing in `aionui-extension::skill_routes`) are modified
in-place to dispatch by source.

### 6.2 Request/response types

All types live in `aionui-api-types` (so neither `aionui-assistant` nor the
frontend depends on axum for the shapes). Key types:

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAssistantRequest {
    pub id: Option<String>,              // server-generated if absent
    pub name: String,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub preset_agent_type: Option<String>, // defaults to "gemini"
    pub enabled_skills: Option<Vec<String>>,
    pub custom_skill_names: Option<Vec<String>>,
    pub disabled_builtin_skills: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
    pub models: Option<Vec<String>>,
    pub name_i18n: Option<HashMap<String, String>>,
    pub description_i18n: Option<HashMap<String, String>>,
    pub prompts_i18n: Option<HashMap<String, Vec<String>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAssistantRequest {
    // All fields Option — partial update
    pub name: Option<String>,
    pub description: Option<String>,
    ...
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAssistantStateRequest {
    pub enabled: Option<bool>,
    pub sort_order: Option<i32>,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ImportAssistantsRequest {
    pub assistants: Vec<CreateAssistantRequest>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAssistantsResult {
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<ImportError>,
}

#[derive(Debug, Serialize)]
pub struct ImportError {
    pub id: String,
    pub error: String,
}
```

### 6.3 Per-endpoint behavior

#### `GET /api/assistants`

Returns `ApiResponse<Vec<AssistantResponse>>`. Auth required. No pagination
(list is bounded — built-ins ≤ 50, users typically < 100, extensions ≤ 20).

#### `POST /api/assistants`

| Condition | Status |
| --- | --- |
| happy path | 201 + full `AssistantResponse` |
| `name` empty or missing | 400 `"name is required"` |
| `id` conflicts with built-in | 400 `"Id conflicts with built-in assistant"` |
| `id` conflicts with extension | 400 `"Id conflicts with extension-contributed assistant"` |
| `id` already exists in user table | 409 `"Assistant with id '{id}' already exists"` |
| unauthenticated | 401 |

Server-generated id format: `custom-{unix_ms}-{4 random hex chars}`.

#### `PUT /api/assistants/{id}`

Partial update. Same fields as POST, all optional. Updates `updated_at`.

| Condition | Status |
| --- | --- |
| happy path | 200 |
| classify → Builtin | 403 `"Cannot modify built-in assistant"` |
| classify → Extension | 403 `"Cannot modify extension-contributed assistant"` |
| classify → User but `repo.get` returns None | 404 |

#### `DELETE /api/assistants/{id}`

Sequence:

1. Transaction: delete `assistants` row + delete `assistant_overrides` row.
2. Best-effort fs cleanup (failures logged, not rolled back):
   - `~/.aionui/assistant-rules/{id}.*.md`
   - `~/.aionui/assistant-skills/{id}.*.md`
   - `~/.aionui/assistant-avatars/{id}.*`

| Condition | Status |
| --- | --- |
| happy path | 200 `{"success":true}` |
| classify → Builtin | 403 |
| classify → Extension | 403 |
| user row not found | 404 |

#### `PATCH /api/assistants/{id}/state`

Upsert `assistant_overrides`. Only `enabled`, `sort_order`, `last_used_at`
honored.

| Condition | Status |
| --- | --- |
| happy path | 200 + fresh `AssistantResponse` |
| classify → Extension | 400 `"Extension assistants are read-only"` |
| not found in any source | 404 |

#### `POST /api/assistants/import`

**Bulk insert-only** (not upsert). This endpoint exists solely to support the
one-shot Electron migration path; it must never clobber a user's current edits
on retry. Behavior per row:

- If `id` clashes with a built-in → **skip** (`skipped += 1`).
- If `id` clashes with an extension id → **skip** (`skipped += 1`).
- If `id` already exists in the `assistants` table → **skip**, not overwrite.
  Already-imported rows stay exactly as they are, even if the client resends
  the legacy payload. This makes the migration hook idempotent across
  retries.
- Otherwise → insert. Failures collected per-row, not surfaced as 4xx.

Returns aggregate:

```json
{
  "success": true,
  "data": {
    "imported": 12,
    "skipped": 3,
    "failed": 1,
    "errors": [{ "id": "bad-one", "error": "name is required" }]
  }
}
```

Never returns 4xx for the import itself — only for the request shape
(malformed JSON etc.).

Implementation note: use `IAssistantRepository::create` (not `upsert`) inside
the per-row loop, and treat `sqlx::Error::Database` with a
primary-key-conflict code as the `skipped` case. `upsert` remains on the
repo for other callers; `import` specifically does not use it.

#### `GET /api/assistants/{id}/avatar`

Serves the avatar bytes. Dispatch:

- Built-in → `{assets}/builtin-assistants/assets/{filename}` (content from
  manifest's `avatar` field).
- User → `~/.aionui/assistant-avatars/{id}.{ext}`.
- Extension → 404 (extensions use `aion-asset://` URLs, not this endpoint).

Returns raw bytes with appropriate `Content-Type`. 404 if file missing.

#### `POST /api/assistants/{id}/avatar`

Multipart upload; user only. Stores at
`~/.aionui/assistant-avatars/{id}.{ext}`. Updates the `avatar` field of the
assistant row with the relative path.

### 6.4 Rule-md dispatch (modifies existing endpoints)

**`POST /api/skills/assistant-rule/read`** — unchanged request body:

```json
{ "assistantId": "...", "locale": "en-US" }
```

Service internally:

```rust
match classify(&req.assistant_id) {
    Builtin => {
        let path = builtin_registry.rule_path(&req.assistant_id, &req.locale)?;
        fs::read_to_string(&path).unwrap_or_default()
    }
    Extension => {
        let ext = extension_registry.get_assistant(&req.assistant_id)?;
        ext.resolved_rule_content(&req.locale).unwrap_or_default()
    }
    User => {
        let path = user_rule_path(&req.assistant_id, &req.locale);
        fs::read_to_string(&path).unwrap_or_default()
    }
}
```

Empty-string fallback on missing file preserves the current behavior
documented in `modules/assistant.md`.

**`POST /api/skills/assistant-rule/write`** — classifies; rejects with 400
for built-in or extension; user writes go to the existing path.

**`DELETE /api/skills/assistant-rule/{assistantId}`** — same 400 rejection.

### 6.4a Assistant-skill-md dispatch (parallel treatment)

The `/api/skills/assistant-skill/{read,write,delete}` trio
(`modules/assistant.md` lines 25-27) handles per-assistant skill-definition
md files and must receive the **same source-dispatch treatment** as
`assistant-rule/*`:

- **read** — dispatches by classified source:
  - built-in → read from a parallel `skills/` subdir under
    `assets/builtin-assistants/` (or a per-assistant `skillFile` reference
    in `assistants.json`, mirroring the `ruleFile` pattern).
  - extension → read from the contributing extension's directory
    (the `skillFile` resolved by `contributes.assistants[]`).
  - user → `~/.aionui/assistant-skills/{id}.{locale}.md`.
- **write** — user only; built-in / extension → 400.
- **delete** — user only; built-in / extension → 400.

Failure to dispatch these endpoints leaves a loophole: clients could write a
skill-md keyed by a built-in id, creating drift that the single-source rule
forbids. The dispatch implementation reuses the `AssistantClassifier`
introduced in §6.4.

If the built-in manifest does not yet declare a `skillFile` for every
built-in, reads for missing entries return empty string (same fallback as
`rule/read`).

### 6.5 Auth & CSRF

All `/api/assistants/*` routes: JWT-protected via `auth_middleware`; write
ops also under `csrf_middleware` (applied at the `aionui-app` composition
layer, not inside the crate).

## 7. `aionui-app` Wiring

```rust
// In AppServices::from_database_with_data_dir
let builtin_registry = Arc::new(BuiltinAssistantRegistry::load().unwrap_or_else(|e| {
    warn!("Built-in assistants unavailable: {e}");
    BuiltinAssistantRegistry::empty()
}));

let assistant_repo: Arc<dyn IAssistantRepository> =
    Arc::new(SqliteAssistantRepository::new(pool.clone()));
let override_repo: Arc<dyn IAssistantOverrideRepository> =
    Arc::new(SqliteAssistantOverrideRepository::new(pool.clone()));

let assistant_service = AssistantService::new(
    assistant_repo,
    override_repo,
    builtin_registry,
    extension_registry.clone(),
);
```

Compose in `create_router`:

```rust
let assistant_authenticated =
    assistant_routes(states.assistant.clone())
        .route_layer(from_fn_with_state(auth_mw_state.clone(), auth_middleware));

let router = Router::new()
    ...
    .merge(assistant_authenticated)
    ...;
```

Rule-md routes stay in `aionui-extension::skill_routes` but now receive a
dependency on `BuiltinAssistantRegistry` and `IAssistantRepository` to
perform source dispatch. Alternative: move rule-md routes into
`aionui-assistant` entirely — cleaner but touches more files. **Decision:**
keep rule-md in `aionui-extension` for this spec; carry an
`AssistantClassifier` trait that `aionui-assistant` implements. Revisit the
move in a follow-up cleanup spec.

## 8. Migration Path (Server Side)

### 8.1 Schema migration

Single migration file `NNN_assistants.sql`. The concrete version prefix is
assigned at implementation time — it is one greater than the highest number
currently in `crates/aionui-db/migrations/` when the PR branches off.

- Creates both tables.
- Creates `idx_assistants_updated_at`.
- Idempotent; safe to re-run via sqlx's migration tracking.

### 8.2 No backfill

Server-side migration does not pull from any existing file. The frontend's
Path Y (see companion spec §8) drives imports via `POST /api/assistants/import`.

### 8.3 Built-in changes

Adding/removing/editing built-in assistants = modifying files in
`assets/builtin-assistants/` + rebuilding backend. No DB migration needed.
Removed built-ins automatically get their `assistant_overrides` rows cleaned
by `delete_orphans` on the next `list()` call.

## 9. Testing

### 9.1 Unit tests — `aionui-assistant/src/**/*.rs`

| Scope | Cases |
| --- | --- |
| `BuiltinAssistantRegistry::load` | happy / missing dir / malformed JSON / rule file missing / empty list |
| `AssistantService::list` | only built-in / only user / only extension / three-way / override applied / sort correct / zombie cleanup |
| `classify()` | all three paths; built-in wins over user/ext with same id |
| `create` | happy / name empty / builtin id / extension id / duplicate user id |
| `update` | happy / partial / not found / wrong source |
| `delete` | happy / fs delete failure logged but succeeds / wrong source |
| `set_state` | insert / update / extension reject / not found |
| `import` | all happy / partial failure / builtin collision / extension collision / existing user-id collision (skip, not overwrite) / retry with same payload is a no-op |
| `read_rule` dispatch | three paths; missing file → empty string |
| `write_rule` | user ok / builtin reject / extension reject |
| `read_skill` dispatch | three paths; missing file → empty string |
| `write_skill` | user ok / builtin reject / extension reject |
| `delete_skill` | user only; reject other sources |

### 9.2 HTTP integration tests — `crates/aionui-app/tests/assistants_e2e.rs`

`tower::ServiceExt::oneshot` pattern; fixtures use `init_database_memory()`
+ a temp dir for built-in assets (seeded via
`AIONUI_BUILTIN_ASSISTANTS_PATH`).

At minimum one happy path + one error path per endpoint (see
companion spec §9.2).

### 9.3 Gating commands

Before a PR merges:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

All must pass. Plus a cross-crate integration run that exercises
`rule-md` dispatch inside `aionui-extension`'s tests (which already exist —
rerun them to catch regressions from the dispatch change).

### 9.4 Performance budgets

- `BuiltinAssistantRegistry::load` < 50 ms for 100 built-ins.
- `GET /api/assistants` < 100 ms at 20 built-ins + 50 users + 20 extensions.
- `POST /api/assistants/import` < 500 ms for 100 records.

Add a microbenchmark in `tests/` if concerns arise; not a hard gate.

## 10. Touched-crate Impact Matrix

| Crate | Change | Risk |
| --- | --- | --- |
| `aionui-assistant` | New | Isolated |
| `aionui-db` | New repos + models + migration | Low; additive |
| `aionui-api-types` | New request/response types | Low; additive |
| `aionui-extension` | `skill_routes` dispatch gains classifier for both `assistant-rule/*` AND `assistant-skill/*`; adds `AssistantClassifier` dependency | Medium — rule-md AND skill-md tests must regress-green |
| `aionui-app` | Register new state + routes | Low; follows existing pattern |

No existing endpoints change contract (only internal dispatch changes).

## 11. Open Items for Implementation

1. **Migration version number** — assigned when the implementation PR
   branches off (one greater than the highest migration currently in
   `crates/aionui-db/migrations/`).
2. **`AssistantClassifier` trait location** — `aionui-common` or
   `aionui-extension`? Backend-dev decides during implementation.
3. **Avatar multipart handling** — reuse whichever upload helper already
   exists in the codebase (check `aionui-file` first).
4. **`cargo build` asset placement** — confirm `build.rs` in `aionui-app`
   works on all target platforms (macOS, Linux, Windows). Fall back to
   `include_dir!` if needed.
5. **Rule-md dispatch — move or not** — this spec keeps it in
   `aionui-extension`. Follow-up spec may move it into `aionui-assistant`
   once the module stabilizes.

## 12. Definition of Done (Backend)

- [ ] `aionui-assistant` crate merged with full test coverage
- [ ] Migration file committed; `init_database_memory()` applies cleanly
- [ ] `cargo fmt --all -- --check` ✅
- [ ] `cargo clippy --workspace -- -D warnings` ✅
- [ ] `cargo test --workspace` ✅
- [ ] `crates/aionui-app/tests/assistants_e2e.rs` green (all endpoints)
- [ ] `aionui-extension` rule-md dispatch tests green
- [ ] `assets/builtin-assistants/` populated with at least the current
      frontend preset list
- [ ] `cargo build` produces a binary with assets accessible at runtime on
      **all three target platforms** (macOS, Linux, Windows) — verified by
      running the `list` endpoint smoke probe on each. Platform validation
      is a hard gate, not a follow-up: the verification pilot does not
      close while any platform shows missing/misplaced assets.
- [ ] Backend hand-off doc:
      `handoffs/backend-dev-assistant-user-data-2026-XX-XX.md`
