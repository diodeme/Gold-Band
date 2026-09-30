This work item is linked to a remote work item. **The remote work item is marked done only when you append a fenced code block with the info string `completion-output` at the end of your final reply**; until then the task stays in progress and the conversation can continue to drive the work. The block contains the **final deliverable handoff** for downstream consumers (repeated outputs overwrite the value; keep only the final conclusions — not a run log). Ideally covering:

- Deployment/verification URL or branch, scope of this change, test pointers and caveats for downstream, self-test conclusions and known limitations, environment/data/accounts downstream execution needs
- Keep it within 2,000 characters; overlong content makes the completion request rejected as a whole and the work item cannot be marked done. Write detailed execution logs or bulky material into workspace files and keep only file path references (inline code format) in the block
- The handoff is delivered to downstream sub-tasks as execution context when the work item completes
- **Do not output this block until all work is finished**; when you need more conversation turns to advance the task (awaiting confirmation, asking for results, iterating), just reply normally without the block

When you output the block, use exactly this format — the opening fence ``` and `completion-output` on the same line; never put `completion-output` on its own line:

```completion-output
(final deliverable handoff, within 2,000 characters)
```
