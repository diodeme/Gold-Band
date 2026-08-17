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
  it('shares the workspace info bar with scheduled authoring while keeping worktree selection unavailable there', () => {
    expect(composerSource).toContain('data-conversation-workspace-info="true"');
    expect(composerSource).toContain('workLocation={workLocation}');
    expect(composerSource).toContain("void selectLocation('worktree')");
    expect(composerSource).toContain('showWorkLocation={!scheduledMode}');
    expect(composerSource).not.toMatch(/\{scheduledMode \? \(\s*<ConversationWorkspaceControl/u);
  });

  it('uses a low inset info rail above the quick composer without affecting the session composer', () => {
    const infoBarBaseClasses = CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName
      .split(' ')
      .filter((className) => !className.startsWith('before:') && !className.startsWith('after:'));

    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).toContain('mx-auto');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).toContain('w-[80%]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).not.toContain('mx-9');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).toContain('h-7');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).toContain('items-center');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).toContain('justify-start');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).toContain('gap-0');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).toContain('pl-8');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).not.toContain('justify-center');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).toContain('[--conversation-workspace-info-surface:var(--gold-surface-high)]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).not.toContain('var(--gb-conversation-background)');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).not.toContain('rounded-t-2xl');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).not.toContain('before:');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).not.toContain('after:');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).not.toContain('bg-muted/80');
    expect(infoBarBaseClasses).not.toContain('absolute');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoRailClassName).toBe('min-w-0');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName');
    expect(acpChatSource).toContain('"absolute left-0 top-0 z-20');
    expect(acpChatSource).toContain('ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName');
    expect(acpChatSource).not.toContain('COMPOSER_ATTACHED_INFO_SURFACE_CLASS_NAME');
    expect(composerSource).not.toContain('bg-muted/45');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName).not.toContain('pt-2');
    expect(composerSource).not.toContain("scheduledMode ? '' : '-mt-2'");
    expect(composerSource).not.toContain('relative z-10 rounded-2xl border-border/60 bg-card/60');
    expect(composerSource).not.toContain('rounded-tl-none');
    expect(composerSource).toContain('data-conversation-workspace-info-curve="left"');
    expect(composerSource).toContain('data-conversation-workspace-info-curve="right"');
    expect(composerSource).toContain('data-conversation-workspace-info-body="true"');
    expect(composerSource).toContain('data-conversation-workspace-info-controls="true"');
    expect(composerSource).toContain('relative z-10 flex min-w-0 items-center gap-0');
    expect(composerSource).toContain('absolute inset-y-0 left-12 right-12');
    expect(composerSource).toContain('absolute left-0 bottom-0 h-7 w-12');
    expect(composerSource).toContain('absolute right-0 bottom-0 h-7 w-12');
    expect(composerSource).not.toContain('absolute -left-9 bottom-0 h-8 w-9');
    expect(composerSource).not.toContain('absolute -right-9 bottom-0 h-8 w-9');
    expect(composerSource).toContain("'M0 28L20.14 4Q23.497 0 29.497 0H48V28Z'");
    expect(composerSource).toContain('transform="translate(48 0) scale(-1 1)"');
  });

  it('keeps simple composer submit actions beside the workspace when space allows', () => {
    expect(composerSource).toContain('data-slot="conversation-composer-toolbar"');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName');
    expect(composerSource).toContain("data-layout={isDirect ? 'configured' : 'simple'}");
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.simpleToolbarClassName');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.configuredToolbarClassName');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.simpleTrailingActionsClassName');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.containerClassName).toContain('@container/conversation-composer');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.containerClassName).toContain('gap-1.5');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.containerClassName).not.toContain('gap-4');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName).toContain('grid gap-1.5');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName).not.toContain('border-t');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName).not.toContain('pt-');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.simpleToolbarClassName).toContain('grid-cols-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.simpleToolbarClassName).toContain('@xs/conversation-composer:grid-cols-[minmax(0,1fr)_auto]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.simpleToolbarClassName).not.toContain('@sm/conversation-composer:grid-cols-[minmax(0,1fr)_auto]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.simpleTrailingActionsClassName).toContain('justify-end');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.workspaceControlClassName).toContain('w-fit');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.workspaceControlClassName).toContain('max-w-full');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.workspaceControlClassName).toContain('flex-initial');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.workspaceControlClassName).not.toContain('flex-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredToolbarClassName).toContain('@2xl/conversation-composer:grid-cols-[minmax(12rem,0.75fr)_minmax(28rem,1.25fr)]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName).toContain('@sm/conversation-composer:grid-cols-2');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName).toContain('@lg/conversation-composer:grid-cols-');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName).toContain('@2xl/conversation-composer:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName).not.toContain('@2xl/conversation-composer:flex');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configTriggerClassName).toBe('h-8 px-2.5 w-full max-w-none');
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

  it('uses the session composer control baseline in the responsive home toolbar', () => {
    expect(ACP_SESSION_COMPOSER_LAYOUT.commandBarClassName).toContain('mt-1');
    expect(ACP_SESSION_COMPOSER_LAYOUT.commandBarClassName).toContain('px-1 py-1');
    expect(ACP_SESSION_COMPOSER_LAYOUT.configTriggerClassName).toContain('h-8');
    expect(acpChatSource.match(/triggerClassName=\{ACP_SESSION_COMPOSER_LAYOUT\.configTriggerClassName\}/gu)).toHaveLength(2);
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName).toContain('mt-2');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.configTriggerClassName).toContain(ACP_SESSION_COMPOSER_LAYOUT.configTriggerClassName);
    expect(composerSource).toContain('className="size-7 rounded-full"');
    expect(composerSource).not.toContain('className="size-9 rounded-full border border-border/50 bg-gold-surface-high/25');
  });

  it('uses the run-mode Route icon for the workflow option row', () => {
    expect(composerSource).toContain('data-conversation-workflow-selector="true"');
    expect(composerSource).toMatch(/data-conversation-workflow-selector="true"[\s\S]*?<Route className="size-4 text-muted-foreground"/u);
  });

  it('stacks run-mode and Agent controls before their composer container is wide enough', () => {
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionSectionClassName).toContain('flex-col');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionSectionClassName).toContain('@sm/conversation-composer:flex-row');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionTabsListClassName).toContain('w-full');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionTabsListClassName).toContain('h-7');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.modeControlHeightClassName).toBe('h-7');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).toContain('flex-col');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).toContain('@sm/conversation-composer:flex-row');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionSectionClassName).toContain('px-4 py-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionSectionClassName).not.toContain('py-2');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.optionSectionClassName).not.toContain('py-3');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).toContain('min-h-10');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).toContain('px-4 py-0');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).not.toContain('min-h-11');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName).not.toContain('px-4 py-3');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.autoSectionClassName).toContain('space-y-1.5');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.autoSectionClassName).toContain('px-4 py-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.autoGoalClassName).toContain('min-h-9');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.workflowSectionClassName).toContain('px-4 py-1');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.autoSectionClassName');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.autoGoalClassName');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.workflowSectionClassName');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('overflow-x-auto');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('overflow-y-hidden');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('gold-scrollbar-hidden');
    expect(stylesSource).toContain('.gold-scrollbar-hidden {');
    expect(stylesSource).toContain('scrollbar-width: none;');
    expect(stylesSource).toContain('.gold-scrollbar-hidden::-webkit-scrollbar {');
    expect(stylesSource).toContain('display: none;');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('py-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName).toContain('@sm/conversation-composer:flex-1');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsListClassName).toContain('h-8');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsListClassName).toContain('w-max');
    expect(composerSource).toContain('CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName');
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT.agentOptionClassName).toContain('h-8 min-w-8');
    expect(composerSource).toContain('className={CONVERSATION_HOME_COMPOSER_LAYOUT.agentOptionClassName}');
    expect(composerSource.match(/triggerClassName=\{CONVERSATION_HOME_COMPOSER_LAYOUT\.modeControlHeightClassName\}/gu)).toHaveLength(2);
    expect(composerSource).toContain('`${CONVERSATION_HOME_COMPOSER_LAYOUT.modeControlHeightClassName} min-w-0 flex-1 text-xs`');
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
    expect(workflowEditorSource).toContain('data-slot="worker-inspector"');
    expect(workflowEditorSource).toContain('data-slot="worker-model-config"');
    expect(workflowEditorSource).toContain('data-slot="worker-node-config"');
    expect(workflowEditorSource).not.toContain('<Badge variant="outline">worker</Badge>');
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
