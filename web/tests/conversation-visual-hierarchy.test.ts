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

    expect(dialog).toContain('max-w-[var(--conversation-content-rail-max-inline-size)] space-y-5 px-5 py-5');
    expect(dialog).toContain('border-t px-5 py-3');
    expect(dialog).toContain('data-acp-conversation-rail="timeline"');
    expect(dialog).toContain('data-acp-conversation-rail="composer"');
    expect(dialog.match(/max-w-\[var\(--conversation-content-rail-max-inline-size\)\]/g)).toHaveLength(2);
    expect(composer).toContain('mt-2 flex items-center gap-2 px-2 pb-1 text-xs leading-4');
    expect(composer).toContain('mt-2 flex min-w-0 flex-wrap items-center gap-2 border-t');
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
