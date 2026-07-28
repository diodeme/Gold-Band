import { describe, expect, it } from 'vitest';
import { ACP_SYSTEM_PROMPT_DIALOG_LAYOUT } from '../src/components/acp/ACPChatDialog';

describe('ACP system prompt dialog layout', () => {
  it('keeps the dialog bounded and delegates vertical overflow to one scroll area', () => {
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('max-h-[86vh]');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('overflow-hidden');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('flex');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('flex-col');

    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.headerClassName).toContain('shrink-0');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('gold-themed-scrollbar');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('min-h-0');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('flex-1');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('overflow-y-scroll');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('overflow-x-hidden');
  });

  it('wraps long paths and prompt content without creating a nested scroll container', () => {
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.promptClassName).toContain('min-w-0');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.promptClassName).toContain('max-w-full');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.promptClassName).toContain('overflow-x-hidden');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.promptClassName).toContain('whitespace-pre-wrap');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.promptClassName).toContain('break-all');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.promptClassName).toContain('[overflow-wrap:anywhere]');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.promptClassName).not.toContain('overflow-auto');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.promptClassName).not.toContain('max-h-');
  });

  it('does not depend on a percentage-height Radix viewport inside a max-height dialog', () => {
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT).not.toHaveProperty('scrollAreaType');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT).not.toHaveProperty('scrollAreaClassName');
  });
});
