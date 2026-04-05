# ClawCodeRust Detailed Specification: Architecture Overview

## Background and Goals

`design-overview.md` defines the product at a high level: a Claude Code and Codex inspired Rust coding agent built around sessions, turns, items, tools, permissions, compaction, and a transport-neutral API. This document expands that overview into an implementation-ready architecture contract.

Primary goals:

- Make `claw-code-rust` the only implementation target.
- Preserve the conversation and tool-driven behavior described in `design-overview.md`.
- Keep the architecture Rust-native, modular, and testable.
- Provide stable crate and module boundaries that allow incremental delivery.

This overview spec is the umbrella contract for the subordinate specs:

- [Conversation](./spec-conversation.md)
- [Language Model](./spec-language-model.md)
- [Safety](./spec-safety.md)
- [Safety Execution Flow](./spec-safety-execution-flow.md)
- [Context Management](./spec-context-management.md)
- [Tools](./spec-tools.md)
- [Server API](./spec-server-api.md)
- [App Config](./spec-app-config.md)
- [Detailed Specs Index](./spec-detail-index.md)

## Scope

In scope:

- Crate responsibilities and ownership boundaries.
- Shared runtime vocabulary and invariants.
- Cross-cutting requirements for async execution, persistence, observability, and testing.
- The target architecture for the project.

Out of scope:

- Provider-specific HTTP payload minutiae.
- UI rendering details for CLI, desktop, or IDE clients.
- MCP protocol internals beyond the boundaries needed by the agent runtime.

## Architectural Principles

1. Conversation state is the source of truth. Views such as prompts, summaries, and streamed UI events are derived artifacts.
2. Tool execution is explicit and itemized. Every tool request and result must become structured history.
3. Safety is enforced outside the model. The model is informed of constraints, but enforcement is deterministic.
4. Session state and turn state are separate. Session state persists across turns; turn state is disposable and cancellable.
5. Context compaction changes prompt materialization, never the recoverable raw history.
6. Transport and UI are adapters. Core runtime logic must not depend on CLI-specific behavior.
7. User guidance during an active turn should be modeled as same-turn steering with queued pending input, not as implicit interruption and restart.

## Target Crate Responsibilities

| Crate | Responsibility | Mandatory Additions |
| --- | --- | --- |
| `clawcr-core` | Session, turn, item model, main loop, model integration, context management, persistence, and event emission | Add explicit session repository, turn state machine, model catalog, provider adapters, tokenizer estimation hooks, and compaction machinery |
| `clawcr-code` | Long-running execution, coding workflow orchestration, background task state, and completion routing | Add turn-linked task registry, execution lifecycle management, and completion notification flow |
| `clawcr-tools` | Tool traits, registry, orchestration, execution metadata | Add typed tool-call journal records and approval integration points |
| `clawcr-safety` | Policy evaluation, rules, approval scopes, secret redaction contracts, and sandbox integration | Add resource-scoped approvals, rule persistence, policy snapshots, and platform safety adapters |
| `clawcr-mcp` | MCP connection management and dynamic capability ingestion | Expand from placeholder into server registry and bridge adapters |
| `clawcr-server` | Transport-neutral runtime server, JSON-RPC lifecycle, subscriptions, and connection management | Add stdio and WebSocket listeners, session routing, approval response plumbing, and event fanout |
| `clawcr-cli` | Local bootstrap, config loading, REPL, and human-oriented terminal UX | Add client-side server bootstrap hooks and approval UX adapters |
| `clawcr-utils` | Cross-cutting low-level helpers with no stable domain owner | Add shared path normalization, JSONL helpers, process-output truncation utilities, retry helpers, and time/UUID formatting helpers |

## Shared Vocabulary

| Term | Definition |
| --- | --- |
| Session | Persistent conversation identified by UUID v7 and containing turn history |
| Turn | One execution cycle beginning with user input and ending in terminal assistant output, interruption, or failure |
| Item | Smallest persisted execution record, including user input, assistant output, tool use, tool result, reasoning summary, approval request, and approval decision |
| Prompt View | Model-facing materialization of session history after truncation, compaction, and modality filtering |
| Policy Snapshot | Resolved safety state used for a turn, including sandbox, network, and approval caches |
| Summary Snapshot | Recoverable compaction artifact that replaces historical prompt material but does not delete raw item history |

## Cross-Cutting Data Contracts

Every persisted domain object must carry:

- Stable identifier.
- `session_id`.
- `turn_id` when applicable.
- RFC 3339 timestamp in UTC.
- Schema version.

Every streamed event must carry:

- Event name.
- Correlation identifiers: `session_id`, `turn_id`, and `item_id` when applicable.
- Monotonic sequence number per session connection.

Every error exposed outside a crate must be one of:

- Validation error.
- Policy denial.
- Approval required.
- Provider error.
- Sandbox error.
- Persistence error.
- Internal invariant violation.

## Suggested Module Layout

The design assumes the following target module layout:

