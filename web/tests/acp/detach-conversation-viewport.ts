import { act } from 'react';
import { expect } from 'vitest';

/**
 * 把会话视口驱动到"脱离底部"状态：伪造滚动几何（jsdom 无布局，clientHeight/scrollHeight
 * 恒为 0），再派发 wheel 事件触发 ACPChatDialog 的滚动处理器，使 return-to-latest 按钮
 * 按视口脱离语义提交显示。
 *
 * 提取自 acp-session-reentry-reconciliation.test.tsx 的同名局部 helper。
 */
export async function detachConversationViewport(container: HTMLElement) {
  const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
    .find((element) => element.classList.contains('h-full')
      && element.classList.contains('overflow-y-auto'));
  expect(scroller).toBeDefined();
  if (scroller!.scrollHeight <= scroller!.clientHeight) {
    Object.defineProperties(scroller!, {
      clientHeight: { configurable: true, value: 100 },
      scrollHeight: { configurable: true, value: 1_000 },
      scrollTop: { configurable: true, value: 500, writable: true },
    });
  } else if (
    scroller!.scrollHeight - scroller!.scrollTop - scroller!.clientHeight <= 2
  ) {
    scroller!.scrollTop = Math.max(
      0,
      scroller!.scrollHeight - scroller!.clientHeight - 100,
    );
  }
  await act(async () => {
    scroller!.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: -1 }));
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  });
  return scroller!;
}
