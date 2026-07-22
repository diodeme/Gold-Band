import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const source = readFileSync(fileURLToPath(new URL('../src/pages/ContextManagementPage.tsx', import.meta.url)), 'utf8');

function openingTagBefore(tagName: 'textarea' | 'Textarea', marker: string): string {
  const markerIndex = source.indexOf(marker);
  if (markerIndex < 0) return '';
  const start = source.lastIndexOf(`<${tagName}`, markerIndex);
  if (start < 0) return '';
  const end = source.indexOf('>', markerIndex);
  return end < 0 ? '' : source.slice(start, end + 1);
}

describe('context management editor fonts', () => {
  it('lets MCP, SKILL, and profile body editors inherit the app font', () => {
    const mcpEditor = openingTagBefore('textarea', 'value={mcpJsonContent}');
    const skillEditor = openingTagBefore('textarea', 'value={form.body}');
    const profileEditor = openingTagBefore('Textarea', 'className="min-h-72 text-sm leading-relaxed"');

    expect(mcpEditor).not.toBe('');
    expect(skillEditor).not.toBe('');
    expect(profileEditor).not.toBe('');
    expect(mcpEditor).not.toContain('font-mono');
    expect(skillEditor).not.toContain('font-mono');
    expect(profileEditor).not.toContain('font-mono');
  });
});
