/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { ResidentPinToggle } from '@/components/conversation/ResidentPin';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
});

function render(pinned: boolean, onChange: (resident: boolean) => Promise<void>) {
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  act(() => {
    root.render(<TooltipProvider><ResidentPinToggle pinned={pinned} onChange={onChange} /></TooltipProvider>);
  });
  const button = host.querySelector<HTMLButtonElement>('[data-resident-toggle]');
  if (!button) throw new Error('toggle not rendered');
  return { root, button };
}

describe('ResidentPinToggle', () => {
  it('shows the outline pin and requests residency when the session is not resident', async () => {
    const onChange = vi.fn().mockResolvedValue(undefined);
    const { button } = render(false, onChange);

    expect(button.getAttribute('aria-pressed')).toBe('false');
    expect(button.getAttribute('aria-label')).toBe('conversation.sidebar.keepResident');
    expect(button.querySelector('[data-resident-pin="off"]')).not.toBeNull();

    await act(async () => { button.click(); });
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it('shows the filled pin and releases residency when the session is resident', async () => {
    const onChange = vi.fn().mockResolvedValue(undefined);
    const { button } = render(true, onChange);

    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(button.getAttribute('aria-label')).toBe('conversation.sidebar.releaseResident');
    expect(button.querySelector('[data-resident-pin="on"]')).not.toBeNull();

    await act(async () => { button.click(); });
    expect(onChange).toHaveBeenCalledWith(false);
  });

  it('blocks repeated clicks and shows progress until the request settles', async () => {
    let settle!: () => void;
    const onChange = vi.fn(() => new Promise<void>((resolve) => { settle = resolve; }));
    const { button } = render(false, onChange);

    act(() => { button.click(); });
    expect(button.disabled).toBe(true);
    expect(button.getAttribute('aria-busy')).toBe('true');
    expect(button.querySelector('[data-resident-pin]')).toBeNull();
    act(() => { button.click(); });
    expect(onChange).toHaveBeenCalledTimes(1);

    await act(async () => { settle(); });
    expect(button.disabled).toBe(false);
    expect(button.querySelector('[data-resident-pin="off"]')).not.toBeNull();
  });
});
