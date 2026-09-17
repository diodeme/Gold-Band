## Requirement Identity Precheck

Run this section before topology enumeration, Interview Round 0, or Grill branch enumeration. At the start of this section, first call `memory_read`, then perform the identity check.

1. Use `memory_read` to read the current task and workspace memory. Inspect only the `task` entries in the returned snapshot; a workspace `storyId` or `storyName` does not count as an existing current-task identity.
2. Continue the original role workflow only when the task contains both `storyId` and `storyName` and both values are nonempty after trimming.
3. If either key is missing, empty, or not paired, first ask exactly one question about the requirement identity. The required semantic choices are:
   - `不存在`
   - `其他（用户自行输入）`
4. Use `AskUserQuestion` or an equivalent elicitation tool when available. Otherwise output a plain-text question, but preserve the same two semantic choices. This question is not a topology step, ambiguity round, Interview round, or Grill branch. Continue the original workflow after the user answers.
5. When the user chooses `不存在`:
   - Set `storyId` to the string `0`.
   - Extract a short `storyName` from the requirement: prefer an explicit title or first-line summary, then the core noun phrase in the goal, and use `系统需求` only if extraction remains impossible.
   - Remove template labels, status notes, Markdown markers, and irrelevant punctuation. Save one line with at most 40 Unicode characters.
6. When the user chooses `其他（用户自行输入）`, obtain both the requirement ID and requirement name. If free text cannot be clearly split into the two values, ask once more; if it remains unclear, wait for clarification instead of guessing. Trim both values and store each as one line.
7. Use `memory_write` to write both entries to task scope. Before writing, use the target-key revisions from the latest `memory_read`: pass `expectedRevision = null` when the key is absent, and the matching revision when correcting an existing key. Never write these keys to workspace scope.
8. After both writes, call `memory_read` again and confirm that task-scope `storyId` and `storyName` exactly match the user-confirmed values.
9. If `gold-band-memory` is unavailable or the read fails, state that the requirement identity was not read and ask normally according to step 3. If a write or post-write verification fails, state that the requirement identity was not persisted. Neither case is a blocker. Continue with the user-confirmed or newly supplied `storyId` and `storyName`; a later node may ask again.
10. Never edit memory files directly. After obtaining the identity, continue with Interview initialization or Grill decision-tree enumeration; keep the task-scope copy when the tool write succeeds.
