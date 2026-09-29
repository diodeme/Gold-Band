/** @vitest-environment jsdom */
import { act, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SlashCommandMenu } from '@/components/conversation/SlashCommandMenu';
import { useSlashCommandController } from '@/hooks/useSlashCommandController';
import { MENTION_FILE_SEARCH_DEBOUNCE_MS } from '@/hooks/useMentionWorkspaceFiles';
import {
  buildMentionGroups,
  mentionFilesRequest,
  parentMentionView,
  roleSlashItems,
  type SlashCatalogGroup,
} from '@/lib/slash-command';
import type { WorkspaceDirectoryEntryVm } from '@/types';

const entry = (relativePath: string, kind: 'file' | 'directory' = 'file'): WorkspaceDirectoryEntryVm => ({
  name: relativePath.split('/').at(-1) ?? relativePath,
  relativePath,
  canonicalPath: `D:/repo/${relativePath}`,
  kind,
  hasChildren: kind === 'directory',
  byteLength: kind === 'file' ? 10 : null,
  modifiedAtNs: null,
});

const api = vi.hoisted(() => ({
  listWorkspaceDirectory: vi.fn(),
  searchWorkspaceFiles: vi.fn(),
}));
vi.mock('@/api/client', () => ({ getRuntimeApi: () => api }));

const labels = { files: 'Files', roles: 'Roles', workspaceRoot: 'Workspace' };
const roleItems = roleSlashItems([{ id: 'pf-dev', name: '开发', summary: 'dev', content: 'role body' }]);
const roleGroup: SlashCatalogGroup[] = [{ id: 'roles', heading: '', items: roleItems }];
const ready = (entries: WorkspaceDirectoryEntryVm[] = []) => ({ status: 'ready' as const, entries });

describe('@ mention groups', () => {
  it('lists categories first and only offers roles when the catalog has them', () => {
    const names = (groups: SlashCatalogGroup[]) => groups.flatMap((group) => group.items.map((item) => item.name));
    const root = { view: { kind: 'root' as const }, query: '', filesAvailable: true, files: ready(), labels };
    expect(names(buildMentionGroups({ ...root, roleItems }))).toEqual(['Files', 'Roles']);
    // Workflow and AUTO pass no roles, so the only category is files.
    expect(names(buildMentionGroups({ ...root, roleItems: [] }))).toEqual(['Files']);
  });

  it('browses one directory level and reports loading, empty and error in place', () => {
    const view = { kind: 'files' as const, path: 'src' };
    const groups = buildMentionGroups({
      view, query: '', roleItems, filesAvailable: true, labels,
      files: ready([entry('src/lib', 'directory'), entry('src/main.ts')]),
    });
    expect(groups).toHaveLength(1);
    expect(groups[0].heading).toBe('src');
    expect(groups[0].items.map((item) => [item.kind, item.name, item.description])).toEqual([
      ['workspace-directory', 'lib', ''],
      ['workspace-file', 'main.ts', 'src'],
    ]);
    const status = (files: Parameters<typeof buildMentionGroups>[0]['files']) =>
      buildMentionGroups({ view, query: '', roleItems, filesAvailable: true, labels, files })[0].status;
    expect(status({ status: 'loading', entries: [] })).toBe('loading');
    expect(status(ready())).toBe('empty');
    expect(status({ status: 'error', entries: [] })).toBe('error');
  });

  it('searches every category for a typed query', () => {
    const groups = buildMentionGroups({
      view: { kind: 'root' }, query: '开', roleItems, filesAvailable: true, labels,
      files: ready([entry('docs/开发.md')]),
    });
    expect(groups.map((group) => [group.heading, group.items.map((item) => item.name)])).toEqual([
      ['Roles', ['开发']],
      ['Files', ['开发.md']],
    ]);
  });

  it('asks for a directory while browsing, a search for a query, and nothing for roles', () => {
    expect(mentionFilesRequest({ kind: 'root' }, '')).toBeNull();
    expect(mentionFilesRequest({ kind: 'files', path: 'src' }, '')).toEqual({ kind: 'directory', path: 'src' });
    expect(mentionFilesRequest({ kind: 'files', path: 'src' }, ' ma ')).toEqual({ kind: 'search', query: 'ma' });
    expect(mentionFilesRequest({ kind: 'roles' }, 'x')).toBeNull();
    expect(parentMentionView({ kind: 'files', path: 'src/lib' })).toEqual({ kind: 'files', path: 'src' });
    expect(parentMentionView({ kind: 'files', path: 'src' })).toEqual({ kind: 'files', path: '' });
    expect(parentMentionView({ kind: 'files', path: '' })).toEqual({ kind: 'root' });
  });
});

