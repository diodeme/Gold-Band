## Experience Improvements

### 1.Direct background stop fixes

1.Fixed Stop doing nothing when a background question appeared after a scheduled task woke the Agent. Stop now acts on the background work correctly: a pending permission request is recorded as cancelled, a pending question is answered with cancel, and the session no longer stays stuck waiting.
2.Permission requests and questions that arrive while a reply is running now settle with that turn instead of staying in the session after you stop; processed interactions are no longer rolled back to pending by newer live content.

### 2.Background activity no longer sticks

Some Agents never send the final update for tools after being stopped, which kept the sidebar showing the session as active, left the stop button up forever, and stopped resident sessions from ever being reclaimed. Tools now settle when their owning turn ends or when background work is stopped, and the session returns to idle after 10 seconds without activity.

### 3.Model menu fix in narrow panes

When the conversation pane is narrow, the secondary menu for the model and thinking effort used to be squeezed to the window's left edge and cover the sidebar. When there is no room, it now expands in place inside the main menu (click or use the arrow keys); when there is enough space it still pops out on hover as before.

### 4.Other improvements

1.Updated the ACP Registry snapshot and Agent catalog.
