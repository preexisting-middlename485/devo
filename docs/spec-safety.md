# ClawCodeRust Detailed Specification: Safety

## Background and Goals

The overview defines safety as the combination of:

- Secret protection
- Access control
- User approval
- Constraint injection into model context

This document defines the persistent types, policy interfaces, redaction pipeline, sandbox integration points, and approval scoping rules required to implement that safety model.

## Scope

In scope:

- Secret redaction before provider invocation.
- Policy evaluation for filesystem, shell, network, and custom tools.
- Approval prompts and persisted approval scope.
- Sandbox policy integration and policy snapshots.

Out of scope:

- UI text and visual design of approval dialogs.
- OS-specific sandbox implementation details beyond the interfaces core relies on.

## Reference Rationale

The overview already defines deterministic redaction, sandboxing, and user approval. The concrete approval scopes and policy snapshot shape here are based on reference behavior:

- Claude Code demonstrates that approvals must be more granular than a single boolean mode.
- Codex demonstrates that sandbox policy and approval state need explicit typed structures so tools and transports can share them safely.
- Codex's `sandboxing`, `secrets`, `linux-sandbox`, and `windows-sandbox-rs` crates show that the runtime should separate declared policy, effective merged policy, platform transform, and execution backend instead of collapsing them into one opaque sandbox flag.

## Design Goals

- Enforce safety deterministically outside the model.
- Make decisions inspectable, reproducible, and persistable.
- Separate policy evaluation from sandbox execution.
- Keep approval scope explicit and narrow by default.

## Module Responsibilities and Boundaries

`clawcr-safety` owns:

- Rule parsing and matching.
- Approval scope definitions.
- Approval cache.
- Secret detectors and redactors.
- Secret storage handles and environment-scoped secret lookup.
- Policy snapshot generation.
- Declared-to-effective sandbox policy transforms.
- Platform sandbox adapter selection.

`clawcr-tools` owns:

- Reporting intended resource access in a structured form.

`clawcr-core` owns:

- Blocking execution on `Ask`.
- Recording approval request and decision items.
- Injecting resolved safety constraints into prompt construction.

Sandbox manager integration belongs under `clawcr-safety`; it is not a standalone crate in the target design.

## Core Data Structures

```rust
pub enum ResourceKind {
    FileRead,
    FileWrite,
    ShellExec,
    Network,
    McpServer,
    Custom(String),
}

pub struct OperationTarget {
    pub path: Option<PathBuf>,
    pub host: Option<String>,
    pub command: Option<Vec<String>>,
}
```

```rust
pub enum ApprovalScope {
    Once,
    Turn,
    Session,
    PathPrefix(PathBuf),
    Host(String),
    Tool(String),
}
```

```rust
pub struct PermissionRequest {
    pub tool_name: String,
    pub resource: ResourceKind,
    pub description: String,
    pub target: OperationTarget,
    pub requested_permissions: Option<PermissionProfile>,
    pub justification: Option<String>,
}
```

```rust
pub enum PermissionDecision {
    Allow {
        scope: ApprovalScope,
        reason: PermissionReason,
    },
    Deny {
        reason: PermissionReason,
    },
    Ask {
        prompt: ApprovalPrompt,
        suggested_scope: ApprovalScope,
        reason: PermissionReason,
    },
}
```

```rust
pub struct SecretRedactionReport {
    pub match_count: u32,
    pub detector_kinds: Vec<SecretDetectorKind>,
}
```

```rust
pub enum SecretDetectorKind {
    OpenAiKey,
    AwsAccessKeyId,
    BearerToken,
    SecretAssignment,
    Custom(String),
}
```

```rust
pub enum SecretScope {
    Global,
    Workspace(String),
}
```

```rust
pub struct PolicySnapshot {
    pub declared_sandbox_mode: SandboxMode,
    pub effective_sandbox_policy: EffectiveSandboxPolicy,
    pub readable_roots: Vec<PathBuf>,
    pub writable_roots: Vec<PathBuf>,
    pub network: NetworkPolicy,
    pub rules: Vec<PermissionRule>,
    pub cached_approvals: Vec<CachedApproval>,
    pub denied_history: Vec<DeniedOperation>,
}
```

```rust
pub struct EffectiveSandboxPolicy {
    pub sandbox_type: PlatformSandboxType,
    pub sandbox_policy: SandboxPolicyRecord,
    pub file_system_policy: FileSystemPolicyRecord,
    pub network_policy: NetworkPolicy,
}
```

```rust
pub enum PlatformSandboxType {
    None,
    MacOsSeatbelt,
    LinuxSeccomp,
    WindowsRestrictedToken,
}
```

