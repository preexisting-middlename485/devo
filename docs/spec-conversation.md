# ClawCodeRust Detailed Specification: Conversation

## Background and Goals

The design overview defines a three-level conversation hierarchy:

- Session
- Turn
- Item

This specification turns that hierarchy into the canonical runtime and persistence model.

Goals:

- Define the recoverable source-of-truth data model.
- Support persistence, replay, resume, and fork.
- Make prompt construction a derived operation rather than the primary state representation.

## Scope

In scope:

- Session, turn, and item identifiers.
- Persistent storage layout and journal schema.
- Session lifecycle and turn lifecycle.
- Rust structs, repository interfaces, and invariants.

Out of scope:

- Provider-specific request shaping.
- UI rendering and transport framing.

## Reference Rationale

The overview establishes session, turn, and item as the primary hierarchy. The detailed journaling and replay requirements here are strengthened by two implementation lessons from the reference codebases:

- Claude Code keeps enough history structure to replay tool interactions and compaction boundaries meaningfully.
- Codex rollout shows that append-only session persistence works best as a single rollout stream with an initial metadata line, while fast listing and lookup can be handled by secondary indexes rather than splitting the primary source of truth across multiple files.

## Design Constraints

The conversation model must preserve:

- Individual item boundaries.
- Turn-level status.
- Approval records.
- Tool progress metadata.
- Replayable persistence.

## Module Responsibilities and Boundaries

`clawcr-core::conversation` owns:

- Identifiers.
- Session and turn metadata.
- Item schema and serialization.
- rollout JSONL append and load.
- Resume and fork reconstruction.
- secondary index updates for listing and lookup.
- session-title state transitions and title update persistence.

`clawcr-core::runtime` owns:

- Transitioning a turn through states.
- Emitting items during execution.
- Translating provider and tool events into items.

`clawcr-core::context` may read items but must not mutate raw persisted history.

## Core Data Structures

### Identifiers

```rust
pub struct SessionId(Uuid);
pub struct TurnId(Uuid);
pub struct ItemId(Uuid);
```

Requirements:

- `SessionId`, `TurnId`, and `ItemId` use UUID v7.
- Newtypes implement `Display`, `Serialize`, `Deserialize`, `Copy` only when cheap and safe.
- IDs are generated only by core runtime factories, not by UI adapters.

### Session Metadata

```rust
pub struct SessionRecord {
    pub id: SessionId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub cwd: PathBuf,
    pub title: Option<String>,
    pub title_state: SessionTitleState,
    pub source: SessionSource,
    pub ephemeral: bool,
    pub parent_session_id: Option<SessionId>,
    pub schema_version: u32,
}
```

```rust
pub enum SessionTitleState {
    Unset,
    Provisional {
        strategy: ProvisionalTitleStrategy,
        generated_at: DateTime<Utc>,
    },
    Final {
        source: FinalTitleSource,
        generated_at: DateTime<Utc>,
    },
}

pub enum ProvisionalTitleStrategy {
    FirstUserMessageDerive,
}

pub enum FinalTitleSource {
    ExplicitUserRename,
    ModelGenerated,
}
```

### Turn Metadata

```rust
pub enum TurnStatus {
    Pending,
    Running,
    WaitingApproval,
    Interrupted,
    Completed,
    Failed,
}

pub struct TurnRecord {
    pub id: TurnId,
    pub session_id: SessionId,
    pub sequence: u32,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: TurnStatus,
    pub model_slug: String,
    pub input_token_estimate: Option<u32>,
    pub usage: Option<TurnUsage>,
}
```

### Item Model

```rust
pub enum ItemKind {
    UserMessage,
    AssistantMessage,
    ReasoningSummary,
    ToolCall,
    ToolResult,
    ToolProgress,
    ApprovalRequest,
    ApprovalDecision,
    ContextSummary,
    SystemNotice,
}

pub struct ItemRecord {
    pub id: ItemId,
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub kind: ItemKind,
    pub payload: ItemPayload,
    pub schema_version: u32,
}
```

`ItemPayload` must be an exhaustive enum, not free-form JSON, with `serde(tag = "type")`.

### Rollout Line Model

Primary persisted history must be written as a single append-only rollout stream.

