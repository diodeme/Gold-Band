## New and Updated Features

### 1.Direct sessions keep receiving background messages and can stay running

When an Agent keeps working after a reply ends (for example, when a scheduled task fires or a sub-Agent runs in the background), a Direct session now keeps receiving its replies, tool calls, permission requests and questions instead of losing them once the reply ends.

1.You are notified once when a new background reply arrives; later content of the same reply does not notify again.
2.While background work is running, the session icon in the sidebar pulses and a spinner replaces the time; inside the session you can see what the Agent is thinking about or which tool it is calling.
3.While there is background activity, the composer shows a stop button so you can stop the background work; sending new messages is not affected.
4.Files changed by background tools are counted in the most recent turn's file changes.

Right-click a Direct session and choose "Keep session running" to show a pin next to its title; right-click again and choose "Allow session eviction" to undo it.

All live Direct sessions are kept while there are 8 or fewer. Above 8, sessions idle for more than 6 hours are reclaimed; above 20, the sessions that have been inactive the longest are reclaimed early. Sessions kept running, executing, or waiting for you to answer a permission request or question are never reclaimed.

> [!WARNING]
> Keeping a session running only applies while the app is open; kept sessions are not started automatically after the app restarts. Scheduled tasks in a session that is not kept running may be interrupted when the session is reclaimed. Stopping background work only sends a cancel request to the Agent; whether its scheduled tasks are removed is up to the Agent.

### 2.Mermaid diagrams and GitHub alerts in Markdown

` ```mermaid ` code blocks in conversations and file previews now render as diagrams whose colors follow the current theme. Click a diagram to open it in the right workspace, where you can zoom, pan, and copy or save it as PNG.

Conversations and file previews also support GitHub's five alerts: `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` and `[!CAUTION]`.

### 3.Web images load only from trusted domains

To prevent documents or Agent replies from making the client send requests to arbitrary servers, web images in Markdown now load automatically only from trusted domains.

1.In conversations: untrusted images show a placeholder; click "Load" to trust the domain and display the image.
2.In file previews: untrusted images stay as links, and a banner at the top lets you trust every domain in the document at once.
3.View and remove trusted domains in "Settings → Advanced → Trusted image domains".

### 4.View context compaction summaries

When an Agent finishes compacting context and returns a text summary, the compaction entry shows "View summary". Click it to open the summary in the right workspace. It renders as Markdown by default, and you can switch to source and copy it.

### 5.Fetch configuration per model

When you pick a model that has not been used yet in a workflow node, AUTO settings, the home composer or a session, "Configuration for this model has not been fetched" and a "Fetch model configuration" button appear. Clicking it starts the Agent once with that model to read its actual options; no message is sent and no task is created.

You can keep editing, saving and starting tasks without clicking it. Existing model settings are no longer trimmed using another model's configuration, and a failed background Agent diagnosis no longer clears model configurations that were already fetched.

### 6.Release notes render like GitHub and are available from the Help menu

Notes in the update dialog now render the same way as GitHub Releases, including line breaks and alerts. The new "Help → Release notes" lets you read the current version's notes at any time.

### 7.Redesigned workflow graph layout

The workflow editor and run graph use a new automatic layout with clearer node and edge placement. Groups created dynamically by AI-DYNAMIC are shown as nested frames, and a loading indicator appears when layout takes longer.


## Experience Improvements

### 1.AUTO delivery must pass acceptance

When AUTO finishes development, the work must go to an acceptance node that checks it against the original requirement. If acceptance fails, AUTO schedules fixes and runs acceptance again; it ends only after acceptance passes. Acceptance uses the original requirement as its only basis: a plan that excludes requirement content is treated as a blocker, and modified existing tests must be re-verified against their original versions.

Long summaries and task descriptions submitted by Agents are now handed off through files, reducing retries caused by output format errors. When AUTO pauses, the banner shows the specific error title.

### 2.Claude and Codex integration upgrade

Claude ACP is upgraded to `0.87.0` and Codex ACP to `2.1.1`.

1.Sessions show context compaction status, duration and usage before compaction.
2.Output from commands run by Codex keeps accumulating in the tool details, up to 256,000 characters.
3.Custom answers to Claude's questions can be submitted correctly.
4.When an Agent needs you to sign in again, you get a clear prompt; sign in and then resend manually.

### 3.More stable sessions

1.Re-entering a session no longer repeatedly shows a loading state or briefly shows stale content.
2.Execution errors such as a full disk or missing write permission are shown immediately, and the composer no longer stays waiting; tools without a returned result show "Result unconfirmed", and existing replies and drafts are kept.
3.After you stop a reply, the session always ends the current turn correctly instead of staying on "Stopping".

### 4.More accurate file change records

1.Fixed a whole turn's changes being marked incomplete when Agents such as Claude edit the same file several times in fragments.
2.Fixed files created by Cursor being recorded as modifications with Diff markers mixed into their content.
3.File change rows consistently show the file name, directory and added/removed lines; files edited multiple times also show the edit count.

### 5.Other improvements

1.Unified image preview: both file images and session images zoom directly with the mouse wheel, fit the window when first opened, and offer a "100%" button for the original size.
2.When the built-in browser opens a local HTML file, resources in parent directories now load correctly; when a page accesses a directory outside the granted scope, you can choose "Allow access" in the notice.
3.Headings in conversations have a clearer hierarchy and keep their spacing while streaming; "Release notes" moved to the end of the Help menu.
4.Fixed installing or updating a client from one channel closing a running client from another channel when both are installed.
5.Updated the ACP Registry and Agent catalog.
