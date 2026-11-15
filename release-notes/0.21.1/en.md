## New and Updated Features

### 1. Guided first launch and local Agent discovery

New installations and users who have not completed initialization now get a three-step setup flow:

1. Check Git. You can skip it for now and set it up later when a Git-dependent operation is needed.
2. Check Node.js, npm, and npx. All three must pass before continuing.
3. Discover and diagnose available local Agents. Successfully diagnosed Agents are added to “Agent Management”; choose a default Agent, model, and permission mode, preview the changes, and apply them to all built-in Workflows.

Users who have already completed initialization can rerun the flow from “Settings → General → Setup and Workflows.” The “Configure defaults” action updates built-in Workflows in bulk and does not overwrite custom Workflows.

> [!NOTE]
> The flow does not install Git, Node.js, or Agents automatically. When a tool is missing, it provides the official installation guidance; install or sign in, then run detection again. An Agent is listed as selectable only after it passes the current diagnostic run.

### 2. Composer drafts and send queue

You can keep typing while a Direct session is running and place messages in a send queue for later delivery. The queue holds up to 10 items and supports automatic or manual sending, pause and resume, reordering, editing, deletion, and send now. Unsent items remain after stopping a session or restarting the client.

The new draft inbox stores up to 20 drafts. Drafts retain their text, file references, role, and attachment references until you choose to restore and send them. A failed save does not clear the text you are editing, and attachments are not lost when you stop a session or change pages.

### 3. Multica requirements and result handoff

The internal edition’s “Requirements” page now provides a more complete remote-task workflow:

1. Filter by development or test task and search by task name. Test tasks whose development task is complete show “Ready” and move ahead in the queue; tasks that require a DPMS requirement show a “DPMS” marker.
2. Designated Workflow nodes can send interim delivery snapshots so downstream tasks can retrieve guidance before the upstream task finishes; the final node summarizes the whole work item. After completion, you can continue in the original conversation to revise the delivery notes.
3. When upstream delivery notes are not ready, an in-conversation card lets you keep waiting, provide the information manually, or skip this pull.
4. Completed or failed tasks can be rerun. The original row moves back to the queue and the successor is marked “Rerun.”
5. When a role or Workflow is shared from the Multica web app, the client can receive it in the background and refresh the corresponding list without a second desktop import.

### 4. Role management and the built-in WeAPM role

“Context Management → Roles” can now import one or more Markdown files directly, as well as import a folder in bulk. Each batch processes up to 5,000 files and keeps a per-file success or failure reason. Custom roles are now shown in one complete list instead of separate pages.

The WB channel adds a read-only built-in WeAPM role for querying Trace and Log data with the WeAPM CLI to investigate slow calls, errors, timeouts, and exception stacks.

> [!NOTE]
> The WeAPM role is available only in the WB channel and requires the internal WeAPM service, network access, and the `weapm` CLI. Its queries are read-only.

### 5. IM remote intervention and Feishu authorization

IM integrations are now organized as independently collapsible channel cards, so connection status, authorization, binding, and notification preferences can be managed separately. Feishu authorization can select an existing app or create a new one, and you can switch apps later. Remote-intervention answers now remain consistent across retries, reconnects, and late callbacks; a callback that can no longer be applied is shown as a follow-up message.

> [!WARNING]
> **Changing the Feishu app clears the current private-chat binding**
> Replacing the app replaces the authorization and removes the existing private-chat binding. After authorizing the new app, send the bot another private message from the corresponding account. Upgraded IM data cannot be read directly by older clients; back up application data before upgrading if you need downgrade capability.

### 6. Workflow canvas and desktop display settings

“Settings → Appearance” now includes “Interface zoom,” with whole-client scaling from `50%–250%`. You can also use `Ctrl+wheel`, `Ctrl+plus/minus`, and `Ctrl+0` to adjust or reset it.

Authoring and runtime Workflow graphs share an “Auto / Horizontal / Vertical” layout preference. Both the runtime graph and editor support a focused fullscreen view that keeps the current draft and inspector; press `Esc` to exit.

## Experience Improvements

### 1. Find and recover conversations more easily

The sidebar now includes a cross-workspace “Recent conversations” section sorted by activity. Clicking a row opens the conversation directly; selecting a search result that is not loaded yet also creates the correct sidebar row and reveals it.

Direct startup continues waiting for ACP to become ready instead of reporting a failure while the Agent is still starting. Confirmed terminal errors remain visible. Background refreshes, pagination, and workspace changes no longer let late responses overwrite conversations already on screen.

### 2. More reliable files, attachments, and input state

Workspace files, run artifacts, conversation attachments, and Git file tabs share actions such as copy path, reveal in the file manager, and reference to the conversation while preserving each file’s original source. Attachment references in drafts and queues can also be restored across pages. Opening a conversation from search keeps the sidebar expanded and focused on the target.

Long-text input, IM conversation scrolling, and right-workspace transitions are more stable. Errors in Direct startup, queue dispatch, attachment previews, and local file references stay within their owning surface instead of becoming a whole-conversation or whole-workspace failure.

### 3. Other stability improvements

1. Workflow graphs refit to the correct canvas when switching direction or entering and leaving fullscreen, reducing overlap between edges, nodes, and the sidebar.
2. Recent-conversation and task lists use bounded pagination and progressive loading instead of reading all history at once.
3. Direct-session recovery, pending-intervention settlement, and remote-approval synchronization are more consistent, reducing resurfaced requests and stale task statuses.

---

## 0.21.1: Small improvements since 0.21.0

### 1. Defer first-run setup

Once the environment checks pass, you can choose “Set up later” after Agent detection completes or fails, enter the app, and finish Agent configuration later in Settings. You can proceed even if no Agent is available. This choice and any earlier decision to skip Git are preserved after a restart.

### 2. More stable sidebar navigation and recent conversations

Alignment and selection states are more consistent across fixed navigation, workspaces, recent conversations, and run history. Recent conversations support the same expansion and row actions as workspace lists, and refreshes preserve the selection in the current section. Lists load page by page through “Show more”; collapsing and reopening workspace or recent-conversation lists restores the first batch of items.

### 3. Updated website documentation and download entry points

The website and Demo pages now use the MALING brand, with a knowledge-base documentation reader and a platform-specific download menu. Document switching, narrow-screen layout, and download entry points are more stable.