```rust
pub enum RolloutLine {
    SessionMeta(SessionMetaLine),
    Turn(TurnLine),
    Item(ItemLine),
    SessionTitleUpdated(SessionTitleUpdatedLine),
    CompactionSnapshot(CompactionSnapshotLine),
}
```

```rust
pub struct SessionMetaLine {
    pub timestamp: DateTime<Utc>,
    pub session: SessionRecord,
    pub git: Option<GitSessionInfo>,
}
```

```rust
pub struct TurnLine {
    pub timestamp: DateTime<Utc>,
    pub turn: TurnRecord,
}
```

```rust
pub struct ItemLine {
    pub timestamp: DateTime<Utc>,
    pub item: ItemRecord,
}
```

```rust
pub struct SessionTitleUpdatedLine {
    pub timestamp: DateTime<Utc>,
    pub session_id: SessionId,
    pub title: String,
    pub title_state: SessionTitleState,
    pub previous_title: Option<String>,
}
```

## Persistence Layout

The overview requires JSONL partitioned by date and session ID. The primary persistence model should follow the Codex rollout pattern: one append-only rollout file per session, plus optional secondary indexes for listing and repair.

Required filesystem layout:

```text
<data_root>/sessions/
  2026/
    04/
      05/
        rollout-2026-04-05T12-30-45-<session_id>.jsonl
<data_root>/session_index.jsonl
<data_root>/state/
  state.sqlite
```

Rules:

- the rollout `.jsonl` file is the canonical recoverable source of truth
- the first persisted line for a created session must be `SessionMetaLine`
- subsequent lines append turn metadata changes, item records, and compaction snapshot records in chronological order
- title changes append `SessionTitleUpdatedLine` records in the same chronological stream
- date partition is derived from session creation timestamp
- filename must embed both creation timestamp and session id
- forked sessions create their own rollout file and record `parent_session_id`
- `session_index.jsonl` is optional but recommended as an append-only name lookup index
- SQLite or other structured state stores are optional accelerators for listing, search, and metadata repair; they are not the canonical history source

### Primary Rollout File Rules

- writes are append-only
- every line is standalone JSON
- each line carries its own timestamp
- partial trailing lines may be ignored or rejected on resume, but earlier valid lines remain authoritative
- persistence may flush after every append for durability

### Secondary Index Rules

Optional secondary indexes may store:

- latest session title or thread name
- title state
- created and updated timestamps
- cwd
- git metadata
- archived state
- model provider summary fields
- first user message preview

Rules:

- secondary indexes must be derivable from the rollout file
- missing or stale secondary indexes must be repairable by rescanning rollout files
- resume must not depend on secondary indexes

## Repository Interfaces

```rust
#[async_trait]
pub trait SessionRepository {
    async fn create_session(&self, record: &SessionRecord) -> Result<(), SessionRepoError>;
    async fn append_turn(&self, record: &TurnRecord) -> Result<(), SessionRepoError>;
    async fn append_item(&self, item: &ItemRecord) -> Result<(), SessionRepoError>;
    async fn update_session_title(
        &self,
        session_id: SessionId,
        title: &str,
        title_state: SessionTitleState,
    ) -> Result<(), SessionRepoError>;
    async fn load_session(&self, id: SessionId) -> Result<LoadedSession, SessionRepoError>;
    async fn fork_session(
        &self,
        source_session_id: SessionId,
        new_record: &SessionRecord,
    ) -> Result<LoadedSession, SessionRepoError>;
}
```

`LoadedSession` must include:

- `SessionRecord`
- ordered `Vec<TurnRecord>`
- ordered `Vec<ItemRecord>`
- latest summary snapshot references
- rollout path

## Lifecycle and State Transitions

Session lifecycle:

1. `Created`
2. `Active`
3. `Archived`
4. `Deleted` is out of scope and should not be implemented until retention policy exists

Session title lifecycle:

1. `Unset` when the session is created without an explicit title
2. `Provisional` after the first successful assistant reply if deterministic derivation succeeds
3. `Final(ModelGenerated)` after an asynchronous title-generation upgrade succeeds
4. `Final(ExplicitUserRename)` whenever the user or API explicitly sets a title

Title transition rules:

- explicit title creation or rename always wins and must never be auto-overwritten
- a provisional title may be replaced by one model-generated final title
- once `Final(ExplicitUserRename)` is set, queued automatic title jobs must be canceled or ignored
- automatic title generation must not run before the first assistant reply completes successfully
- session title and first-message preview are separate concepts and must remain separate in storage and API surfaces

Turn lifecycle:

1. `Pending` when accepted by the runtime
2. `Running` after provider execution begins
3. `WaitingApproval` whenever user approval blocks execution
4. `Completed`, `Interrupted`, or `Failed` as terminal states

Transition rules:

- A turn cannot move from a terminal state to a non-terminal state.
- An approval decision resumes the same turn; it must not create a new turn.
- A fork copies all completed turns and items from the source session and starts with no active turn.

## Key Execution Flows

### New Session

1. Generate `SessionId` and `SessionRecord`.
2. Create rollout path from timestamp plus session id.
3. Append `SessionMetaLine`.
4. Update optional secondary indexes.
5. Emit session-created event.
6. Accept first `turn/start`.

### Session Title Generation

This design combines Claude Code's placeholder-first behavior with Codex's explicit metadata update discipline.

1. Create the session with `title = None` and `title_state = Unset` unless the client supplied an explicit title at create time.
2. Persist the first user item as normal and execute the first turn.
3. When the first assistant reply reaches `Completed`, check whether the session already has an explicit title.
4. If not, attempt deterministic provisional derivation from the first user message.
5. If derivation succeeds, append `SessionTitleUpdatedLine`, update secondary indexes, and emit a title-updated event.
6. If config enables asynchronous finalization, queue a background title-generation job using the first completed exchange as input context.
7. When the background job returns a valid title, re-check the current title state.
8. If the title is still `Unset` or `Provisional`, append a second `SessionTitleUpdatedLine` with `Final(ModelGenerated)`.
9. If the user renamed the session while the background job was running, discard the generated result without writing it.

### Provisional Title Derivation

The provisional title path must be deterministic, cheap, and independent of any model call.

Rules:

- source text is the first persisted user-message item of the session
- ignore leading whitespace, markdown quote markers, and obvious shell prompt noise
- collapse internal whitespace to single spaces
- strip fenced code blocks and large pasted code spans before deriving a title candidate
- prefer the first title-worthy clause or sentence, not the full body
- output must be sentence case
- target length is 20 to 60 visible characters
- hard maximum is 80 visible characters
- if the first message is too short, code-only, or otherwise not title-worthy, the session may remain `Unset` until async generation or explicit rename

### Model-Generated Title Contract

The final automatic title path may use a model, but it produces metadata rather than conversational output.

Rules:

- generation input is a prompt view containing the first user message and the first successful assistant reply
- generation runs asynchronously relative to the visible first-turn completion
- output must be a short sentence-case title, not a filename, slug, or markdown heading
- target length is 3 to 8 words
- hard maximum is 80 visible characters
- trailing punctuation should be omitted unless required by a proper noun
- provider failure, timeout, or invalid output must not fail the turn or session; the current title remains unchanged
- only one automatic finalization attempt is required for the first milestone

### Turn Start

1. Generate `TurnId`.
2. Append `TurnLine` with `Pending`.
3. Append initial `UserMessage` item or input item batch as `ItemLine`s.
4. Append updated `TurnLine` with `Running`.
5. Invoke prompt builder.

### Resume Session

1. Read rollout `.jsonl`.
2. Parse the first `SessionMetaLine` as the canonical session header.
3. Replay subsequent `TurnLine`, `ItemLine`, `SessionTitleUpdatedLine`, and snapshot lines in file order.
4. Rebuild in-memory indices.
5. Validate item sequence and tool pair invariants.
6. Reconstruct the latest session title state from the most recent valid title-update line.
7. Reconstruct prompt view lazily on next turn.

### Fork Session

1. Load source session.
2. Materialize a new `SessionRecord` with `parent_session_id = source.id`.
3. Create a new rollout file for the forked session.
4. Append a new `SessionMetaLine` for the forked session.
5. Replay copied raw history into the new rollout stream or persist an explicit fork baseline record.
6. Append a `SystemNotice` item describing fork origin.

## Invariants

