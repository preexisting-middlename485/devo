# ClawCodeRust Detailed Specification: Detail Index and Rollout

## Background and Goals

Section `2. Detail` in `design-overview.md` is currently a placeholder. This document defines what that section means operationally: it is the entry point into the detailed spec set and the recommended implementation order for the target architecture.

## Scope

In scope:

- index of all detailed specifications
- dependency ordering
- implementation rollout guidance
- definition of done for the design phase

Out of scope:

- feature-by-feature coding tasks
- milestone project management beyond the architecture dependencies

## Detailed Specification Index

| Overview Section | Detailed Spec |
| --- | --- |
| `1. Overview` | [spec-overview-architecture.md](./spec-overview-architecture.md) |
| `1.1 Conversation` | [spec-conversation.md](./spec-conversation.md) |
| `1.2 Language Model` | [spec-language-model.md](./spec-language-model.md) |
| `1.3 Safety` | [spec-safety.md](./spec-safety.md) |
| `Execution Flow with Safety` | [spec-safety-execution-flow.md](./spec-safety-execution-flow.md) |
| `1.4 Context Management` | [spec-context-management.md](./spec-context-management.md) |
| `1.5 Tools` | [spec-tools.md](./spec-tools.md) |
| `1.6 Server API` | [spec-server-api.md](./spec-server-api.md) |
| `1.7 App Config` | [spec-app-config.md](./spec-app-config.md) |
| `2. Detail` | this document |

## Recommended Implementation Order

Phase 1:

- land conversation IDs, turn records, and JSONL persistence in `clawcr-core`
- split prompt view from raw session history
- standardize on UUID v7 session, turn, and item IDs

Phase 2:

- add model catalog loading, provider adapters, and resolved turn model data in `clawcr-core`
- move token budgeting, truncation, and compaction to `clawcr-core::context`

Phase 3:

- establish `clawcr-safety` with approval scopes, policy snapshots, redaction, and sandbox integration
- wire the safety execution loop into tool orchestration

Phase 4:

- add summary-backed compaction and snapshots inside `clawcr-core`

Phase 5:

- add `clawcr-tools` built-in tool registry, shell command runner, and file-search backend
- integrate tool lifecycle persistence and approval routing into the main loop

Phase 6:

- add `clawcr-server` for stdio and WebSocket API adapters
- keep `clawcr-cli` focused on local UX and bootstrap flows

## Definition of Done for the Design Phase

The design phase is complete only when:

- every overview section maps to a standalone detailed specification
- crate ownership is explicit for each subsystem
- all major data structures and interfaces are named
- lifecycle and failure paths are specified
- open questions are called out instead of silently skipped

## Acceptance Criteria

- A new engineer can start implementation without reverse engineering the overview.
- The overview can be used as a navigation page into the detailed specs.
- The docs reflect the intended crate layout and identify where new modules are required.

## Open Questions and Assumptions

Assumptions:

- These detailed specs are the intended source for future implementation work unless a closer `AGENTS.md` or a newer design doc supersedes them.
