/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs';
import path from 'node:path';
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';

import { afterEach, describe, expect, it, vi } from 'vitest';

import { ConversationSessionSwitcher } from '@/components/conversation/ConversationSessionSwitcher';
import type { ConversationSessionLeafVm, ConversationSessionTreeVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const switcherSource = readFileSync(
  path.resolve(__dirname, '../src/components/conversation/ConversationSessionSwitcher.tsx'),
  'utf8',
);
const headerSource = readFileSync(
  path.resolve(__dirname, '../src/components/conversation/ConversationRunHeader.tsx'),
  'utf8',
);

function leaf(attemptId: string): ConversationSessionLeafVm {
  return {
    roundId: 'round-001',
    nodeId: 'dev',
    attemptId,
    pathLabel: `开发/${attemptId}`,
    status: 'paused',
    outcome: null,
    runtimeDisplay: {
      code: 'paused',
      tone: 'warning',
      icon: 'pause',
      terminal: false,
      resumable: true,
      reasonCode: 'waiting-for-user-input',
      blockingError: false,
    },
    current: attemptId === 'attempt-001',
    manualCheckPending: false,
    artifactCount: 0,
    attachmentCount: 0,
  };
}

function tree(): ConversationSessionTreeVm {
  return {
    selectedSessionKey: 'round-001/dev/attempt-001',
    rounds: [{
      roundId: 'round-001',
      index: 1,
      label: 'round-001',
      status: 'paused',
      runtimeDisplay: leaf('attempt-001').runtimeDisplay,
      nodes: [{
        nodeId: 'dev',
        label: '开发',
        nodeType: 'dev',
        status: 'paused',
        runtimeDisplay: leaf('attempt-001').runtimeDisplay,
        attempts: [leaf('attempt-001'), leaf('attempt-002')],
      }],
    }],
  };
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('conversation session switcher theme surface', () => {
  it('delegates transparency and blur to the theme popover recipe', () => {
    expect(switcherSource).toContain('data-theme-role="popover"');
    expect(switcherSource).toContain('className="w-64 p-2"');
    expect(switcherSource).not.toContain('bg-popover');
    expect(switcherSource).not.toContain('border-border/60 bg-popover');
    expect(switcherSource).not.toContain('shadow-sm');
    expect(headerSource).toContain('aria-expanded={sessionSwitcherOpen}');
  });

  it('keeps hover, selected, and current-session semantics distinct', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSelectSession = vi.fn();

    try {
      await act(async () => {
        root.render(React.createElement(ConversationSessionSwitcher, {
          tree: tree(),
          selectedKey: 'round-001/dev/attempt-001',
          onSelectSession,
        }));
      });

      const buttons = [...container.querySelectorAll('button')];
      const round = buttons.find((button) => button.textContent?.trim() === 'round-001');
      const node = buttons.find((button) => button.textContent?.trim() === '开发');
      const selected = buttons.find((button) => button.textContent?.trim() === '开发/attempt-001');
      const idle = buttons.find((button) => button.textContent?.trim() === '开发/attempt-002');

      expect(round?.className).toContain('hover:bg-sidebar-accent/55');
      expect(node?.className).toContain('hover:bg-sidebar-accent/55');
      expect(selected?.className).toContain('bg-sidebar-accent');
      expect(selected?.className).toContain('hover:bg-sidebar-accent');
      expect(selected?.getAttribute('aria-current')).toBe('true');
      expect(selected?.dataset.selected).toBe('true');
      expect(idle?.className).toContain('hover:bg-sidebar-accent/55');
      expect(idle?.getAttribute('aria-current')).toBeNull();
      expect(idle?.dataset.selected).toBe('false');

      await act(async () => selected?.click());
      expect(onSelectSession).toHaveBeenCalledWith(expect.objectContaining({ attemptId: 'attempt-001' }));
    } finally {
      await act(async () => root.unmount());
    }
  });
});
