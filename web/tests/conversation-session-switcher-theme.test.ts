/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs';
import path from 'node:path';
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';

import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  CONVERSATION_SESSION_TREE_SCROLL_MAX_HEIGHT,
  ConversationSessionSwitcher,
  conversationSessionTreeBranchKey,
} from '@/components/conversation/ConversationSessionSwitcher';
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
  it('uses the shared popover and a viewport-bounded shadcn scroll area', async () => {
    expect(headerSource).toContain('<Popover open={sessionSwitcherOpen}');
    expect(headerSource).toContain('<PopoverContent');
    expect(switcherSource).toContain('<ScrollArea');
    expect(switcherSource).toContain('data-conversation-session-tree-scroll="true"');
    expect(headerSource).toContain('aria-expanded={sessionSwitcherOpen}');

    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(React.createElement(ConversationSessionSwitcher, {
          tree: tree(),
          selectedKey: 'round-001/dev/attempt-001',
          expansion: {},
          onExpansionChange: vi.fn(),
          onSelectSession: vi.fn(),
        }));
      });

      const scrollArea = container.querySelector<HTMLElement>('[data-conversation-session-tree-scroll="true"]');
      expect(scrollArea?.className).toContain('overflow-hidden');
      expect(scrollArea?.className).toContain('[&_[data-slot=scroll-area-viewport]]:max-h-[inherit]');
      expect(scrollArea?.style.maxHeight).toBe(CONVERSATION_SESSION_TREE_SCROLL_MAX_HEIGHT);
    } finally {
      await act(async () => root.unmount());
    }
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
          expansion: {},
          onExpansionChange: vi.fn(),
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

  it('controls each branch by its stable tree path so remounts can restore the run view state', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onExpansionChange = vi.fn();
    const roundBranchKey = conversationSessionTreeBranchKey(['round', 'round-001']);

    try {
      await act(async () => {
        root.render(React.createElement(ConversationSessionSwitcher, {
          tree: tree(),
          selectedKey: 'round-001/dev/attempt-001',
          expansion: { [roundBranchKey]: false },
          onExpansionChange,
          onSelectSession: vi.fn(),
        }));
      });

      const round = [...container.querySelectorAll('button')]
        .find((button) => button.textContent?.trim() === 'round-001');
      expect(round?.getAttribute('aria-expanded')).toBe('false');
      expect([...container.querySelectorAll('button')]
        .some((button) => button.textContent?.trim() === '开发')).toBe(false);

      await act(async () => round?.click());
      expect(onExpansionChange).toHaveBeenCalledWith(roundBranchKey, true);

      await act(async () => {
        root.render(React.createElement(ConversationSessionSwitcher, {
          tree: tree(),
          selectedKey: 'round-001/dev/attempt-001',
          expansion: { [roundBranchKey]: true },
          onExpansionChange,
          onSelectSession: vi.fn(),
        }));
      });
      expect([...container.querySelectorAll('button')]
        .some((button) => button.textContent?.trim() === '开发')).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
