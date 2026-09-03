---
name: git-pr
description: Prepare, review, publish, or update a complete GitHub pull request for the current repository, including local commit readiness, safe push strategy, semantic title validation, repository-native PR templates, issue linking, and checks verification. Use when the user asks to draft, create, submit, publish, open, or update a pull request. Always require the user to review and explicitly approve the exact final PR before any push or GitHub write.
---

# Git PR

Publish a coherent pull request through Git and the official `gh` CLI. Reuse the existing `git-commit` skill for staging and commit boundaries instead of reproducing its rules here.

## Enforce the review gate

Never push or mutate a pull request before completing all of these steps:

1. Prepare the exact final pull request.
2. Show the user the repository, base branch, head branch, draft status, title, complete body, labels, reviewers, assignees, milestone, linked issues, local commits, and planned push remote.
3. Ask the user to approve that exact preview.
4. Wait for an explicit approval in a later user response.

Treat the initial request to create or submit a PR only as authorization to prepare the preview. Do not treat it as approval of content or metadata the user has not seen. Do not bypass the gate when the user says "submit directly."

Bind approval to the displayed revision. If the diff, commit set, title, body, base, head, draft status, remote, labels, reviewers, assignees, milestone, or linked issues change after approval, show the revised complete preview and obtain approval again.

Before approval, do not run `git push`, `gh pr create`, `gh pr edit`, `gh pr ready`, `gh pr comment`, or any equivalent API mutation. Local read-only inspection and local commits requested through `git-commit` are allowed. Tell the user if local commits are created while preparing the PR.

## Prepare the branch and evidence

1. Read the applicable repository instructions, including `AGENTS.md` and routed rule files.
2. Resolve the repository, default branch, current branch, remotes, and viewer permission dynamically.
3. Inspect worktree status, staged changes, commits, and `base...HEAD`. Never publish directly from the default branch.
4. If required changes are uncommitted, invoke or follow `git-commit` to create coherent local commits. Never include unrelated user changes or secrets.
5. Confirm that code changes have synchronized both required project design and development-plan documentation.
6. Use fresh verification evidence for the final commit set. Never claim a test, build, benchmark, or visual check that did not complete successfully against the reviewed revision.
7. Read `.github/PULL_REQUEST_TEMPLATE.md`, pull-request templates, semantic-title workflows, repository rules, and current checks. Treat them as authoritative and do not hardcode `main` or a status-check name.
8. Check whether the head branch already has an open PR. Prepare an update to that PR instead of creating a duplicate.
9. Identify related issues. Use `Closes #N` only when merge fully resolves the issue; otherwise use `Refs #N`.

## Draft the PR

Use English for the published title, template headings, and prose by default. Translate a Chinese request into natural technical English while preserving code, commands, logs, identifiers, and quoted errors verbatim. Use Chinese only when the user explicitly requests a Chinese PR.

Follow the repository PR template. Remove inapplicable optional sections instead of filling the body with empty headings or `N/A`. Ensure the body communicates:

- The outcome and scope.
- Root cause or design rationale when relevant.
- The concrete changes and removal of replaced paths.
- Verification commands and truthful results.
- Product-design and development-plan documentation updates.
- Performance impact and overdesign review conclusions when meaningful.
- Known risks, limitations, migrations, or follow-up work.

Generate a title that satisfies the repository's current semantic-title rules. Prefer Conventional Commits form:

```text
<type>[optional scope]: <imperative description>
```

Do not use vague titles such as `update`, `fix bug`, or `[codex] ...` when semantic titles are enforced.

## Choose a safe push strategy

- Prefer a normal push to an existing writable remote.
- If origin is not writable, detect an existing fork before proposing creation of a new fork.
- Include the selected remote and head branch in the review preview.
- Never use bare `--force`.
- Do not rewrite published history without explicit authorization for that operation. When authorized, use only `--force-with-lease` and obtain a new PR preview approval because the commit set changed.
- Do not merge, enable auto-merge, delete branches, or mark a draft ready unless the user separately requests and reviews that action.

## Publish and verify

After approval:

1. Reconfirm that the approved preview matches the current diff, commit set, branches, remote, title, body, and metadata.
2. Push the reviewed head branch.
3. Create the PR or update the existing PR with `gh`, using a body file or other quoting-safe multiline input.
4. Read it back with `gh pr view` and verify title, body, base, head, draft state, metadata, URL, and check rollup.
5. Watch the repository checks at least through semantic-title validation. Report every pending, successful, failed, cancelled, or skipped check truthfully.
6. If only the title check fails, prepare a corrected complete PR preview and obtain approval again before editing the title.
7. Do not repair unrelated CI failures, push follow-up commits, edit the PR, or comment without the user's authorization and a new reviewed preview when published content or commits will change.

If authentication, permissions, base/head identity, branch ownership, dirty-worktree scope, validation status, or requested metadata remains ambiguous, stop before publishing and ask the user.
