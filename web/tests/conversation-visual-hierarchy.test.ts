import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

function source(relativePath: string) {
  return readFileSync(path.resolve(__dirname, relativePath), 'utf8');
}

describe('conversation visual hierarchy contract', () => {
  it('keeps the conversation title compact and above session metadata in the type scale', () => {
    const title = source('../src/components/conversation/EditableConversationTitle.tsx');
    const header = source('../src/components/conversation/ConversationRunHeader.tsx');

    expect(title).toContain('text-sm font-semibold leading-5 text-foreground');
    expect(title).toContain('text-ui-caption leading-4 text-muted-foreground/55');
    expect(header).toContain('bg-content-header px-5 py-2');
    expect(header).toContain('h-7 gap-1.5 px-2 text-xs font-normal');
  });

  it('uses tighter spacing inside groups and wider spacing between message groups', () => {
    const dialog = source('../src/components/acp/ACPChatDialog.tsx');
    const composer = source('../src/components/conversation/AcpConversationComposer.tsx');
    const quickComposer = source('../src/components/conversation/ConversationComposer.tsx');

    expect(dialog).toContain('max-w-[var(--conversation-content-rail-max-inline-size)] space-y-1 px-5 py-5');
    expect(dialog).toContain('<div className="min-w-0 space-y-1">');
    expect(dialog).toContain('<div className="px-5 pt-1 pb-2">');
    expect(dialog).toContain('className="relative mx-auto w-full max-w-[var(--conversation-content-rail-max-inline-size)] [filter:drop-shadow(var(--gb-material-shadow))_drop-shadow(var(--gb-material-edge-shadow))]"');
    expect(dialog).toContain('absolute left-0 top-px z-20 w-max max-w-[calc(100%-0.625rem)] -translate-y-full');
    expect(dialog).toContain('rounded-t-md border border-b-0 border-border bg-card py-0.5 pl-2.5 pr-3 !shadow-none');
    expect(dialog).toContain("before:-right-2.5 before:bottom-px before:size-2.5 before:rounded-bl-md before:shadow-[-3px_3px_0_3px_var(--card)] before:content-['']");
    expect(dialog).toContain("after:-right-2.5 after:bottom-px after:size-2.5 after:rounded-bl-md after:border-b after:border-l after:border-border after:content-['']");
    expect(dialog).toContain('composerInfoTabTarget === "todo"');
    expect(dialog).toContain('integratedInfoTab={composerInfoTabTarget === "queue"}');
    expect(dialog).toContain('integratedInfoTab={composerInfoTabTarget === "composer"}');
    expect(composer).toContain("integratedInfoTab && !attachedPanelVisible && 'rounded-tl-none'");
    expect(composer).toContain('bg-card !shadow-none transition-colors');
    expect(dialog).not.toContain('absolute -top-1 left-3 z-20 w-max max-w-[calc(100%-1.5rem)] -translate-y-full');
    expect(dialog).not.toContain('className="mb-1"');
    expect(dialog).not.toContain('<div className="px-5 py-3">');
    expect(dialog).not.toContain('shrink-0 bg-background/95 backdrop-blur');
    expect(dialog).toContain('data-acp-conversation-rail="timeline"');
    expect(dialog).toContain('data-acp-conversation-rail="composer"');
    expect(dialog.match(/max-w-\[var\(--conversation-content-rail-max-inline-size\)\]/g)).toHaveLength(2);
    expect(composer).toContain('mt-2 flex min-w-0 flex-wrap items-center gap-2 px-2 py-2');
    expect(composer).not.toContain('data-acp-composer-command-bar="true">\n            <div className="min-w-0 flex-1">');
    expect(composer).not.toContain('promptInputHint');
    expect(quickComposer).not.toContain('promptInputHint');
    expect(dialog.lastIndexOf('<AcpUsagePanel')).toBeLessThan(dialog.lastIndexOf('<ConversationPromptQueue'));
    expect(dialog.lastIndexOf('<AcpUsagePanel')).toBeLessThan(dialog.lastIndexOf('<AcpTodoPanel'));
    expect(dialog.lastIndexOf('<AcpTodoPanel')).toBeLessThan(dialog.lastIndexOf('<ConversationPromptQueue'));
    expect(dialog.lastIndexOf('<ConversationPromptQueue')).toBeLessThan(dialog.lastIndexOf('<AcpConversationComposer'));
    expect(dialog).toContain('processingLabel={showComposerStatus ? composerStatusLabel : null}');
    expect(composer).not.toContain('{status}');
  });

  it('places workspace headings above task titles and keeps time metadata subordinate', () => {
    const sidebar = source('../src/components/conversation/ConversationSidebar.tsx');

    expect(sidebar).toContain('mb-4');
    expect(sidebar).toContain('text-sm font-bold leading-5 text-sidebar-foreground/80');
    expect(sidebar).toContain('truncate text-ui-compact');
    expect(sidebar).toContain("bg-sidebar-accent/70 font-semibold text-sidebar-accent-foreground");
    expect(sidebar).toContain('text-ui-caption font-normal leading-4 tabular-nums text-muted-foreground/55');
    expect(sidebar).not.toContain('text-ui-micro tabular-nums text-muted-foreground');
  });
});
