## New and Updated Features

### 1.Manage files directly in the file workspace

You can now create files and folders, rename entries, and delete them directly from the file tree in the right workspace without switching to the system file manager.

Create and rename actions use inline editing. Deletion asks for confirmation and moves the content to the system Recycle Bin or Trash. When the file tree has focus, press `Ctrl+Z` (`Command+Z` on macOS) to undo the most recent create, rename, or delete action.

![image.png](https://static.dion.blue/2026/09/20260930162214313.png)

### 2.Quote files and Diffs directly into a conversation

The `@` menu in the composer can now browse and search files in the current workspace. Selecting a file adds a file reference to the message for the current Agent; downstream nodes in Workflow and AUTO also receive these references.

![image.png](https://static.dion.blue/2026/09/20260930163805780.png)

Select text in a workspace file, run directory, attachment, or file change to quote the corresponding lines. Selections in a Diff automatically include the complete affected change blocks, and the file header can quote the full Diff. Quotes retain the file path, line numbers, and change origin so the Agent can interpret the context accurately.

![image.png](https://static.dion.blue/2026/09/20260930162523931.png)

### 3.Discard changes to an individual file from source control

Source control file actions now include “Discard changes.” After confirmation, both staged and unstaged changes are restored to `HEAD`; new files that are not tracked by Git are deleted.

> [!attention] Discarded changes cannot be recovered
> This action does not move content to the system Recycle Bin or Trash. Confirm that you no longer need the file's local changes before continuing.

![image.png](https://static.dion.blue/2026/09/20260930162543785.png)

### 4.Redesigned update entry point and installation flow

When a new version is available, a persistent “Update” button appears in the title bar. Open it to read the release notes, enlarge images in the notes, and start the update directly. Download progress is shown in the dialog, failed downloads can be retried, and completed downloads prompt you to restart and install.

![image.png](https://static.dion.blue/2026/09/20260930162647062.png)

Installation now saves open edits, pauses sessions, and closes connections and background services through the normal exit lifecycle. Gold Band then verifies the downloaded update's signature and version before installation, reducing the risk of losing work state or installing the wrong package.

This release is a normal update: the client only notifies you and waits for you to start the download and installation manually.

### 5.Targeted npx cache repair in Agent management

When an npm/npx Agent fails diagnostics because its installation cache is incomplete, Agent management offers “Repair cache.” After you review the target directories, Gold Band removes only the npx installation directories involved in that error and automatically checks the Agent again.

The repair does not clear the entire npm cache or remove project files, global packages, or account settings. Agents currently using those cache directories may be interrupted, so stop related tasks first.

![image.png|500](https://static.dion.blue/2026/09/20260930162937093.png)
![image.png|500](https://static.dion.blue/2026/09/20260930162952757.png)

## Experience Improvements

### 1.The right workspace now follows the session worktree

File browsing, editing, search, file links, the `@` file menu, and source control now use the current session's actual worktree consistently. Switching to another node or worktree updates the right workspace instead of reading the main project directory by mistake.

If a historical session's worktree has been reclaimed, Gold Band shows an explicit unavailable state. It switches to the main workspace only after you choose “Browse main workspace,” with no silent fallback.

### 2.Source control performance improvements for faster opening

Initial loading, background refreshes, and commit reviews are more responsive, reducing the wait when source control is first opened. Repository information and file lists appear earlier, with line statistics filled in afterward; workspace file search skips `.git` metadata, and transient Git lock files no longer trigger unnecessary full refreshes.

This also fixes delayed status updates after workspace file changes and slow requests overwriting newer results. Open Diffs, search terms, and repository information remain available during background reconciliation.

### 3.Other improvements

1.The file workspace now previews SVG files natively and can open the current file with a system application.
2.Agent turn file-change records are more complete, including changes from background processing or outside the worktree; incomplete evidence is clearly identified.
3.The app loads only the active interface language at startup to reduce unnecessary reads and startup work.
4.Fixed window borders disappearing after native resizing on Windows 10.
5.Fixed truncated-text tooltips remaining open after menu actions, tooltips sticking to the top-left corner after their trigger was hidden, and the composer losing focus after removing a role tag.
6.The default channel no longer shows the unavailable Requirements entry, and several remaining translation errors were corrected.