const selected = vi.fn();
let cleanup = async () => {};
afterEach(async () => {
  await cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
});

const commandGroup: SlashCatalogGroup = {
  id: 'agent',
  heading: 'Agent',
  items: [{ kind: 'command', id: 'review', name: 'review', description: 'Review' }],
};

function Harness({ roles, initial = '' }: { roles: SlashCatalogGroup[]; initial?: string }) {
  const [input, setInput] = useState(initial);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const slash = useSlashCommandController({
    input,
    groups: roles,
    onInputChange: setInput,
    textareaRef,
    mention: { root: { projectId: 'project-1', workspacePath: 'D:/repo/.wt/a' }, labels, onSelectWorkspaceFile: selected },
  });
  return (
    <div>
      <textarea ref={textareaRef} value={input} onChange={(event) => setInput(event.target.value)} onKeyDown={slash.onKeyDown} />
      <ul data-open={slash.isOpen}>
        {slash.filteredGroups.map((group) => (
          <li key={group.id} data-heading={group.heading} data-status={group.status}>
            {group.items.map((item) => item.name).join(',')}
          </li>
        ))}
      </ul>
    </div>
  );
}

async function mount(roles: SlashCatalogGroup[] = roleGroup, initial = '') {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  cleanup = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  await act(async () => root.render(<Harness roles={roles} initial={initial} />));
  const textarea = host.querySelector('textarea')!;
  /** Sets the value as typing would, leaving the caret at `caret` (default: the end). */
  const type = async (value: string, caret = value.length) => {
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
      setter.call(textarea, value);
      textarea.setSelectionRange(caret, caret);
      textarea.dispatchEvent(new Event('input', { bubbles: true }));
    });
  };
  const press = async (key: string) => {
    await act(async () => {
      textarea.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    });
  };
  const groups = () => [...host.querySelectorAll('li')].map((li) => ({
    heading: li.dataset.heading,
    status: li.dataset.status,
    items: li.textContent,
  }));
  return { textarea, type, press, groups };
}

describe('@ mention navigation', () => {
  it('opens the files category, enters a directory, adds a file and clears the mention', async () => {
    api.listWorkspaceDirectory.mockImplementation(async (_projectId: string, path: string) => (
      path === '' ? [entry('src', 'directory'), entry('README.md')] : [entry('src/main.ts')]
    ));
    const { textarea, type, press, groups } = await mount();
    await type('@');
    expect(groups()).toEqual([{ heading: '', status: undefined, items: 'Files,Roles' }]);

    await press('Enter');
    expect(textarea.value).toBe('@');
    expect(api.listWorkspaceDirectory).toHaveBeenCalledWith({ projectId: 'project-1', workspacePath: 'D:/repo/.wt/a' }, '');
    expect(groups()).toEqual([{ heading: 'Workspace', status: undefined, items: 'src,README.md' }]);

    await press('Enter');
    expect(groups()).toEqual([{ heading: 'src', status: undefined, items: 'main.ts' }]);

    await press('Enter');
    expect(selected).toHaveBeenCalledWith(entry('src/main.ts'));
    expect(textarea.value).toBe('');
  });

  it('goes back one level with Backspace and keeps the @', async () => {
    api.listWorkspaceDirectory.mockResolvedValue([entry('src', 'directory')]);
    const { textarea, type, press, groups } = await mount();
    await type('@');
    await press('ArrowDown');
    await press('Enter');
    expect(groups()[0].items).toBe('开发');

    await press('Backspace');
    expect(textarea.value).toBe('@');
    expect(groups()[0].items).toBe('Files,Roles');
  });

  it('does not submit the bare @ while a directory is loading', async () => {
    api.listWorkspaceDirectory.mockReturnValue(new Promise(() => {}));
    const { textarea, type, press, groups } = await mount();
    await type('@');
    await press('Enter');
    expect(groups()).toEqual([{ heading: 'Workspace', status: 'loading', items: '' }]);

    const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    await act(async () => textarea.dispatchEvent(event));
    expect(event.defaultPrevented).toBe(true);
  });

  it('debounces the search and only publishes the latest response', async () => {
    vi.useFakeTimers();
    let resolveFirst!: (value: unknown) => void;
    api.searchWorkspaceFiles
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockImplementationOnce(async () => ({ requestId: 'b', entries: [entry('src/main.ts')], truncated: false }));
    const { type, groups } = await mount([]);
    await type('@m');
    await act(async () => { vi.advanceTimersByTime(MENTION_FILE_SEARCH_DEBOUNCE_MS); });
    await type('@ma');
    await type('@mai');
    await act(async () => { vi.advanceTimersByTime(MENTION_FILE_SEARCH_DEBOUNCE_MS); });
    expect(api.searchWorkspaceFiles).toHaveBeenCalledTimes(2);
    expect(api.searchWorkspaceFiles).toHaveBeenLastCalledWith({ projectId: 'project-1', workspacePath: 'D:/repo/.wt/a' }, 'mai', expect.any(String), 20);
    expect(groups()).toEqual([{ heading: 'Files', status: undefined, items: 'main.ts' }]);

    await act(async () => resolveFirst({ requestId: 'a', entries: [entry('stale.ts')], truncated: false }));
    expect(groups()).toEqual([{ heading: 'Files', status: undefined, items: 'main.ts' }]);
  });
});