`PermissionReason` must be structured, not just free-form text:

- rule matched
- mode default
- sandbox restriction
- secret risk
- invalid target

## Secret Protection

Redaction pipeline:

1. Gather model-bound text fragments.
2. Run deterministic detectors over each fragment.
3. Replace matches with typed placeholders.
4. Keep a redaction report for telemetry and debugging.

Required detector categories:

- API keys
- cloud credentials
- bearer tokens
- password-like assignments
- user-defined custom regexes

Required placeholder form:

```text
[REDACTED_SECRET]
```

Rules:

- Redaction must never call the model.
- Raw secret values may remain available to local tool execution.
- Redacted text is what is persisted into provider-visible request logs.
- The default detector set must include regex detectors for OpenAI-style API keys, AWS access key IDs, bearer tokens, and common `key/token/secret/password = value` assignments.
- Detectors must be compiled once at startup or static init time, not per request.
- Redaction is best-effort pattern matching; failure to match an unknown secret format must not be represented as proof of safety.

## Secret Storage and Lookup

The safety subsystem must distinguish between redaction and secret storage.

Required interface:

```rust
pub trait SecretsBackend: Send + Sync {
    fn set(&self, scope: &SecretScope, name: &SecretName, value: &str) -> Result<(), SecretError>;
    fn get(&self, scope: &SecretScope, name: &SecretName) -> Result<Option<String>, SecretError>;
    fn delete(&self, scope: &SecretScope, name: &SecretName) -> Result<bool, SecretError>;
    fn list(&self, scope_filter: Option<&SecretScope>) -> Result<Vec<SecretListEntry>, SecretError>;
}
```

Rules:

- the first implementation may support only a local backend
- secret names must use a restricted stable format such as `A-Z`, `0-9`, and `_`
- workspace-scoped secrets should derive a stable environment identifier from the git repo root when available, otherwise from a canonicalized cwd hash
- secret values must not be stored in normal app config files
- local tool execution may resolve secrets by scope, but provider-visible text must always pass through redaction first

## Access Control Policy

Policy modes supported by the overview:

- `Unrestricted`
- `StaticPolicy`
- `ModelGuidedPolicy`

Implementation note:

- `ModelGuidedPolicy` may use a lightweight local classifier or main-model subcall later, but the runtime contract remains `decide(operation) -> allow | deny | request_approval`.
- Even in model-guided mode, final enforcement remains deterministic in code.

Required policy interface:

```rust
#[async_trait]
pub trait PermissionPolicy {
    async fn decide(
        &self,
        snapshot: &PolicySnapshot,
        request: &PermissionRequest,
    ) -> Result<PermissionDecision, PermissionError>;
}
```

Required policy transform interface:

```rust
pub trait SandboxPolicyTransformer {
    fn effective_permissions(
        &self,
        sandbox_policy: &SandboxPolicyRecord,
        file_system_policy: &FileSystemPolicyRecord,
        network_policy: NetworkPolicy,
        additional_permissions: Option<&PermissionProfile>,
    ) -> Result<EffectiveSandboxPolicy, PermissionError>;
}
```

Transform rules:

- approval-granted additional permissions are normalized before merge
- filesystem approval paths must be canonicalized and deduplicated
- effective policy may widen reads/writes only within the granted approval scope
- effective network policy becomes enabled only when the declared policy or granted additional permissions explicitly allow it
- unrestricted or external-sandbox modes pass through unchanged
- restricted read-only plus newly approved writes may upgrade to a workspace-write style effective policy when the declared type cannot express the granted subset exactly

## Policy Snapshot

Rules:

- The snapshot is constructed at turn start.
- Session-scoped approvals update the next turn snapshot and the current turn state.
- Once-scoped approvals are consumed immediately after use.
- The snapshot must carry both declared user policy and effective merged sandbox policy for the current tool execution attempt.
- The runtime must never hand platform-specific mutable sandbox state directly to prompt construction; prompt construction receives only the summarized constraints.

## Sandbox Backend Architecture

Sandbox execution must be modeled as a transform pipeline:

1. declared sandbox mode and policy are loaded from config/runtime state
2. approval-granted additional permissions are merged into effective filesystem/network policy
3. a platform backend is selected
4. backend-specific command arguments or process tokens are produced
5. the tool is executed under that backend

Platform backend mapping:

- macOS: seatbelt profile generation
- Linux: bubblewrap/seccomp primary path, legacy Landlock fallback only when policy shape is equivalent
- Windows: restricted token plus ACL/firewall/environment preparation

