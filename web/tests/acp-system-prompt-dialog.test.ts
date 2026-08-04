import { describe, expect, it } from 'vitest';
import { ACP_SYSTEM_PROMPT_DIALOG_LAYOUT } from '../src/components/acp/ACPChatDialog';

describe('ACP system prompt dialog layout', () => {
  it('keeps the dialog bounded and gives its flex viewport to the read-only editor', () => {
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('max-h-[86vh]');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('overflow-hidden');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('flex');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('flex-col');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).toContain('sm:max-w-5xl');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName).not.toContain('max-w-4xl');

    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.headerClassName).toContain('shrink-0');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('min-h-0');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('flex-1');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('overflow-hidden');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).not.toContain('overflow-y-scroll');
  });

  it('delegates prompt scrolling and Markdown controls to the read-only AtomEditor viewer', () => {
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).toContain('overflow-hidden');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName).not.toContain('overflow-y-scroll');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.bodyClassName).toContain('h-full');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.bodyClassName).toContain('min-h-0');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.attemptSelectorClassName).toContain('absolute');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.attemptSelectorClassName).toContain('left-2');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.attemptSelectorClassName).toContain('top-2');
  });

  it('does not depend on a percentage-height Radix viewport inside a max-height dialog', () => {
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT).not.toHaveProperty('scrollAreaType');
    expect(ACP_SYSTEM_PROMPT_DIALOG_LAYOUT).not.toHaveProperty('scrollAreaClassName');
  });
});
