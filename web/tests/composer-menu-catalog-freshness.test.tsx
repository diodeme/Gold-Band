/** @vitest-environment jsdom */
import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock('@/api/shared', () => ({ isTauriRuntime: () => true }));
vi.mock('@/api', () => ({ getAgentCommandCatalog: vi.fn(), getProfiles: vi.fn() }));

import { getAgentCommandCatalog, getProfiles } from '@/api';
import { useComposerMenuCatalog } from '@/hooks/useComposerMenuCatalog';
import { useSlashCommandController } from '@/hooks/useSlashCommandController';
import { resetProfileCatalogForTests } from '@/lib/profile-catalog';
import type { AcpCommandCatalogVm, ProfileListVm, ProfileVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const LABELS = { files: 'Files', roles: 'Roles', workspaceRoot: 'Root' };

function catalog(skills: string[]): AcpCommandCatalogVm {
  const items = skills.map((name) => ({ name, description: name }));
  return { agentType: 'codex-acp', projectId: 'project-1', skillCommands: items, commands: items } as AcpCommandCatalogVm;
}

function profile(id: string, name: string): ProfileVm {
  return {
    id, name, summary: name, content: `${name} body`, dynamicTemplate: false,
    scope: 'user', isBuiltIn: false, createdAt: '', updatedAt: '', path: '',
  };
}

let setInput: (value: string) => void = () => undefined;

function Composer() {
  const [input, setValue] = useState('');
  setInput = setValue;
  const menuCatalog = useComposerMenuCatalog({
    agentType: 'codex-acp',
    workspacePath: 'C:/repo',
    rolesEnabled: true,
  });
  const slash = useSlashCommandController({
    input,
    groups: menuCatalog.groups,
    contextKey: menuCatalog.contextKey,
    onInputChange: setValue,
    mention: { root: null, labels: LABELS, onSelectWorkspaceFile: () => undefined },
    onMenuTrigger: menuCatalog.onMenuTrigger,
  });
  return <ul data-open={slash.isOpen}>{slash.filteredItems.map((item) => <li key={item.id}>{item.name}</li>)}</ul>;
}

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

beforeEach(async () => {
  resetProfileCatalogForTests();
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.clearAllMocks();
});

const menuItems = () => [...host.querySelectorAll('li')].map((item) => item.textContent);
const type = (value: string) => act(async () => setInput(value));

describe('composer menu catalogs', () => {
  it('lists a Skill added after the composer mounted when `/` opens', async () => {
    vi.mocked(getProfiles).mockResolvedValue({ profiles: [] });
    vi.mocked(getAgentCommandCatalog)
      .mockResolvedValueOnce(catalog(['old-skill']))
      .mockResolvedValue(catalog(['old-skill', 'new-skill']));
    await act(async () => root.render(<Composer />));

    await type('/');

    expect(menuItems()).toEqual(['old-skill', 'new-skill']);
  });

  it('lists a role created after the composer mounted when `@` opens', async () => {
    vi.mocked(getAgentCommandCatalog).mockResolvedValue(catalog([]));
    vi.mocked(getProfiles)
      .mockResolvedValueOnce({ profiles: [profile('pf-old', 'old-role')] } as ProfileListVm)
      .mockResolvedValue({ profiles: [profile('pf-old', 'old-role'), profile('pf-new', 'new-role')] } as ProfileListVm);
    await act(async () => root.render(<Composer />));

    await type('@new');

    expect(menuItems()).toEqual(['new-role']);
  });
});
