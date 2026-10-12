---
name: git-changelog
description: Create and localize user-facing release notes from an explicit Git change range. Use when the user asks for a changelog, release notes, multilingual update notes, or GitHub Release copy. Require explicit start and end points, a target release version, and the user's normal or critical update choice; ask for missing inputs before proceeding.
---

# Git Changelog

## Confirm release inputs first

Require both a changelog start point and end point. Accept tags, commits, SHAs, branches, dates, or another unambiguous Git range.

Also require an explicit target release version and the user's choice of normal (`critical: false`) or critical (`critical: true`) update for that version. Reuse a clear answer already given for this target version; do not ask again unnecessarily. If any input is missing or ambiguous, ask the user and wait. Do not infer range boundaries or the target version from the latest tag, the previous release, the current branch, or `HEAD`. Never infer criticality from commits, commit labels, breaking changes, security fixes, or release-note wording.

Explain the choice when asking:

- Normal update: notify the user and wait for a manual update action; no automatic download or installation.
- Critical update: download the signed package in the background and install through the normal application exit lifecycle (or the existing subsequent-startup pending-install path). Do not force an immediate restart or interrupt active work merely because a critical update is discovered or finishes downloading.

Before overwriting any existing target-version file, read it. Inspect an existing `release-notes/<version>/release.json` and compare it with the user's explicit choice; the file alone is not user confirmation. Resolve conflicts or ambiguous instructions with the user rather than silently replacing the choice.

## Prepare the release notes

1. Read the Release PR, release-please `CHANGELOG.md`, merged PRs and commits in the range, and relevant product documents.
2. Draft the user-facing Simplified Chinese release notes. Use `release-notes/0.15.0/zh-CN.md` through `release-notes/0.17.0/zh-CN.md` as style references.
3. Keep the historical structure and tone: `## 功能新增/调整`, `## 体验优化`, numbered feature headings, short workflow steps, explicit limitations, and callouts when needed. Write GitHub-compatible Markdown, because GitHub Release and the in-app update dialog render it the same way:
   - Use only GitHub alerts: `> [!NOTE]` for supplementary information, `> [!TIP]` for usage advice, `> [!IMPORTANT]` for changes users must know, `> [!WARNING]` for limitations or risky operations, and `> [!CAUTION]` for irreversible actions or possible data loss. Do not use Obsidian callouts such as `> [!attention]`; they render as plain quotes. Put the `[!TYPE]` marker alone on the first line; alert titles are fixed by type and cannot be customized (no `> [!WARNING] Custom title`). When a callout needs a heading, make the next line bold, for example `> [!CAUTION]` / `> **放弃更改无法撤销**` / `> 正文…`.
   - A single newline is a line break; separate paragraphs with a blank line.
   - Write numbered lists as `1. item`, with a space after the number.
   - Do not use Obsidian image sizing such as `![image.png|500](…)`.
4. Include only verified user-visible changes. Do not invent details from commit titles alone. Remove duplicates and internal-only implementation details.
5. Use stable `https://static.dion.blue/...` image URLs. Check supplied images and links when network access is available; do not rewrite images to GitHub asset URLs.
6. Translate the confirmed Chinese source into `zh-TW`, `en`, `ja-JP`, `ko-KR`, `pt-BR`, and `es`. Preserve facts, Markdown structure, URLs, warnings, code, identifiers, and product terms.
7. Review every language against `zh-CN` for omissions, inconsistent facts, or fabricated content.

Write files to:

```text
release-notes/<version>/zh-CN.md
release-notes/<version>/zh-TW.md
release-notes/<version>/en.md
release-notes/<version>/ja-JP.md
release-notes/<version>/ko-KR.md
release-notes/<version>/pt-BR.md
release-notes/<version>/es.md
release-notes/<version>/release.json
```

Always write `release.json` alongside all seven locale files, including for a normal update. Its complete JSON object must be exactly `{"critical": false}` or `{"critical": true}`, matching the confirmed choice: one boolean field, no additional keys. Report the target version, final normal/critical choice, and files written in the completion summary.

## Release metadata and generator contract

`release-notes/<version>/release.json` is the version-scoped source of truth for default-channel criticality; the generated manifests are projections, not independent choices. The generator reads metadata from the release source SHA/tag, not an unrelated workflow branch or an uncommitted working tree. All seven locale files and `release.json` must be included in that release SHA/tag before generation and publication; merely creating the files locally is insufficient.

- Missing `release.json` means a normal historical release (`critical: false`); this historical default does not permit this skill to omit the file for a newly prepared release.
- A present file must parse as a JSON object containing exactly `critical` with a boolean value. Malformed JSON, missing fields, non-boolean values, arrays/null, or extra keys are invalid; fail before writing any generated output, rather than falling back to normal or leaving partial output.
- All eight default manifests (seven `latest.<locale>.json` plus `latest.json`) must reflect the same choice: include `critical: true` for a critical release; omit the field for a normal release. `latest.json` remains identical to the Simplified Chinese manifest.

GitHub Release uses the Chinese and English files. The `default` client channel uses all seven languages. Every app build, including `wb`, also embeds `release-notes/<package.json version>/<locale>.md` for Help → Release notes, so builds made before the notes exist ship without them. The `wb` channel remains a single `latest.json`; its existing build/critical controls are unchanged and do not consume this default-channel metadata. Keep `CHANGELOG.md` managed by release-please.

Do not automatically commit, push, tag, or publish a GitHub Release. The requirement to include these files in the release SHA/tag is not authorization to do so; each action requires a separate user request.
