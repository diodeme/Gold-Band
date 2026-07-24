import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const composerSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)),
  'utf8',
);
const contextSource = readFileSync(
  fileURLToPath(new URL('../src/pages/ContextManagementPage.tsx', import.meta.url)),
  'utf8',
);
const settingsSource = readFileSync(
  fileURLToPath(new URL('../src/pages/SettingsPage.tsx', import.meta.url)),
  'utf8',
);
const workspaceSelectSource = readFileSync(
  fileURLToPath(new URL('../src/pages/WorkspaceSelectPage.tsx', import.meta.url)),
  'utf8',
);
const runDetailSource = readFileSync(
  fileURLToPath(new URL('../src/pages/RunDetailPage.tsx', import.meta.url)),
  'utf8',
);
const workflowEditorSource = readFileSync(
  fileURLToPath(new URL('../src/components/WorkflowEditor.tsx', import.meta.url)),
  'utf8',
);

describe('responsive desktop layout contracts', () => {
  it('lets the composer toolbar wrap its leading and trailing control groups', () => {
    expect(composerSource).toContain('data-slot="conversation-composer-toolbar"');
    expect(composerSource).toContain('flex flex-wrap items-center gap-3');
    expect(composerSource).toContain('basis-[15rem] flex-wrap');
    expect(composerSource).toContain('basis-[22rem] flex-wrap');
    expect(composerSource).not.toContain('h-8 w-[150px] rounded-full');
  });

  it('uses profile-list container width and wrapping card actions instead of viewport-only fixed rows', () => {
    expect(contextSource.match(/@container\/profile-list/g)?.length).toBe(2);
    expect(contextSource.match(/@6xl\/profile-list:grid-cols-3/g)?.length).toBe(2);
    expect(contextSource.match(/CardFooter className="[^"]*flex-wrap/g)?.length).toBeGreaterThanOrEqual(2);
  });

  it('uses nested container widths for settings sections and theme cards', () => {
    expect(settingsSource).toContain('@container/settings-section');
    expect(settingsSource).toContain('@container/settings-content');
    expect(settingsSource).toContain('@6xl/settings-content:grid-cols-2');
    expect(settingsSource).toContain('@container/theme-summary');
    expect(settingsSource).toContain('@xl/theme-summary:grid-cols-[auto_minmax(0,1fr)_auto]');
    expect(settingsSource).toContain('@container/theme-drawer');
    expect(settingsSource).toContain('@2xl/theme-drawer:grid-cols-2');
    expect(settingsSource).not.toContain('@lg/theme-drawer:grid-cols-[72px_minmax(0,1fr)]');
    expect(settingsSource).not.toContain('flex min-h-32 gap-4');
    expect(settingsSource).not.toContain('md:grid-cols-2');
    expect(settingsSource).not.toContain('lg:grid-cols-[160px_minmax(0,1fr)]');
  });

  it('stacks fixed desktop splits before their actual containers are wide enough', () => {
    expect(workspaceSelectSource).toContain('grid grid-cols-1');
    expect(workspaceSelectSource).toContain('lg:grid-cols-[minmax(0,0.95fr)_minmax(360px,0.55fr)]');
    expect(runDetailSource).toContain('@container/run-detail');
    expect(runDetailSource).toContain('@4xl/run-detail:grid-cols-[minmax(320px,420px)_minmax(0,1fr)]');
    expect(workflowEditorSource).toContain('@container/workflow-editor');
    expect(workflowEditorSource).toContain('@5xl/workflow-editor:grid-cols-[minmax(0,1fr)_340px]');
    expect(workflowEditorSource).toContain('max-w-[calc(100%-1.5rem)] flex-wrap');
  });
});
