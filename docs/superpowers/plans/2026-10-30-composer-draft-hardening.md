# Composer Draft Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make immediate queue dispatch, saved-role restoration, and composer draft storage safe under concurrency and bounded growth.

**Architecture:** Keep the existing task-scoped draft entity and queue model. Reserve a queue item before performing the destructive stop side effect, carry the durable role snapshot through the transient composer draft, and enforce a shared prompt-text bound before persistence. Follow-up tasks close resource-release and API-contract gaps without adding a second state machine.

**Tech Stack:** Rust/Tauri commands, existing prompt queue storage, React 19, Vitest/jsdom, project i18n and shadcn/Radix components.

---

### Task 1: Reserve Immediate Queue Dispatch Before Stop

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Test: `src-tauri/src/commands.rs`

- [x] Add hook-driven command tests proving missing/dispatching items do not invoke stop and stop failure releases the reservation.
- [x] Observe the tests fail against the current stop-before-claim sequence.
- [x] Extract a reservation helper and change `use_conversation_queued_prompt` to claim first, emit queue state, stop/wait only when still busy, and release on stop/settle failure.
- [x] Run the focused Rust tests.

### Task 2: Restore The Durable Role Snapshot

**Files:**
- Modify: `web/src/lib/acp-composer-draft.ts`
- Modify: `web/src/components/acp/ACPChatDialog.tsx`
- Test: `web/tests/acp-composer-draft.test.ts`
- Test: `web/tests/acp-runtime-continue-submit.test.tsx`

- [x] Add tests for changed/deleted/duplicate/space-named role snapshots reaching submission unchanged.
- [x] Observe failures from the current `@name` display-name reconstruction.
- [x] Add a transient role field to `AcpComposerDraft`, render its saved prefix/content through the existing role tag, and make draft capture submit that snapshot rather than a catalog lookup.
- [x] Run focused composer draft tests.

### Task 3: Bound Draft Text

**Files:**
- Modify: `src/provider/mod.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/acp/composer_drafts.rs`
- Test: `src-tauri/src/commands.rs` and `src/acp/composer_drafts.rs`

- [x] Add a shared `MAX_USER_PROMPT_TEXT_CHARS` and command test proving overlong input returns `conversation.prompt-too-long` without writing `composer-drafts.json`.
- [x] Observe the test fail.
- [x] Enforce the bound for all conversation prompt inputs and expose its params through the existing command error mapping.
- [x] Run focused prompt validation and draft storage tests.

### Task 4: Close P2 Resource And Concurrency Gaps

**Files:**
- Modify: `web/src/components/acp/ACPChatDialog.tsx`
- Modify: `web/src/components/conversation/ComposerDraftBox.tsx`
- Test: focused existing web tests under `web/tests/`

- [x] Add failing DOM tests for attachment tabs closing after save and send submission being blocked while draft storage is pending.
- [x] Lift draft-save pending into the composer operation boundary and route attachment release through preview-tab closure.
- [x] Mark unresolved restored attachments and workspace references unavailable and disable their preview/open actions.
- [x] Run focused tests and browser deep-link verification.

### Task 5: Consume Remaining API Contract Items

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/acp/composer_drafts.rs`
- Modify: `web/src/components/conversation/ComposerDraftBox.tsx`
- Modify: `web/src/locales/*.json`
- Modify: product design and development plan documents

- [x] Add tests for Direct-only role drafts, unsupported draft schema/version, and draft-specific empty errors.
- [x] Add destructive confirmation for meaningful draft deletion.
- [x] Synchronize seven locales and the two authoritative Gold Band documents.
- [x] Run focused web/Rust checks, production build, and format checks where available.