- Item `seq` is strictly increasing within a session.
- Turn `sequence` is strictly increasing within a session.
- `ItemRecord.turn_id` must reference an existing turn.
- Every `ToolResult` must reference a prior `ToolCall`.
- Compaction summaries cannot replace raw history; they only affect prompt materialization.
- the effective session title is the latest valid `SessionTitleUpdatedLine` if one exists; otherwise it falls back to `SessionMetaLine.session.title`
- session title and session preview must never be conflated in persistence or API responses

## Configuration Definitions

Conversation-related config fields:

- `data_root: PathBuf`
- `ephemeral_sessions: bool`
- `session_title: SessionTitleConfig`
- `rollout_flush_mode: enum { immediate, buffered }`
- `max_items_per_turn: u32`
- `enable_session_index: bool`
- `enable_state_db: bool`

```rust
pub struct SessionTitleConfig {
    pub mode: SessionTitleMode,
    pub generate_async: bool,
    pub max_title_chars: u16,
}

pub enum SessionTitleMode {
    ExplicitOnly,
    DeriveThenGenerate,
}
```

## Error Handling Strategy

`SessionRepoError` variants:

- `NotFound`
- `AlreadyExists`
- `CorruptRollout`
- `SchemaMismatch`
- `Io`
- `InvariantViolation`

Behavior:

- Corrupt rollout files fail session resume with a hard error once the canonical header or invariant-critical lines are unreadable.
- A failed item append aborts the current turn.
- Buffered writes are allowed only if the process still flushes before sending a terminal turn event.
- Secondary index write failure must not invalidate canonical rollout persistence, but it must surface as a warning and schedule repair.
- Failed automatic title writes must not invalidate the session or turn; they surface as metadata warnings and may be retried only while no explicit title exists

## Concurrency and Async Model

- One session writer task owns append order.
- Read operations may run concurrently with prompt construction if they use immutable loaded state.
- Resume and fork operations lock the target session but not unrelated sessions.
- Secondary index reconciliation may run asynchronously after canonical rollout append succeeds.
- automatic title generation may run on a background task, but title persistence must still be serialized through the session writer

## Observability

Required logs and metrics:

- `conversation.session.created`
- `conversation.session.title.updated`
- `conversation.session.title.generation_failed`
- `conversation.turn.started`
- `conversation.turn.completed`
- `conversation.item.appended`
- `conversation.resume.duration_ms`
- `conversation.fork.duration_ms`
- `conversation.index.repair.count`

## Security and Edge Cases

- User-supplied text may contain secrets; items persisted to disk must store redacted copies when configured.
- Partial rollout writes must be detected during resume by rejecting malformed trailing lines or stopping replay at the final incomplete line.
- Ephemeral sessions must never create on-disk directories.
- automatic title derivation and generation must use the same redacted prompt view that is safe for persistence and telemetry
- titles must not be derived from hidden system prompts, raw reasoning content, or tool-only output that the user never saw

## Testing Strategy and Acceptance Criteria

Required tests:

- UUID v7 ordering and serialization.
- Append and resume round-trip.
- Fork preserves parent history without reusing IDs.
- Tool call/result pair validation.
- Corrupt trailing JSONL line handling.
- secondary index rebuild from rollout file
- provisional title derivation from a natural-language first user message
- explicit rename blocks asynchronous automatic overwrite
- resume reconstructs latest title state from title-update lines
- listing preview remains distinct from canonical session title

Acceptance criteria:

- A session with approvals, tool calls, and compaction can be replayed from a single rollout JSONL file without consulting any UI adapter state.
- Resume and fork preserve ordering and cross-reference integrity.
- after the first successful exchange, a session can expose a stable title without blocking turn completion on a model call

## Dependencies With Other Modules

- Language Model consumes prompt-view projections from conversation state.
- Safety adds approval and redaction item types.
- Context Management consumes items to build summaries.
- API exposes session and turn lifecycle operations.

## Open Questions and Assumptions

Assumptions:

- rollout JSONL is the only required canonical persistence artifact; indexes and state DBs are accelerators.
- the first milestone requires at most one automatic model-generated title upgrade per session

Open questions:

- Whether session metadata should also be mirrored into a global index for faster listing.
- Whether reasoning raw content should be persisted as encrypted blobs or omitted entirely.