```text
crates/core/src/
  conversation/
    ids.rs
    item.rs
    turn.rs
    session.rs
    repository.rs
    journal.rs
  runtime/
    loop.rs
    event.rs
    approval.rs
  prompt/
    builder.rs
    truncation.rs
```

```text
crates/tools/src/
  registry.rs
  definition.rs
  execution.rs
  progress.rs
  shell_command.rs
  file_search.rs
  apply_patch.rs
```

```text
crates/safety/src/
  approval.rs
  redaction.rs
  sandbox.rs
  rules.rs
  snapshot.rs
```

```text
crates/core/src/model/
  catalog.rs
  config.rs
  tokenizer.rs
  fallback.rs
  provider.rs
```

```text
crates/core/src/context/
  estimator.rs
  selector.rs
  summarizer.rs
  snapshot.rs
  truncation.rs
```

```text
crates/code/src/
  task.rs
  manager.rs
  workflow.rs
  notifications.rs
```

```text
crates/server/src/
  transport/
    stdio.rs
    websocket.rs
  protocol/
    request.rs
    response.rs
    notification.rs
  session_router.rs
  connection.rs
```

```text
crates/utils/src/
  path.rs
  jsonl.rs
  process.rs
  text.rs
  retry.rs
  time.rs
```

## Async and Concurrency Model

- Use `tokio` as the sole async runtime.
- Session operations are serialized per session through a `SessionHandle` actor or `tokio::Mutex<SessionState>`.
- Read-only tool calls may execute concurrently within a turn.
- Mutating tool calls are serialized in invocation order.
- Approval waits, model streaming, and MCP elicitations must be cancellable.
- Persistence appends must be ordered and awaited before emitting terminal item completion events.

## Client and Server Topology

The architecture must support multiple UI clients, but it must not depend on one mandatory singleton server process.

Supported topology modes:

- embedded server runtime inside a local client process
- spawned child-process server connected over stdio
- separately launched shared server connected over websocket

Rules:

- all modes use the same `clawcr-server` protocol and the same persisted session format
- persisted sessions are the cross-client continuity mechanism
- in-memory loaded-session state is local to one running server process
- a client may attach to a different server process later and resume the same persisted session
- process-local optimizations such as live subscriptions, loaded-session caches, and active-turn handles are ephemeral runtime state rather than durable shared truth

## Persistence and IO Baseline

- Raw session history is stored as JSONL under a date-partitioned directory tree.
- Session metadata and resumable indexes are stored separately from raw item journals.
- Configuration is read from user-level JSON or TOML config, but runtime journals are JSONL only.
- Secrets are never written to model-visible history; redacted values may be written only if the original cannot be reconstructed from logs.

## Observability Baseline

- Structured logs use `tracing`.
- Every turn emits start, completion, interruption, and failure events.
- Metrics must include token usage, compaction count, approval prompts, tool latency, model latency, and persistence write latency.
- Long-running operations should create tracing spans: `session.start`, `turn.start`, `model.stream`, `tool.execute`, `approval.wait`, `compact.run`.

## Security Baseline

- Redaction occurs before any provider request is serialized.
- Sandbox policy is resolved before command execution.
- Approval caches may widen access only within the approved scope.
- Persisted journals must avoid storing raw secrets in text fields, command stderr, or tool output payloads.

## Testing Strategy

Minimum required test layers:

- Unit tests for IDs, item schemas, prompt construction, policy evaluation, and compaction selectors.
- Contract tests for provider normalization and API serialization.
- Integration tests for session resume, fork, approval escalation, compaction recovery, and tool pairing invariants.
- Golden JSON fixtures for streamed event sequences and persisted journal lines.

## Acceptance Criteria

- All runtime behavior described in `design-overview.md` maps to a concrete crate and module owner.
- A session can be persisted, resumed, compacted, and replayed without losing raw history.
- Tool execution, approvals, and compaction all produce structured items and events.
- The API layer can drive the runtime without accessing crate-internal mutable state directly.
- common helper logic that is reused by multiple crates has a clear home in `clawcr-utils` instead of accumulating as duplicated local helpers

## Dependencies With Other Specifications

- Conversation defines the persisted model used everywhere else.
- Language Model defines prompt building inputs and model catalog data.
- Safety defines policy, redaction, and approval contracts.
- Context Management defines prompt view derivation and summary lifecycle.
- Tools defines the built-in tool contract and execution lifecycle.
- Server API defines the external orchestration surface.
- App Config defines cross-cutting runtime defaults and config merge rules.

## Open Questions and Assumptions

Assumptions:

- JSONL persistence belongs in `clawcr-core`, not a new crate, unless persistence complexity grows enough to justify extraction.
- `clawcr-server` is the canonical home for the runtime API surface.

Open questions:

- Whether session metadata should live beside the JSONL journal or in a separate index file per session.
- Whether reasoning raw content should ever be persisted, or only summaries and encrypted payload references.
