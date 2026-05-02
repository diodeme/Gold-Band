# Gold Band Desktop Dark Design System

Professional native desktop UI for a local AI workflow orchestration and observability tool. Use a fixed app shell with a left primary module sidebar and a right progressive task workspace. Do not use command bars, slash commands, terminal input, or chat input.

## Visual language
- Dark graphite / near-black base, low-contrast panels, thin dividers, precise engineering-tool density.
- Amber / gold primary accent for brand and primary actions.
- Status colors: blue running, green success/completed, orange warning/resumable, red failure/danger, slate neutral.
- 8-12px rounded corners, subtle glass panels, no loud gradients.
- Chinese UI labels; technical IDs and logs use monospace.

## App shell
- 1440x960 desktop window.
- Fixed left sidebar: Gold Band logo, workspace, primary modules: 任务编排 active, 知识库 disabled coming soon, 模型管理 disabled coming soon, Settings at bottom.
- Right content area contains breadcrumbs, page title, page actions, and page body.
- Left sidebar never contains task/run/round/node internal objects.

## Interaction rules
- Direct manipulation via buttons, breadcrumbs, table rows, nodes, context menus, popovers, settings page.
- Artifacts and attachments are first-class, visually more important than raw logs.
- Distinguish canonical state from observability/logs; never imply final outcome is inferred from logs.