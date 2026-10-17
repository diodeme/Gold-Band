import { act } from 'react';

/** Dispatch the mouse entry sequence, then flush Radix's zero-delay opening timer. */
export async function hoverTooltip(trigger: Element) {
  await act(async () => {
    for (const type of ['pointerover', 'pointermove']) {
      const event = new MouseEvent(type, { bubbles: true });
      Object.defineProperty(event, 'pointerType', { value: 'mouse' });
      trigger.dispatchEvent(event);
    }
  });
  await act(async () => new Promise<void>((resolve) => setTimeout(resolve, 0)));
}