Linux requirements derived from Codex:

- prefer a system `bwrap` outside the workspace when available
- fall back to a bundled helper when system `bwrap` is unavailable
- default to read-only root with explicit writable bind mounts for allowed roots
- re-apply protected subpaths inside writable roots as read-only carveouts
- isolate user and PID namespaces
- when network is restricted, isolate the network namespace unless managed proxy mode is active
- managed proxy mode may allow only proxy-routed traffic and should block arbitrary new local socket creation after setup

Windows requirements derived from Codex:

- create restricted tokens rather than relying only on high-level command wrappers
- apply allow and deny ACLs on resolved paths before process creation
- preserve separate workspace capability SIDs for cwd-scoped write access
- normalize environment variables for non-interactive execution and null-device behavior
- apply network blocking via environment/firewall helper integration when the effective policy forbids network
- restore temporary ACL changes when non-persistent guards were used

macOS requirements:

- compile seatbelt profiles from effective filesystem and network policy
- keep seatbelt execution as a platform adapter, not as a policy decision engine

## Approval Model

Approval decisions must support these scopes:

- once
- turn
- session
- resource scoped by path prefix or host
- tool scoped

Approval requests must include:

- action summary
- why it is needed
- risk explanation
- available scope options

Approval persistence:

- `Once` lives only inside active turn state.
- `Turn` lives until the turn reaches a terminal state.
- `Session` and resource-scoped approvals live in session state and are journaled.
- Approval-granted additional permissions must be merged into effective sandbox policy only for the approved scope and execution lifetime.

## Constraint Injection Into Model Context

The model must receive a synthesized safety summary every turn.

Required summary fields:

- sandbox mode
- writable roots
- network restriction state
- active approval mode
- explicit denials from the current turn or session

Example generated sentences:

- `You may only write under <root>.`
- `Network access is restricted unless approved.`
- `The user denied writes outside the workspace earlier in this session.`

## Error Handling Strategy

`PermissionError` variants:

- `InvalidRequest`
- `PathNormalizationFailed`
- `PolicyUnavailable`
- `ApprovalChannelClosed`
- `SandboxPolicyConflict`
- `SecretBackendUnavailable`
- `SandboxBackendUnavailable`
- `SandboxTransformFailed`

Behavior:

- Invalid permission requests fail closed.
- Missing policy implementation fails closed unless the runtime is explicitly configured as unrestricted.

## Concurrency and Async Model

- Policy evaluation is async because it may wait for user approval or consult external state.
- Approval prompts suspend only the affected turn.
- Concurrent read-only tool calls must still evaluate permissions independently.

## Persistence and Audit

Persist as `ItemRecord` entries:

- approval request
- approval decision
- denial
- redaction summary when non-empty

Audit records must not include raw secrets.
Platform sandbox setup reports may be persisted only as sanitized failure metadata, not raw OS diagnostic dumps containing sensitive paths or environment variables.

## Observability

Metrics:

- `safety.permission.ask.count`
- `safety.permission.deny.count`
- `safety.permission.allow.count`
- `safety.redaction.match.count`
- `safety.approval.wait.duration_ms`

Logs:

- include tool name, resource kind, decision, and scope
- never log secret values or full denied payloads without redaction
- include selected platform sandbox backend and whether additional permissions were merged

## Security and Edge Cases

- Normalize and canonicalize paths before rule checks.
- Reject approval caching for ambiguous path expressions.
- Treat relative paths outside the session cwd as invalid until normalized.
- Do not permit wildcard session approvals for unrestricted network unless explicitly configured.

## Testing Strategy and Acceptance Criteria

Required tests:

- regex-based secret detection
- path normalization and root matching
- approval scope caching
- denial persistence
- prompt constraint summary rendering
- effective policy merge with additional permissions
- Linux backend transform for nested readable/writable/denied paths
- Windows backend path allow/deny planning
- secret-scope lookup and environment-id derivation

Acceptance criteria:

- The model never receives raw secrets detected by the configured redaction pipeline.
- Tool execution is blocked when approval is required but not granted.
- Approval decisions are replayable from session history.

## Dependencies With Other Modules

- Conversation journals approval and denial items.
- Context Management incorporates safety summaries into prompt views.
- Server API transports approval requests and decisions.

## Open Questions and Assumptions

Assumptions:

- Session-scoped approvals should survive resume because the overview explicitly mentions persisted permission state.

Open questions:

- Whether redaction should support pluggable enterprise secret detectors.
- Whether model-guided policy should use the active main model or a dedicated smaller classifier model.
