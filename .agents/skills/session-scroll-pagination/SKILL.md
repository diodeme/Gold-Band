---
name: session-scroll-pagination
description: Use this skill when designing or fixing chat/session/message-list scrolling, history pagination, prepend loading, scroll position preservation, grouped timeline items, collapsible tool/thought blocks, or flicker/jump issues in conversation UIs. Trigger it for ACP/chat drawers, agent session timelines, message history lists, infinite scroll, native scroll vs virtualization decisions, or any user complaint like “上滑跳动”, “翻页不连贯”, “消息闪烁”, “滚动不自然”, “历史加载后位置变了”.
---

# Session scroll pagination

Use this skill to avoid patchy chat-scroll fixes. The goal is simple: scrolling should feel continuous whether the list is showing current messages, loading older history, or preserving large/collapsible timeline items.

## Core mental model

Treat the visible chat list as a **finite window of logical timeline items**, not as raw protocol frames and not as arbitrary render chunks.

A timeline item is one user message, one assistant message, one thought block, one tool block, one plan block, or one intentionally grouped transcript block. Pagination changes which items are loaded; it must not split one logical item into visible fragments just to make scrolling easier.

## Before coding

Check these in order:

1. **Item identity**
   - Define a stable key for each logical item.
   - Tool updates should key by `toolCallId` when available.
   - Delta streams should keep the same item id while content grows.
   - Do not key by volatile render index or changing display seq.

2. **Window semantics**
   - Keep a bounded in-memory window if needed.
   - Older pagination prepends items; newer pagination appends items.
   - If trimming is needed, trim from the opposite side of the user action.
   - Cursor requests should use the current loaded window’s real oldest/newest item, not a stale session/page cursor.

3. **Visual unit semantics**
   - Do not split long messages into multiple visible bubbles just to reduce height.
   - Do not split thought/tool into separate summary row + detail row if the product expects one card.
   - Collapsible content should live inside the same timeline item.
   - Groups may contain nested timeline items, but the outer group still needs stable identity and predictable collapsed/expanded height behavior.

## Prepend history without jumps

Prefer **DOM anchor preservation** for chat history prepend.

Algorithm:

1. Before loading older history, find the first currently visible timeline item in the scroll container.
2. Save `{ key, top }`, where `key` is its stable item key and `top` is `element.getBoundingClientRect().top`.
3. Fetch older items using a cursor derived from the current window’s oldest item.
4. Merge older items before existing items, dedupe by stable item key, then trim from the end if the window is too large.
5. After React renders, find the same DOM item by key.
6. Set `scrollTop += newTop - oldTop`.
7. Ignore the programmatic scroll event for one animation frame so it does not trigger another page load.

Why: total `scrollHeight` delta is easy but breaks when the window also trims items from the bottom, when collapsibles change height, or when large messages/images settle after render. Anchoring to the user’s actual visible item preserves what the user was reading.

## Bottom pinning

Only pin to bottom when the user is already at bottom.

- Track `isAtBottom` with a small threshold.
- If new content arrives and the user is at bottom, scroll to bottom after render.
- If the user is reading history, do not force bottom; mark that newer content exists if needed.
- Do not let a session refresh or status update reset scroll position.

## Pagination triggers

Use scroll thresholds, but keep them modest and explain them as interaction thresholds, not preload-height hacks.

- Top threshold: trigger older history when `scrollTop` is near the top.
- Bottom threshold: trigger newer history only if there is a known newer window and the user reaches bottom.
- Guard with `loadingOlder` / `loadingNewer` refs to prevent duplicate requests.
- During programmatic scroll restoration, skip pagination triggers for one frame.

## Native scroll vs virtualization

Use native scroll when:

- The active window is bounded and not huge.
- Items have dynamic heights, collapsibles, Markdown, tool output, or nested groups.
- Product priority is natural reading continuity over rendering tens of thousands of rows.

Consider virtualization only when:

- The visible window can grow very large.
- You can model each row as an independent stable visual unit.
- You have a clear anchor/index strategy for prepend and dynamic height changes.

Do not introduce virtualization and then compensate with hardcoded pre-render heights, text chunking, or summary/detail fake rows unless the product explicitly accepts those visual semantics.

## Common traps

- Using stale `eventPage.oldestCursor` after the user has already paged or merged new events.
- Preserving position by item count instead of DOM position.
- Using `scrollHeight` delta while also trimming items from the opposite side.
- Letting programmatic scroll restoration trigger another history fetch.
- Auto-expanding thought/tool blocks during history load, then collapsing them, causing visible flicker.
- Treating raw protocol frames as visible chat rows.
- Changing item keys when status/content updates arrive.
- Fixing flicker by splitting user-visible messages into sections the user did not ask for.

## Implementation checklist

When fixing a bug, verify these before declaring success:

- Older history is reachable by scrolling up.
- After prepend, the same message/card remains under the user’s eyes.
- A long user prompt enters from below naturally; it does not jump to its top.
- Thought/tool cards remain one visible card each.
- Expanding/collapsing a card preserves position if the user is reading history; pins bottom only if already at bottom.
- New messages do not pull the user to bottom while they are reading older history.
- No virtual-list/render-block/text-chunk workaround remains unless it is part of the product design.

## Minimal report format

When reporting a fix, include:

- **Changed:** one sentence about the scroll/pagination behavior changed.
- **Key design choice:** anchor preservation, stable item identity, native scroll, bounded window, etc.
- **Verified:** what UI path was actually exercised.
- **Remaining limitation:** if real long-session data or browser/Tauri validation was not run, say so.
