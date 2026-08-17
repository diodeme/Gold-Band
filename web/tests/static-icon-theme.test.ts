import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const sourcePaths = [
  '../src/components/acp/TurnFileChangesCard.tsx',
  '../src/components/workspace/source-control/SourceControlWorkspacePanel.tsx',
  '../src/components/workspace/RightWorkspaceDock.tsx',
  '../src/components/workspace/files/TurnFileWorkspacePanel.tsx',
  '../src/components/workspace/files/ConversationAssetWorkspacePanel.tsx',
];

describe('static icon theme contract', () => {
  it('uses foreground instead of the accent color for workspace feature icons', () => {
    const sources = sourcePaths.map((path) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8'));

    for (const source of sources) {
      expect(source).toContain('text-foreground');
    }
    expect(sources[0]).not.toContain('<FileDiff className="size-4 shrink-0 text-primary"');
    expect(sources[1]).not.toContain('<GitBranch className="size-4 shrink-0 text-primary"');
    expect(sources[2]).not.toContain('<Icon className="size-4 shrink-0 text-primary"');
    expect(sources[3]).not.toMatch(/<(?:FileDiff|FileText)[^>]*text-primary/);
    expect(sources[4]).not.toContain('<FileText className="size-3.5 text-primary"');
  });
});
