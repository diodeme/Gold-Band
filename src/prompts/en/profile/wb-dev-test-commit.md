## WB Development and Testing Auto-Commit

This section applies only to the WB channel. After completing the requirement implementation and required automated tests, perform this closing step before ending the node.

1. Use `memory_read` to inspect task-scope `storyId` and `storyName`. Reuse both when they are nonempty. When either is missing, empty, or unpaired, first generate the identity with `storyId=0` and the requirement-name extraction rules and try `memory_write`. Unavailable memory tools or a failed write are not blockers: ask the user for the requirement identity or continue with the identity confirmed in this run, and state that it was not persisted.
2. Identify task-related changes produced by this node, including code, tests, prompts, product-design documents, and the development plan. Exclude changes that existed before execution or are clearly unrelated user changes.
3. If there are no task-related changes, do not create an empty commit. Record that no commit was required, the evidence, and any retained uncommitted changes that were not included.
4. Select a standard Conventional Commits type token and a concise Chinese description from the actual task-related diff. Focus on the delivered result, not execution history, model names, or generic statements.
5. Run `git add` only with specific paths. Never use `git add -A` and never include unrelated changes. Create one logical commit after a successful node execution.
6. The commit message must contain exactly three lines:

```text
--story=[{storyId}] {storyName}
{type}: {中文描述}
#AI COMMIT#
```

7. After committing, verify the commit OID, full commit message, and task-related path state. Never claim node completion when the commit or verification failed.
8. Record the commit OID, three-line message, included paths, excluded paths, and reasons in the development-and-testing report. CICD uses these commit OIDs to determine whether this task has unpushed commits.
9. Do not ask the user to confirm the commit message or format. Existing ACP command-permission rules remain in force; never bypass permission boundaries for auto-commit.
10. If committing fails, retain the original error and current Git state for the existing node-failure, manual-recovery, or acceptance-failure path. Do not create a replacement commit or rewrite commit history.
