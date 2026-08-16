import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import {
  ACP_SESSION_COMPOSER_LAYOUT,
  CONVERSATION_HOME_COMPOSER_LAYOUT,
} from '@/lib/conversation-composer-layout';

const composerSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)),
  'utf8',
);
const stylesSource = readFileSync(
  fileURLToPath(new URL('../src/styles.css', import.meta.url)),
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
const runModeManagementSource = readFileSync(
  fileURLToPath(new URL('../src/pages/RunModeManagementPage.tsx', import.meta.url)),
  'utf8',
);
const acpChatSource = readFileSync(
  fileURLToPath(new URL('../src/components/acp/ACPChatDialog.tsx', import.meta.url)),
  'utf8',
);

describe('responsive desktop layout contracts', () => {
  it('keeps simple composer submit actions beside the workspace when space allows', () => {
    expect(composerSource).toContain('data-slot="conversation-composer-toolbar"');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName');
    expect(composerSource).toContain("data-layout={isDirect ? 'configured' : 'simple'}");
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.simpleToolbarClassName');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.configuredToolbarClassName');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.simpleTrailingActionsClassName');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.containerClassName).toContain('@container/conversation-composer');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName).toContain('grid gap-2');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.simpleToolbarClassName).toContain('grid-cols-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.simpleToolbarClassName).toContain('@sm/conversation-composer:grid-cols-[minmax(0,1fr)_auto]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.simpleTrailingActionsClassName).toContain('justify-end');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredToolbarClassName).toContain('@2xl/conversation-composer:grid-cols-[minmax(12rem,0.75fr)_minmax(28rem,1.25fr)]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName).toContain('@sm/conversation-composer:grid-cols-2');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName).toContain('@lg/conversation-composer:grid-cols-');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName).toContain('@2xl/conversation-composer:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName).not.toContain('@2xl/conversation-composer:flex');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configTriggerClassName).toBe('w-full max-w-none');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.submitActionsClassName).toContain('w-fit');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.submitActionsClassName).toContain('justify-self-end');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.submitActionsClassName).toContain('@sm/conversation-composer:col-start-2');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.submitActionsClassName).toContain('@lg/conversation-composer:col-start-3');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.sendButtonClassName).toContain('h-8');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.sendButtonClassName).toContain('w-auto');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.sendButtonClassName).not.toContain('w-full');
    expect(composerSource.match(/CONVERSATION_HOME_COMPOSER_LAYOUT\.submitActionsClassName/gu)).toHaveLength(2);
    expect(composerSource).not.toContain('basis-[15rem]');
    expect(composerSource).not.toContain('basis-[22rem]');
  });

  it('keeps the session composer command bar compact without shrinking the responsive home toolbar', () => {
    expect(ACP_SESSION_COMPOSER_LAYOUT.commandBarClassName).toContain('mt-1');
    expect(ACP_SESSION_COMPOSER_LAYOUT.commandBarClassName).toContain('px-1 py-1');
    expect(ACP_SESSION_COMPOSER_LAYOUT.configTriggerClassName).toContain('h-8');
    expect(acpChatSource.match(/triggerClassName=\{ACP_SESSION_COMPOSER_LAYOUT\.configTriggerClassName\}/gu)).toHaveLength(2);
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName).toContain('mt-3');
  });

  it('stacks run-mode and Agent controls before their composer container is wide enough', () => {
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionSectionClassName).toContain('flex-col');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionSectionClassName).toContain('@sm/conversation-composer:flex-row');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionTabsListClassName).toContain('w-full');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).toContain('flex-col');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).toContain('@sm/conversation-composer:flex-row');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).toContain('px-4 py-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).not.toContain('px-4 py-3');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('overflow-x-auto');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('overflow-y-hidden');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('gold-scrollbar-hidden');
    expect(stylesSource).toContain('.gold-scrollbar-hidden {');
    expect(stylesSource).toContain('scrollbar-width: none;');
    expect(stylesSource).toContain('.gold-scrollbar-hidden::-webkit-scrollbar {');
    expect(stylesSource).toContain('display: none;');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('py-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('@sm/conversation-composer:flex-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsListClassName).toContain('w-max');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName');
    expect(composerSource).not.toContain('className="min-w-0 overflow-x-auto"');
  });

  it('uses profile-list container width and wrapping card actions instead of viewport-only fixed rows', () => {
    expect(contextSource.match(/@container\/profile-list/g)?.length).toBe(2);
    expect(contextSource.match(/@6xl\/profile-list:grid-cols-3/g)?.length).toBe(2);
    expect(contextSource.match(/CardFooter className="[^"]*flex-wrap/g)?.length).toBeGreaterThanOrEqual(2);
  });

  it('keeps profile import settings and results in one resizable sheet workflow', () => {
    expect(contextSource).toContain("profileImport.surface === 'result' ? 'profile-import-result-sheet' : 'profile-import-settings-sheet'");
    expect(contextSource.match(/resizeStorageKey="context-management\/profile-import"/g)).toHaveLength(1);
    expect(contextSource).toContain('data-slot="profile-import-result-list"');
    expect(contextSource).toContain('className="min-h-0 w-full flex-1 overflow-hidden"');
    expect(contextSource).toContain('sm:grid-cols-[minmax(0,1fr)_auto]');
    expect(contextSource).toContain('break-all text-xs text-muted-foreground');
    expect(contextSource).toContain("returnToImportResult={profileImport.surface === 'editing'}");
  });

  it('uses nested container widths for settings sections and the theme drawer', () => {
    expect(settingsSource).toContain('@container/settings-section');
    expect(settingsSource).toContain('@container/settings-content');
    expect(settingsSource).toContain('@container/theme-drawer');
    expect(settingsSource).toContain('@2xl/theme-drawer:grid-cols-2');
    expect(settingsSource).toContain('@container/theme-summary');
    expect(settingsSource).toContain('@xl/theme-summary:grid-cols-[auto_minmax(0,1fr)_auto]');
    expect(settingsSource).toContain('@lg/theme-summary:col-span-2');
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
    expect(workflowEditorSource).toContain("data-workflow-editor-layout={isCompact ? 'compact' : 'split'}");
    expect(workflowEditorSource).toContain('<ResizablePanelGroup orientation="horizontal"');
    expect(workflowEditorSource).toContain("compactPane === 'canvas' ? canvasSurface : inspectorSurface");
    expect(workflowEditorSource).not.toContain('@5xl/workflow-editor:grid-cols-[minmax(0,1fr)_340px]');
    expect(workflowEditorSource).toContain('max-w-[calc(100%-1.5rem)] flex-wrap');
  });

  it('fills the remaining workflow management viewport and keeps inspector scrolling internal', () => {
    expect(runModeManagementSource).toContain("mode === 'workflow' ? 'flex flex-col gap-6 overflow-hidden' : 'space-y-6 overflow-y-auto pb-6'");
    expect(runModeManagementSource).toContain('className="min-h-0 min-w-0 flex-1 overflow-hidden"');
    expect(runModeManagementSource).toContain('className="h-full min-h-0"');
    expect(workflowEditorSource).toContain('<ScrollArea className="size-full">');
    expect(workflowEditorSource).toContain("cn('@container/workflow-editor h-[clamp(520px,calc(100dvh-11rem),760px)] min-h-0', className)");
  });

  it('keeps raw frame search separate while filters wrap horizontally inside the workspace width', () => {
    expect(acpChatSource).toContain('data-raw-frame-filters="true"');
    expect(acpChatSource).toContain('flex min-w-0 flex-wrap items-center gap-2');
    expect(acpChatSource).toContain('h-9 w-44 max-w-full');
    expect(acpChatSource).not.toContain('@3xl/raw-frame:flex-row');
  });
});
