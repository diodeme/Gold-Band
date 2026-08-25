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
    expect(messageSource).toContain('data-theme-role={variant === "user" ? "message-user" : variant === "assistant" ? "message-assistant" : "activity"}');
    expect(chatSource).toContain('variant={isUser ? "user" : "assistant"}');
    expect(chatSource).toContain('w-fit max-w-full rounded-br-md py-3 shadow-none');
    expect(chatSource).not.toContain('var(--primary)_16%');
    expect(chatSource).not.toContain('var(--primary)_26%');
  });

  it('renders hidden prompt navigation as a semantic link and keeps runtime control themed', () => {
    expect(hiddenPromptSource).toContain('variant="link"');
    expect(hiddenPromptSource).toContain('data-hidden-prompt-link="true"');
    expect(hiddenPromptSource).toContain('text-foreground/80');
    expect(hiddenPromptSource).toContain('has-[>svg]:px-0');
    expect(hiddenPromptSource).toContain('<FileText');
    expect(hiddenPromptSource).not.toContain('<Collapsible');
    expect(chatSource).toContain('data-theme-role="runtime-control"');
    expect(hiddenPromptSource).not.toContain('bg-foreground/[0.025]');
    expect(chatSource).not.toContain('border-primary/20 bg-primary/5');
    expect(chatSource).not.toContain('hover:bg-primary/10');
  });

  it('routes thought, activity, and intervention surfaces through existing theme roles', () => {
    expect(chatSource).toContain('data-theme-role="activity"');
    expect(chatSource).toContain('data-theme-role={compact ? undefined : "activity"}');
    expect(chatSource).toContain('data-theme-role="permission-card"');
    expect(chatSource).not.toContain('bg-card/65 px-4 py-3.5 shadow-');
  });

  it('keeps activity summaries and assistant copy actions compact', () => {
    expect(chatSource).toContain('min-h-7 w-full min-w-0 justify-start gap-1.5 rounded-none bg-transparent px-1 py-0.5');
    expect(chatSource).toContain('text-muted-foreground hover:bg-transparent hover:text-foreground');
    expect(chatSource).toContain('data-[state=open]:bg-transparent data-[state=open]:text-foreground');
    expect(chatSource).not.toContain('hover:bg-transparent hover:text-accent-foreground');
    expect(chatSource).toContain('data-agent-message-actions="true"');
    expect(chatSource).toContain('className="h-5 px-1 leading-none opacity-100');
    expect(chatSource).toContain('className="size-5 text-muted-foreground');
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
    expect(hiddenPromptSource).toContain(
      'h-auto min-w-0 max-w-full justify-start gap-1.5 p-0',
    );
    expect(hiddenPromptSource).toContain('expandedHiddenLineInlineSizes: []');
    expect(hiddenPromptSource).not.toContain('CollapsibleContent');
    expect(hiddenPromptSource).not.toContain('max-h-72');
    expect(hiddenPromptSource).not.toContain('[contain:inline-size]');
    expect(hiddenPromptSource).not.toContain('w-full min-w-0');
    expect(hiddenPromptSource).not.toContain('open ? "w-full" : "w-fit"');
  });

  it('always animates the compositor-friendly CSS ring for live activity and composer processing', () => {
    expect(chatSource).toContain('<AcpProcessingSpinner className="size-3.5" />');
    expect(activitySpinnerSource).toContain('border-t-gold-running');
    expect(activitySpinnerSource).toContain('border-t-gold-running animate-spin');
    expect(activitySpinnerSource).not.toContain('motion-safe:animate-spin');
    expect(activitySpinnerSource).not.toContain('motion-reduce:animate-none');
    expect(activitySpinnerSource).not.toContain('Loader2');
  });

  it('keeps assistant prose and main content headers on the page surface', () => {
    expect(messageSource).toContain('? "message-assistant" : "activity"');
    expect(chatSource).toContain(': "rounded-bl-md pb-0 pt-2 shadow-none"');
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
