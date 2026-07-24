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
const runHeaderSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/ConversationRunHeader.tsx', import.meta.url)),
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

  it('lets collapsed hidden context follow visible prompt width and expand from hidden content on demand', () => {
    expect(hiddenPromptSource).toContain('inline-grid min-w-0 max-w-full gap-2');
    expect(hiddenPromptSource).toContain('!open && "[contain:inline-size]"');
    expect(hiddenPromptSource).toContain('group flex w-full min-w-0');
    expect(hiddenPromptSource).toContain('max-h-72 w-max min-w-0 max-w-full');
    expect(hiddenPromptSource).not.toContain('open ? "w-full" : "w-fit"');
  });

  it('keeps assistant prose and main content headers on the page surface', () => {
    expect(messageSource).toContain('variant === "assistant" && "bg-transparent text-foreground"');
    expect(chatSource).toContain(': "rounded-bl-md shadow-none"');
    expect(chatSource).toContain('bg-content-header px-5');
    expect(runHeaderSource).toContain('bg-content-header px-5');
    expect(chatSource).not.toContain('bg-gold-surface-high/60 px-5');
  });
});
