/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@/api', () => ({ prepareAppExit: vi.fn() }));
vi.mock('@/api/shared', () => ({ isTauriRuntime: () => false }));

import { WindowCloseSaveFailureDialog } from '@/components/WindowCloseCoordinator';

afterEach(() => {
  document.body.innerHTML = '';
});

describe('window close save failure dialog', () => {
  it('offers retry, cancel, and discard decisions', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);
    const onDecision = vi.fn();

    await act(async () => {
      root.render(
        <WindowCloseSaveFailureDialog
          open
          onOpenChange={() => {}}
          onDecision={onDecision}
        />,
      );
    });

    const buttons = [...document.body.querySelectorAll('button')];
    expect(document.body.textContent).toContain('common.windowCloseSaveFailedTitle');
    expect(buttons.map((button) => button.textContent)).toEqual([
      'common.retrySave',
      'common.cancelClose',
      'common.discardChangesAndExit',
    ]);

    for (const [index, decision] of ['retry', 'cancel', 'discard'].entries()) {
      await act(async () => buttons[index]?.click());
      expect(onDecision).toHaveBeenNthCalledWith(index + 1, decision);
    }

    await act(async () => root.unmount());
  });
});
