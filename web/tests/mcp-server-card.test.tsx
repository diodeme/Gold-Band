/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string | { defaultValue?: string; name?: string }) => {
      const values: Record<string, string> = {
        'contextManagement.mcp.goldBandMemoryName': 'Gold Band Shared Memory',
        'contextManagement.mcp.diagnosticPassed': 'The latest MCP configuration check passed',
      };
      if (values[key]) return values[key];
      if (typeof fallback === 'string') return fallback;
      return fallback?.defaultValue?.replace('{{name}}', fallback.name ?? '') ?? key;
    },
  }),
}));
vi.mock('@/components/ui/tooltip', () => ({
  TooltipProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
}));

import { McpServerCard } from '@/components/McpServerCard';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let container: HTMLDivElement;

beforeEach(() => {
  container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
});

describe('managed memory MCP card', () => {
  it('uses the standard stdio card, localized identity and latest-check semantics', async () => {
    const onToggle = vi.fn();
    await act(async () => root.render(
      <McpServerCard
        server={{
          id: 'gold-band-memory',
          name: 'gold-band-memory',
          enabled: true,
          transport: 'stdio',
          command: 'gold-band-desktop.exe',
          args: ['--gold-band-memory-mcp'],
          managed: true,
        }}
        health={{ status: 'healthy' }}
        isChecking={false}
        isToolsFetching={false}
        onToggle={onToggle}
        onHealthCheck={vi.fn()}
        onShowTools={vi.fn()}
      />,
    ));

    expect(container.textContent).toContain('Gold Band Shared Memory');
    expect(container.textContent).toContain('Stdio');
    expect(container.textContent).toContain('The latest MCP configuration check passed');
    expect(container.textContent).not.toContain('Running');
    expect(container.querySelector('[aria-label="Edit"]')).toBeNull();
    expect(container.querySelector('[aria-label="Delete"]')).toBeNull();

    const toggle = container.querySelector<HTMLButtonElement>('[role="switch"]')!;
    await act(async () => toggle.click());
    expect(onToggle).toHaveBeenCalledWith(false);
  });
});
