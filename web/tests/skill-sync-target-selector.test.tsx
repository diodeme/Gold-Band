/** @vitest-environment jsdom */

import { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { SkillSyncTargetSelector } from '@/components/SkillSyncTargetSelector';
import type { ConfiguredSkillAgentMeta } from '@/lib/skill-agent-display';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (_key: string, fallback: string) => fallback }),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const agents: ConfiguredSkillAgentMeta[] = [
  {
    agentType: 'claude-acp',
    label: 'Claude',
    iconKey: 'claude',
    primaryAgentDir: '.claude',
    projectPrimaryAgentDir: null,
    compatibleAgentDirs: [],
  },
  {
    agentType: 'codex-acp',
    label: 'Codex',
    iconKey: 'codex',
    primaryAgentDir: '.codex',
    projectPrimaryAgentDir: null,
    compatibleAgentDirs: ['.agents'],
  },
];

afterEach(() => {
  document.body.replaceChildren();
});

describe('SkillSyncTargetSelector', () => {
  it('selects and clears every available sync target as one controlled value', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    function Harness() {
      const [value, setValue] = useState<string[]>([]);
      return <SkillSyncTargetSelector agents={agents} value={value} onValueChange={setValue} />;
    }

    await act(async () => root.render(<Harness />));
    const selectAll = button(container, '全选');
    const selectNone = button(container, '全不选');
    expect(selectAll.disabled).toBe(false);
    expect(selectNone.disabled).toBe(true);

    await act(async () => selectAll.click());
    expect(checkboxStates(container)).toEqual(['checked', 'checked']);
    expect(selectAll.disabled).toBe(true);
    expect(selectNone.disabled).toBe(false);

    await act(async () => selectNone.click());
    expect(checkboxStates(container)).toEqual(['unchecked', 'unchecked']);
    expect(selectAll.disabled).toBe(false);
    expect(selectNone.disabled).toBe(true);

    await act(async () => root.unmount());
  });

  it('disables bulk actions when no configured Agent is available', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    await act(async () => root.render(<SkillSyncTargetSelector agents={[]} value={[]} onValueChange={vi.fn()} />));
    expect(button(container, '全选').disabled).toBe(true);
    expect(button(container, '全不选').disabled).toBe(true);
    expect(container.textContent).toContain('没有可同步的已配置 Agent。');

    await act(async () => root.unmount());
  });
});

function button(container: HTMLElement, label: string) {
  return Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find((item) => item.textContent === label)!;
}

function checkboxStates(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>('[role="checkbox"]')).map((item) => item.dataset.state);
}
