---
name: git-issue
description: Prepare, review, create, or update evidence-based GitHub issues for the current repository with duplicate detection, repository-native templates, valid labels, and post-publication verification. Use when the user asks to draft, file, submit, publish, edit, or refine a GitHub issue, bug report, feature request, performance issue, or technical proposal. Always require the user to review and explicitly approve the exact final issue before any GitHub write.
---

# Git Issue

Create accurate GitHub issues through the official `gh` CLI. Treat repository files and GitHub state as authoritative; never hardcode an owner, repository, default branch, label, or template.

## Enforce the review gate

Never perform a GitHub write before completing all of these steps:

1. Prepare the exact final issue.
2. Show the user the repository, title, complete body, labels, assignees, milestone, and any linked issue.
3. Ask the user to approve that exact preview.
4. Wait for an explicit approval in a later user response.

Treat the initial request to create or submit an issue only as authorization to prepare the preview. Do not treat it as approval of content the user has not seen. Do not bypass the gate when the user says "submit directly."

Bind approval to the displayed revision. If any title, body, repository, label, assignee, milestone, or link changes after approval, show the revised complete preview and obtain approval again.

Before approval, do not run `gh issue create`, `gh issue edit`, `gh issue comment`, or any equivalent API mutation. Read-only discovery is allowed.

## Prepare the issue

1. Read the applicable repository instructions, including `AGENTS.md` and routed rule files.
2. Resolve the repository from the current Git remote, then confirm it with `gh repo view`.
3. Verify `gh` authentication, repository access, and that Issues are enabled.
4. Read `.github/ISSUE_TEMPLATE/config.yml` and every applicable template in `.github/ISSUE_TEMPLATE/`. Use these files as the content-schema source of truth.
5. Search open and closed issues for the same symptom, goal, error, or affected component. Limit broad searches and refine them with distinctive terms.
6. If a likely duplicate exists, show it to the user and ask whether to stop, add new evidence to it, or prepare a distinct issue. Do not mutate the existing issue without a separate reviewed preview.
7. Classify the issue and select the closest repository template:
   - Bug report for observable incorrect behavior.
   - Feature request for new user-facing value.
   - Performance issue for measured latency, resource, throughput, or scale problems.
   - Technical proposal for architecture, data integrity, lifecycle, migration, or maintainability work without a primary user-facing feature.
8. Fetch current labels and use only labels that already exist. Do not create labels, milestones, projects, or assignees unless the user explicitly requests them and reviews the final metadata.

## Write evidence faithfully

Use English for the published title, template headings, and prose by default. Translate a Chinese request into natural technical English while preserving code, commands, logs, identifiers, and quoted errors verbatim. Use Chinese only when the user explicitly requests a Chinese issue.

Follow the selected template instead of duplicating a private format in this skill. Apply these evidence rules:

- State observed behavior, expected behavior, reproduction steps, environment, and impact for bugs.
- Trace original design intent when diagnosing a bug. Mark root cause as `Verified`, `Hypothesis`, or `Unknown`; never present an inference as fact.
- Record the problem, user value, goals, non-goals, existing solutions, and acceptance criteria for features.
- Record data size, baseline, target, measurement method, and correctness constraints for performance issues.
- Explain the missing invariant, authoritative source, alternatives, migration, and deletion of replaced paths for technical proposals.
- Include only claims supported by repository evidence or clearly attribute them to the reporter.
- Remove secrets, credentials, personal data, private URLs, and unnecessary absolute local paths.
- Use `Closes #N` only when the proposed outcome fully resolves the linked issue; otherwise use `Refs #N`.

## Publish and verify

After approval:

1. Reconfirm that the approved preview still matches the pending title, body, and metadata.
2. Create or update the issue with `gh` using a body file or other quoting-safe input for multiline Markdown.
3. Read the result back with `gh issue view` and verify the repository, number, title, body, labels, state, and URL.
4. Report the URL and any metadata GitHub rejected or normalized.
5. Do not perform follow-up edits or comments without another complete preview and explicit approval.

If authentication, permissions, repository identity, template selection, duplicate handling, or requested metadata remains ambiguous, stop before publishing and ask the user.
