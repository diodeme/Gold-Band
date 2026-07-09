import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const source = readFileSync(fileURLToPath(new URL('../src/pages/ContextManagementPage.tsx', import.meta.url)), 'utf8');

describe('context management editor fonts', () => {
  it('lets MCP, SKILL, and profile body editors inherit the app font', () => {
    const mcpEditor = source.match(/<textarea[\s\S]*?value=\{mcpJsonContent\}/)?.[0] ?? '';
    const skillEditor = source.match(/<textarea[\s\S]*?value=\{form\.body\}/)?.[0] ?? '';
    const profileEditor = source.match(/<Textarea className="min-h-72[^"]*" \{\.\.\.field\}/)?.[0] ?? '';

    expect(mcpEditor).not.toContain('font-mono');
    expect(skillEditor).not.toContain('font-mono');
    expect(profileEditor).not.toContain('font-mono');
  });
});
