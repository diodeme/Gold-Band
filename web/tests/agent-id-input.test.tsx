// @vitest-environment jsdom

import { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import { AgentIdInput } from '../src/pages/AgentManagementPage';

let cleanup: (() => void) | null = null;

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

function AgentIdInputHarness() {
  const [value, setValue] = useState('');
  return (
    <AgentIdInput
      value={value}
      disabled={false}
      placeholder="Agent ID"
      onValueChange={setValue}
    />
  );
}

describe('AgentIdInput', () => {
  it('preserves IME composition text and normalizes it only after composition ends', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);
    cleanup = () => {
      act(() => root.unmount());
      container.remove();
    };

    await act(async () => root.render(<AgentIdInputHarness />));
    const input = container.querySelector('input');
    expect(input).not.toBeNull();

    await act(async () => {
      input!.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
      setNativeInputValue(input!, '入-my');
      input!.dispatchEvent(new InputEvent('input', { bubbles: true, data: '入-my', inputType: 'insertCompositionText' }));
    });
    expect(input!.value).toBe('入-my');

    await act(async () => {
      input!.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '入-my' }));
    });
    expect(input!.value).toBe('-my');
  });
});

function setNativeInputValue(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
  setter?.call(input, value);
}
