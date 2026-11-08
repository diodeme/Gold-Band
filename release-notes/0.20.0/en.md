## New and Updated Features

### 1.Remote intervention through Feishu / Lark

In addition to WeCom, you can now handle permission requests, follow-up questions and manual checks that block a task from Feishu / Lark, and receive Run and turn-finished notifications there.

1. Open "Settings → General → IM remote intervention and notifications", expand "Feishu / Lark", and click "Scan to connect".
2. Review the required permissions, click "Agree and generate QR code", and scan the code with Feishu / Lark on your phone. You can pick an existing Feishu / Lark app or create a new one.
3. After authorization, send the bot a message in Feishu / Lark to bind the one-to-one private chat.
4. When a task needs you, Feishu / Lark receives an interactive card showing the ID, node, task and workspace. You can allow or deny permissions, answer follow-up questions (single choice, multiple choice, custom answers and text), and mark manual checks as passed or failed.

After you submit, the card keeps your answers and disables its controls. For requests already handled on the desktop or in WeCom, the card shows where they were handled. Requests that ended because of a stop, a timeout or similar reasons show "Cancelled" and are never shown as declined. If a submission is processed slowly and is not accepted in the end, the bot sends a separate message explaining why.

Under "Configuration" you can rebind the receiving account, switch the Feishu / Lark app, reauthorize, or delete the configuration.

> [!NOTE]
> A bot connects to only one device. Only one-to-one private chats are supported, and a Feishu / Lark card holds at most 5 follow-up questions; handle larger requests on the desktop. The permission to read your name is optional; without it, the receiving account is shown as "Private chat bound".

> [!WARNING]
> After the upgrade, IM data is migrated to a new format that older clients can no longer read; downgrading requires restoring a backup first. WeCom cards sent before the upgrade and still pending become invalid; handle those requests on the desktop.

### 2.Send now and Auto send for pending messages

You now have more control over when messages queued during an Agent reply are sent.

1. "Send now" sends a pending message right away. If a reply is in progress, you are asked first whether to stop it; after checking "Don't ask again", you can turn the confirmation back on in "Settings → Advanced → Pending messages".
2. "Auto send" toggle: when on, pending messages are sent one by one after each reply ends; when off, they stay in the queue until you send each one with "Send now".
3. When the session is stopped or the previous send failed, the queue shows that it is paused and why; click "Resume" to continue sending automatically.
4. Clear the pending queue from "More actions".

### 3.Drafts

The composer toolbar now has "Drafts". Click "Save to drafts" to set aside what you have typed, and "Move to input" later to continue editing. Drafts are kept per session, are never sent automatically, and each session keeps up to 20.

### 4.Native Claude sub-agents

With Claude ACP `0.90.0` or later, Gold Band reads the sub-agent states reported by Claude and accurately shows running, waiting for interaction, completed, interrupted and failed. Sub-agents are no longer shown as interrupted just because the main turn ended.

1. While sub-agents are running, a summary of them appears above the composer; click one to view its full transcript in the right workspace.
2. After the main turn ends, the composer keeps the "Stop" button as long as a sub-agent is still running in the background or waiting for a permission.
3. Once a session has background work, the Token usage tooltip notes "Excludes usage from background wake-ups".

> [!IMPORTANT]
> Claude ACP has been upgraded to `0.90.0`. If your version is too old, you are prompted to upgrade; Gold Band does not change your installation settings automatically.

### 5.Confirm the target before pushing

When you push a branch in Source Control, the source branch, target remote and target branch are shown first, and the push happens only after you confirm. For a branch published for the first time, you can choose the remote and set the target branch as upstream after a successful publish. Branches without an upstream are marked `new`.

### 6.Open conversation search with Ctrl+K

Press `Ctrl+K` (`Command+K` on macOS) to open conversation search; pressing it again while open selects the keyword, and `Esc` closes it. `Ctrl+F` is left to in-file search in editors.


## Experience Improvements

### 1.IM settings and cards

1. "IM remote intervention and notifications" now shows WeCom and Feishu / Lark as two collapsible cards, with notification types grouped into "Needs your action" and "Execution results".
2. WeCom and Feishu / Lark cards follow the desktop language and support all seven interface languages.
3. Completed WeCom cards show every question and custom answer. WeCom multi-question single-choice cards hold at most 3 questions; handle larger requests on the desktop.

### 2.More reliable sessions

1. When an Agent fails during startup, automatic retries now run again correctly; turns that fail after the message has already reached the Agent switch to manual recovery to avoid running twice. During startup, the composer shows "Preparing agent".
2. When a session fails to be created, the reason is shown directly instead of loading indefinitely.
3. Questions and permission requests left behind after the client exits unexpectedly become invalid; answering them now shows a clear message instead of reporting success.
4. Continuing a session keeps its original model and permission mode.
5. Fixed Token usage being counted twice when continuing a Direct session after it was reclaimed.

### 3.Smoother scrolling

1. The message list stays pinned to the bottom when the composer grows taller.
2. While reading history, new content no longer pulls you back to the bottom; scrolling up at the top loads earlier history, even when the content is shorter than one screen.
3. The composer's scroll position no longer jumps while typing long drafts.

### 4.Other improvements

1. The composer's `/` and `@` menus reload skills and roles each time they open, so new skills are available without restarting.
2. Attachments with the same name no longer overwrite earlier attachments in a conversation; when a session's data directory is nested too deeply to save an attachment, a clear message is shown.
3. Local images in conversations are resolved against the session's workspace; the built-in browser keeps query parameters and anchors when opening local HTML links.
4. Markdown live preview fills the panel width; the default editor font size is now 13px.
5. When a tab in the right workspace fails, only that tab shows "This tab couldn't be displayed"; other tabs are unaffected.
6. MCP servers launched through `npx` or `uvx` now wait up to 180 seconds for dependencies to download on their first check, instead of failing while still downloading.
7. When the system language is Chinese, the interface language is chosen by the Simplified or Traditional script tag and is no longer affected by region.
8. Fixed tooltips popping up on their own after clicking dialogs or menus.
9. Updated the ACP Registry and Agent catalog.
