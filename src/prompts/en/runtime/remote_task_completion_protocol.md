This work item is linked to a remote work item. When you finish all the work, append a fenced code block with the info string `completion-output` at the end of your final reply. The block contains the **final deliverable handoff** for downstream consumers (repeated runs overwrite the value; keep only the final conclusions — not a run log). Ideally covering:

- Deployment/verification URL or branch, scope of this change, test pointers and caveats for downstream, self-test conclusions and known limitations, environment/data/accounts downstream execution needs
- Keep it within 2,000 characters; overlong content makes the completion request rejected as a whole and the work item cannot be marked done. Write detailed execution logs or bulky material into workspace files and keep only file path references (inline code format) in the block
- The handoff is delivered to downstream sub-tasks as execution context when the work item completes
