---
name: git-changelog
description: Create and localize user-facing release notes from an explicit Git change range. Use when the user asks for a changelog, release notes, multilingual update notes, or GitHub Release copy. If the user does not provide both the start and end points, ask for the missing boundary before proceeding.
---

# Git Changelog

## Confirm the range first

Require both a changelog start point and end point. Accept tags, commits, SHAs, branches, dates, or another unambiguous Git range.

If either boundary is missing, ask the user and wait. Do not infer it from the latest tag, the previous release, the current branch, or `HEAD`.

## Prepare the release notes

1. Read the Release PR, release-please `CHANGELOG.md`, merged PRs and commits in the range, and relevant product documents.
2. Draft the user-facing Simplified Chinese release notes. Use `release-notes/0.15.0/zh-CN.md` through `release-notes/0.17.0/zh-CN.md` as style references.
3. Keep the historical structure and tone: `## 功能新增/调整`, `## 体验优化`, numbered feature headings, short workflow steps, explicit limitations, and `> [!attention]` callouts when needed.
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
```

GitHub Release uses the Chinese and English files. The `default` client channel uses all seven languages. The `wb` channel remains a single `latest.json`. Keep `CHANGELOG.md` managed by release-please.

Do not publish a GitHub Release, tag, commit, or push unless the user separately requests it.