describe('menu trigger typed before existing text', () => {
  it('opens / at the start of existing text and keeps that text after the command', async () => {
    const { textarea, type, press, groups } = await mount([commandGroup], 'please check');
    await type('/please check', 1);
    expect(groups()).toEqual([{ heading: 'Agent', status: undefined, items: 'review' }]);

    await press('Enter');
    expect(textarea.value).toBe('/review please check');
  });

  it('opens @ at the start of existing text, adds a file and keeps the text', async () => {
    api.listWorkspaceDirectory.mockResolvedValue([entry('README.md')]);
    const { textarea, type, press, groups } = await mount(roleGroup, 'explain this');
    await type('@explain this', 1);
    expect(groups()[0].items).toBe('Files,Roles');

    await press('Enter');
    expect(textarea.value).toBe('@explain this');
    expect(textarea.selectionStart).toBe(1);
    expect(groups()[0].items).toBe('README.md');

    await press('Enter');
    expect(selected).toHaveBeenCalledWith(entry('README.md'));
    expect(textarea.value).toBe('explain this');
  });

  it('puts a chosen role in front of the existing text', async () => {
    const { textarea, type, press } = await mount(roleGroup, 'fix the bug');
    await type('@开fix the bug', 2);
    await press('Enter');
    expect(textarea.value).toBe('@开发 fix the bug');
  });

  it('does not open for a trigger typed after other text', async () => {
    const { type, groups } = await mount([commandGroup], 'hello');
    await type('hello /', 7);
    expect(groups()).toEqual([]);
  });
});

describe('@ mention menu rows', () => {
  it('marks each category with its icon and leaves other rows plain', async () => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    // cmdk observes its list size and scrolls its selection into view; jsdom has neither.
    vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
    Element.prototype.scrollIntoView ??= () => {};
    const host = document.createElement('div');
    document.body.append(host);
    const root = createRoot(host);
    cleanup = async () => {
      await act(async () => root.unmount());
      host.remove();
      vi.unstubAllGlobals();
    };
    const categories = buildMentionGroups({
      view: { kind: 'root' }, query: '', roleItems, filesAvailable: true, files: ready(), labels,
    });
    await act(async () => root.render(
      <SlashCommandMenu
        open
        variant="inline"
        groups={[...categories, ...roleGroup]}
        activeIndex={0}
        onActiveIndexChange={() => {}}
        onDismiss={() => {}}
        onSelect={() => {}}
      >
        <textarea />
      </SlashCommandMenu>,
    ));
    const icons = [...host.querySelectorAll('[data-slash-item-kind]')].map((row) => [
      row.textContent,
      row.querySelector('svg:first-child')?.getAttribute('class')?.match(/lucide-([a-z-]+)/)?.[1] ?? null,
    ]);
    expect(icons).toEqual([
      ['Files', 'file-text'],
      ['Roles', 'brain'],
      ['开发dev', null],
    ]);
  });
});
