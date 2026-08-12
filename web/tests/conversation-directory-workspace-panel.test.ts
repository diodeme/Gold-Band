import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';
import { isConversationDirectorySelectedFile } from '@/components/workspace/ConversationDirectoryWorkspacePanel';
import type { WorkspaceDirectoryEntryVm } from '@/types';

const file: WorkspaceDirectoryEntryVm = {
  name: 'node.json',
  relativePath: 'node.json',
  canonicalPath: 'D:\\attempt\\node.json',
  kind: 'file',
  hasChildren: false,
  byteLength: 42,
  modifiedAtNs: '1',
};

const source = readFileSync(
  fileURLToPath(new URL('../src/components/workspace/ConversationDirectoryWorkspacePanel.tsx', import.meta.url)),
  'utf8',
);

describe('conversation directory tree selection', () => {
  it('fills the available directory pane instead of using a fixed tree viewport height', () => {
    expect(source).toContain('useMeasuredElementHeight(320)');
    expect(source).toContain('height={treeHeight}');
    expect(source).not.toContain('height={500}');
  });

  it('highlights only the file open in the running-directory detail pane', () => {
    expect(isConversationDirectorySelectedFile(file.canonicalPath, file)).toBe(true);
    expect(isConversationDirectorySelectedFile('D:\\attempt\\other.json', file)).toBe(false);
    expect(isConversationDirectorySelectedFile(file.canonicalPath, { ...file, kind: 'directory', hasChildren: true })).toBe(false);
  });
});
