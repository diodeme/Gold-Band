import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const messageSource = readFileSync(
  fileURLToPath(new URL('../src/components/prompt-kit/message.tsx', import.meta.url)),
  'utf8',
);
const chatSource = readFileSync(
  fileURLToPath(new URL('../src/components/acp/ACPChatDialog.tsx', import.meta.url)),
  'utf8',
);
const hiddenPromptSource = readFileSync(
  fileURLToPath(new URL('../src/components/acp/HiddenPromptMessageContent.tsx', import.meta.url)),
  'utf8',
);
const activitySpinnerSource = readFileSync(
  fileURLToPath(new URL('../src/components/acp/AcpProcessingSpinner.tsx', import.meta.url)),
  'utf8',
);
const runHeaderSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/ConversationRunHeader.tsx', import.meta.url)),
  'utf8',
);
const stylesSource = readFileSync(
  fileURLToPath(new URL('../src/styles.css', import.meta.url)),
  'utf8',
);

describe('ACP message theme contract', () => {
  it('uses the shared user-message semantic surface without primary tint, border, or elevation', () => {
    expect(messageSource).toContain('variant === "user" && "bg-message-user text-message-user-foreground"');
    expect(chatSource).toContain('variant={isUser ? "user" : "assistant"}');
    expect(chatSource).toContain('w-fit max-w-full rounded-br-md shadow-none');
    expect(chatSource).not.toContain('var(--primary)_16%');
    expect(chatSource).not.toContain('var(--primary)_26%');
  });

  it('keeps hidden runtime context inside the same tonal surface instead of nesting a white card', () => {
    expect(hiddenPromptSource).toContain('bg-foreground/[0.025]');
    expect(hiddenPromptSource).toContain('border-foreground/10');
    expect(hiddenPromptSource).not.toContain('bg-background/35');
    expect(hiddenPromptSource).not.toContain('bg-background/45');
  });

  it('sizes the user bubble from every currently visible prompt section', () => {
    expect(hiddenPromptSource).toContain('inline-grid min-w-0 max-w-full gap-2');
    expect(hiddenPromptSource).toContain('projectHiddenPromptDisplayParts(parts)');
    expect(chatSource).toContain('[container-type:inline-size]');
    expect(chatSource).toContain('data-acp-message-row={isUser ? "user" : "assistant"}');
    expect(stylesSource).toContain(
      '--conversation-message-max-inline-size: 82cqi;',
    );
    expect(hiddenPromptSource).toContain(
      'resolvePromptBubbleInlineSize',
    );
    expect(hiddenPromptSource).toContain('measuredLineInlineSizes');
    expect(hiddenPromptSource).toContain('getClientRects()');
    expect(hiddenPromptSource).toContain('style={measuredInlineSize ? { width: `${measuredInlineSize}px` } : undefined}');
    expect(hiddenPromptSource).not.toContain('max-w-4xl');
    expect(hiddenPromptSource).not.toContain('max-w-6xl');
    expect(hiddenPromptSource).toContain('className="grid min-w-0 max-w-full"');
    expect(hiddenPromptSource).toContain(
      'group grid min-w-0 grid-cols-[minmax(0,1fr)_auto]',
    );
    expect(hiddenPromptSource).toContain(
      '<CollapsibleContent className="min-w-0 max-w-full">',
    );
    expect(hiddenPromptSource).toContain('max-h-72 w-max min-w-0 max-w-full');
    expect(hiddenPromptSource).not.toContain('[contain:inline-size]');
    expect(hiddenPromptSource).not.toContain('w-full min-w-0');
    expect(hiddenPromptSource).not.toContain('open ? "w-full" : "w-fit"');
  });

  it('uses the compositor-friendly CSS ring for live activity and composer processing', () => {
    expect(chatSource).toContain('<AcpProcessingSpinner className="size-3.5" />');
    expect(activitySpinnerSource).toContain('border-t-gold-running');
    expect(activitySpinnerSource).toContain('motion-safe:animate-spin');
    expect(activitySpinnerSource).not.toContain('Loader2');
  });

  it('keeps assistant prose and main content headers on the page surface', () => {
    expect(messageSource).toContain('variant === "assistant" && "bg-transparent text-foreground"');
    expect(chatSource).toContain(': "rounded-bl-md shadow-none"');
    expect(chatSource).toContain('bg-content-header px-5');
    expect(runHeaderSource).toContain('bg-content-header px-5');
    expect(chatSource).not.toContain('bg-gold-surface-high/60 px-5');
  });

  it('animates only active retry progress and respects reduced motion', () => {
    expect(chatSource).toContain(
      'retryFooter === "retrying" && "acp-retry-live-label"',
    );
    expect(stylesSource).toContain('@media (prefers-reduced-motion: no-preference)');
    expect(stylesSource).toContain('.acp-retry-live-label {');
    expect(stylesSource).toContain(
      'animation: acp-activity-label-breathe 1.8s ease-in-out infinite;',
    );
    expect(stylesSource).toContain('opacity: 0.48;');
    expect(stylesSource).toContain('opacity: 1;');
  });
});
