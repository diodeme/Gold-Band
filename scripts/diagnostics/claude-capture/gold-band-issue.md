### Actual behavior

Claude ACP workflow nodes can finalize before the agent intends to stop, or remain stuck in `finalizing` after emitting an Artifact.

Two distinct incidents were observed: an intentional pause awaiting a background command was followed by forced finalization; separately, an incomplete streamed tool call was reported as SDK success and converted to ACP `end_turn`.

### Expected behavior

At a completed prompt boundary, ask whether the agent intends to finish the current node. If so, collect the existing Artifact; otherwise allow execution to continue. Do not require a new review of business goals or acceptance criteria. A pending prompt must not be considered complete merely because JSON appeared in the stream.

### Steps to reproduce

Observed sequence, not yet a deterministic reproduction:

1. Run a long Claude ACP workflow node using background commands or Monitor.
2. Receive a business `session/prompt` response with `stopReason: end_turn` while work remains.
3. Gold Band immediately sends its hidden finalize prompt, which currently prohibits further business work.
4. When a background notification starts an autonomous cycle around this boundary, the new finalize prompt can be processed within that cycle. Its JSON is delivered, but the SDK result is attributed to `task-notification` and the ACP finalize request remains unresolved.

### Environment

Windows; Gold Band desktop (exact installed application version not recorded here); Claude ACP 0.75.1; Agent SDK 0.3.257; bundled Claude CLI 2.1.257. The configured model was a third-party `gpt-5.6-sol[1M]` endpoint, not a native Claude model. Evidence captured on 2026-09-06.

### Additional context or evidence

Sanitized timeline, UTC+08:00:

- 21:29:26: an Edit tool block began; parameter deltas continued without a captured closing tool block or final message stop.
- 21:35:30.055: SDK reported `success`, `stop_reason: tool_use`, `origin: human`; ACP returned `end_turn` for business request 25.
- 21:35:30.059: a background build-failure notification entered the native transcript.
- 21:35:30.600: Gold Band sent finalize request 30.
- 21:41:11: the agent emitted Artifact JSON; SDK reported `success/end_turn` with `origin: task-notification`, followed by idle. No ACP response for request 30 appeared; it remained pending through 21:57:20.

Verified: the current finalize prompt forces wrap-up and prohibits business continuation. Installed adapter code classifies `task-notification` results as autonomous and skips ordinary prompt settlement, except when settling a previously held prompt. This matches the unresolved request in the capture.

Unknown: why the earlier tool stream ended incompletely while SDK reported success. There is no independent HTTP capture establishing an upstream, network, or SDK root cause. No session/cancel was observed for the affected session.

Temporary mitigation, not yet implemented:

- Force `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` when Gold Band launches managed Claude ACP instances, and merge `Monitor` into SDK `disallowedTools` when creating sessions. Cover existing and new instances without rewriting saved user configuration or adding UI notices. Preserve unrelated options and leave other agents unaffected.
- Change the Chinese and English finalize prompts to ask about intended completion: emit the existing Artifact if finished, otherwise continue directly. Prefer a prompt-only change for this behavior; adjust runtime handling only if tests demonstrate it is necessary.
- Add no waiting tags, countdowns, background polling, or automatic cancellation based on JSON output. Preserve cancellation, execution budgets, failure handling, and Artifact validation.
- Validate in fresh isolated sessions that background execution and Monitor are unavailable and synchronous long commands still work. Add focused regression tests and update product design and development-plan documentation.

This mitigates background-cycle interleaving; it does not claim to fix the underlying incomplete-stream or SDK/adapter attribution defects. Existing background work is not retroactively stopped.

### Submission checklist

- [x] I searched open and closed issues for duplicates.
- [x] I removed secrets, personal data, and unnecessary private paths.
