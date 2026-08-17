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
const stylesSource = readFileSync(
  fileURLToPath(new URL('../src/styles.css', import.meta.url)),
  'utf8',
);

describe('composer context horizontal alignment', () => {
  it('aligns quick-conversation context items, command tags, and text to one edge', () => {
    expect(quickComposerSource).toContain('data-conversation-composer="quick"');
    expect(quickComposerSource).toContain('w-full overflow-y-hidden px-0 py-0');
    expect(quickComposerSource).toContain('className="absolute left-0 top-0 z-10 inline-flex"');
    expect(stylesSource).toContain('[data-conversation-composer="quick"] [data-composer-context-area="true"]');
    expect(stylesSource).toContain('padding-inline: 0;');
  });

  it('aligns ACP context items with the prompt-kit textarea content inset', () => {
    expect(stylesSource).toContain('[data-conversation-composer="acp"] [data-composer-context-area="true"]');
    expect(stylesSource).toContain('padding-inline: 0.75rem;');
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
