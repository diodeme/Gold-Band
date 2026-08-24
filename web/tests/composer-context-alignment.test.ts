import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const quickComposerSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)),
  'utf8',
);
const acpDialogSource = readFileSync(
  fileURLToPath(new URL('../src/components/acp/ACPChatDialog.tsx', import.meta.url)),
  'utf8',
);
const acpComposerSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/AcpConversationComposer.tsx', import.meta.url)),
  'utf8',
);
const promptInputSource = readFileSync(
  fileURLToPath(new URL('../src/components/prompt-kit/prompt-input.tsx', import.meta.url)),
  'utf8',
).replace(/\r\n/g, '\n');
const composerLayoutSource = readFileSync(
  fileURLToPath(new URL('../src/lib/conversation-composer-layout.ts', import.meta.url)),
  'utf8',
);
const composerContextSource = readFileSync(
  fileURLToPath(new URL('../src/components/shared/ComposerContextArea.tsx', import.meta.url)),
  'utf8',
);
const stylesSource = readFileSync(
  fileURLToPath(new URL('../src/styles.css', import.meta.url)),
  'utf8',
);

describe('composer context horizontal alignment', () => {
  it('aligns quick-conversation context items, command tags, and text to one edge', () => {
    expect(quickComposerSource).toContain('data-conversation-composer="quick"');
    expect(quickComposerSource).toContain('className={CONVERSATION_HOME_COMPOSER_LAYOUT.textareaClassName}');
    expect(quickComposerSource).toContain('className="absolute left-0 top-2 z-10 inline-flex"');
    expect(composerLayoutSource).toContain("textareaClassName: `${COMPOSER_TEXTAREA_BASE_CLASS_NAME} w-full overflow-y-hidden px-0`");
    expect(composerLayoutSource).toContain("promptInputClassName: 'relative rounded-2xl border-border bg-card/60 px-2.5 py-2 shadow-sm'");
    expect(stylesSource).toContain('[data-conversation-composer="quick"] [data-composer-context-area="true"]');
    expect(stylesSource).toContain('padding-inline: 0;');
  });

  it('aligns ACP context items with the prompt-kit textarea content inset', () => {
    expect(acpComposerSource).toContain('className={ACP_SESSION_COMPOSER_LAYOUT.textareaClassName}');
    expect(acpComposerSource).toContain('ACP_SESSION_COMPOSER_LAYOUT.promptInputClassName');
    expect(composerLayoutSource).toContain("promptInputClassName: 'px-0'");
    expect(composerLayoutSource).toContain('textareaClassName: `${COMPOSER_TEXTAREA_BASE_CLASS_NAME} px-2.5`');
    expect(stylesSource).toContain('[data-conversation-composer="acp"] [data-composer-context-area="true"]');
    expect(stylesSource).toContain('padding-inline: 0.625rem;');
  });

  it('keeps vertical inset inside the textarea for plain and command-tag input', () => {
    expect(composerLayoutSource).toContain("COMPOSER_TEXTAREA_BASE_CLASS_NAME = 'min-h-12 py-2 text-sm leading-6 text-foreground placeholder:text-muted-foreground'");
    expect(composerLayoutSource).toContain("promptInputClassName: 'relative rounded-2xl border-border bg-card/60 px-2.5 py-2 shadow-sm'");
    expect(promptInputSource).toContain('className,\n        hasLeadingAdornment && "px-0"');
    expect(promptInputSource).not.toContain('hasLeadingAdornment && "px-0 py-0"');
    expect(promptInputSource).toContain('cn("relative min-w-0 px-2.5", containerClassName)');
    expect(promptInputSource).toContain('className="absolute left-2.5 top-2 z-10 inline-flex"');
  });

  it('keeps both composer surfaces and image previews on the full-contrast theme boundary', () => {
    expect(composerLayoutSource).toContain('rounded-2xl border-border bg-card/60');
    expect(composerLayoutSource).toContain("stackSurfaceClassName: 'border border-border [border-width:var(--acp-session-composer-border-width)]'");
    expect(composerContextSource).toContain('rounded-md border border-border object-cover');
  });

  it('routes paste through attachments without converting normal composer changes or submissions', () => {
    expect(quickComposerSource).toContain("onValueChange={(value) => setContent(`${committedSlashCommand?.prefix ?? ''}${value}`)}");
    expect(quickComposerSource).toContain('onPaste={(e) => { void handlePaste(e); }}');
    expect(quickComposerSource).toContain('content: trimmed');
    expect(acpDialogSource).toContain('onPromptChange={setPrompt}');
    expect(acpDialogSource).toContain('onPaste={handlePaste}');
    expect(acpDialogSource).toContain('createUserPromptSubmission(prompt, quotes)');
    expect(quickComposerSource).not.toContain('prepareUserPromptDraftUpdate');
    expect(acpDialogSource).not.toContain('prepareUserPromptDraftUpdate');
  });
});
