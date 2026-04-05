# ClawCodeRust Detailed Specification: Execution Flow With Safety

## Background and Goals

The overview provides a concrete nine-step safety loop. This document turns that loop into a state machine and execution contract shared by `clawcr-core`, `clawcr-safety`, `clawcr-tools`, and `clawcr-server`.

Goals:

- Define exact ordering.
- Prevent unsafe execution races.
- Ensure denials and approvals become first-class history.

## Scope

In scope:

- Runtime sequence from user input to tool execution and continuation.
- Turn suspension and resumption.
- Safety-related item emission.

Out of scope:

- General conversation persistence details already covered in the conversation spec.

## Module Responsibilities and Boundaries

`clawcr-core::runtime` owns the turn state machine and sequencing.

`clawcr-safety` owns policy evaluation, approval prompts, and scope caching.

`clawcr-tools` owns tool schema validation, execution dispatch, and progress emission.

`clawcr-core::model` owns streaming model output normalization but does not decide whether a tool may execute.

## Runtime States

Per turn runtime states:

- `CollectingInput`
- `BuildingContext`
- `AwaitingModel`
- `EvaluatingToolRequest`
- `AwaitingApproval`
- `ExecutingTool`
- `PostProcessing`
- `Completed`
- `Failed`
- `Interrupted`

## Required Runtime Interfaces

```rust
pub struct SafetyRuntimeContext {
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub policy_snapshot: PolicySnapshot,
    pub secrets: SecretStoreHandle,
}
```

```rust
pub enum RuntimeContinuation {
    Continue,
    SuspendForApproval(ApprovalPrompt),
    Abort(AgentError),
}
```

## Step-by-Step Execution Flow

### 1. User Input Arrives

Inputs may include:

- text
- image references
- local file references
- transport metadata

Actions:

- create `TurnRecord`
- append `UserMessage` item
- set state to `BuildingContext`

### 2. System Constructs Context

Inputs to prompt builder:

- recent conversation items
- compacted summary view
- safety constraint summary
- available tools
- current user input

Actions:

- build immutable `PolicySnapshot`
- build prompt envelope
- record any derived system notices only if persisted state changes

### 3. Model Generates Response

Actions:

- send redacted prompt view to provider
- stream assistant text, reasoning summary, and tool call candidates
- append provisional assistant items only after provider output is normalized

Rules:

- assistant text can stream to the client before turn completion
- tool calls are not executed until the full tool request payload is valid

### 4. Tool Call Requested

Actions:

- translate provider tool request into `PermissionRequest`
- validate payload schema
- classify operation resource and targets
- move state to `EvaluatingToolRequest`

### 5. Policy Check

Decision matrix:

- `Allow` -> continue to execution
- `Deny` -> append denial items, inject denial result as tool result or assistant-visible notice, continue model loop
- `Ask` -> append approval request item, suspend turn in `AwaitingApproval`

Rules:

- No sandboxed process may start before policy resolution completes.
- Approval decisions are correlated by `approval_id`.

### 6. User Decision

Reject path:

1. append `ApprovalDecision::Denied`
2. update turn and session policy caches if the denial is scoped
3. inject denial outcome into the model-visible continuation input
4. return to `AwaitingModel`

Approve path:

1. append `ApprovalDecision::Approved`
2. update caches according to selected scope
3. re-enter `ExecutingTool`

### 7. Tool Execution

Actions:

- construct sandbox request from the approved policy snapshot plus approved extensions
- resolve secrets needed for local execution
- transform declared sandbox policy into effective platform-specific execution policy
- run tool
- stream tool progress events
- append `ToolCall`, `ToolProgress`, and `ToolResult` items

Rules:

- Secrets are available to the tool runtime but not written to model-visible output.
- Concurrent execution is allowed only for approved read-only tools.
- The platform sandbox transform must complete before spawning the process; no tool process may start under a partially prepared backend.

### 8. Output Processing

Actions:

- redact tool output for model visibility
- apply truncation policy
- convert output into `ToolResult` continuation input
- set state to `PostProcessing`

### 9. Continue Reasoning

Actions:

- append processed tool result item
- rebuild prompt view
- return to `AwaitingModel`

Terminal conditions:

- assistant emits final response with no tool calls
- user interrupts
- unrecoverable provider or persistence error

## Sequence Constraints

- Approval request item must be appended before notifying clients.
- Approval decision must be appended before tool execution resumes.
- Tool result item must be appended before the next provider call.
- If persistence fails at any step, the turn enters `Failed` and no further execution continues.

## Failure Scenarios

### Approval Channel Lost

- mark turn failed if no client can answer the approval prompt and the mode requires approval

### Sandbox Setup Failure

- append failed tool result with internal failure metadata
- continue model loop only if the failure is representable as a tool error

### Redaction Failure

- fail closed and stop provider continuation

### Tool Timeout

- emit tool result marked error
- keep turn alive unless the runtime-wide timeout is exceeded

## Configuration Definitions

Required fields:

- `approval_timeout_ms`
- `tool_timeout_ms`
- `continue_after_denial: bool`
- `max_tool_call_chain_per_turn`
- `persist_denied_operations: bool`

## Concurrency and Async Model

- Provider streaming, approval waiting, and tool execution are all async.
- Only one safety-critical decision path may be active per tool call.
- Multiple read-only tool calls may execute concurrently only after each has independently passed policy evaluation.
- Approval responses may arrive asynchronously, but the runtime must serialize state updates before resuming execution.

## Observability

Required spans:

- `turn.build_context`
- `turn.policy_check`
- `turn.await_approval`
- `turn.tool_execute`
- `turn.redact_output`

Required events:

- approval requested
- approval accepted
- approval denied
- tool started
- tool completed
- tool failed

## Testing Strategy and Acceptance Criteria

Required tests:

- approval-required tool suspends and resumes same turn
- denial feeds back into subsequent model continuation
- approved session-scope permission suppresses repeated prompts
- redaction runs before provider continuation
- concurrent read-only tool batch does not bypass policy evaluation

Acceptance criteria:

- The runtime follows the nine-step loop from the overview without reordering safety-critical steps.
- Every approval and denial is recoverable from session history.
- Tool execution cannot start before policy resolution and any required approval complete.

## Dependencies With Other Modules

- Safety defines policy and approval data types.
- Conversation stores all emitted items.
- Server API transports approval prompts and user decisions.

## Open Questions and Assumptions

Assumptions:

- Denied tool requests are fed back to the model as structured tool-result-like input rather than discarded.

Open questions:

- Whether approval waiting should survive client disconnects for remote transports.
- Whether multiple approvals in the same turn may be answered out of order or must serialize.
