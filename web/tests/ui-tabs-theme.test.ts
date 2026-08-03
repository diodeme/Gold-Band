import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

describe('themed tabs surface', () => {
  it('keeps the default tab track distinct from light workspace backgrounds', () => {
    const source = readFileSync(
      path.resolve(__dirname, '../src/components/ui/tabs.tsx'),
      'utf8',
    );

    expect(source).toContain(
      'default: "bg-secondary ring-1 ring-inset ring-border/70"',
    );
    expect(source).not.toContain('default: "bg-muted"');
    expect(source).toContain('line: "gap-1 bg-transparent"');
    expect(source).toContain('bare: "gap-1 bg-transparent p-0 ring-0"');
    expect(source).toContain('data-[state=active]:bg-background');
  });

  it('uses the bare variant when tab pills or their container own the boundary', () => {
    const composer = readFileSync(
      path.resolve(__dirname, '../src/components/conversation/ConversationComposer.tsx'),
      'utf8',
    );
    const roundDetail = readFileSync(
      path.resolve(__dirname, '../src/pages/RoundDetailPage.tsx'),
      'utf8',
    );

    expect(composer).toContain('<TabsList variant="bare" className={CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsListClassName}>');
    expect(roundDetail).toContain('<TabsList variant="bare" className="h-8 w-fit rounded-full border bg-background/70 p-1 shadow-sm">');
    expect(composer).not.toContain('TabsList className="h-10 gap-1 bg-transparent p-0"');
  });
});
