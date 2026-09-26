import { describe, expect, it } from 'vitest';
import { remapWorkspacePath, workspacePathIsWithin } from '@/components/workspace/files/workspace-path';

describe('workspace path helpers', () => {
  it('matches a path, its descendants, and Windows drive paths case-insensitively', () => {
    expect(workspacePathIsWithin('D:\\repo\\src', 'D:\\repo\\src')).toBe(true);
    expect(workspacePathIsWithin('d:/Repo/src/main.rs', 'D:\\repo\\SRC')).toBe(true);
    expect(workspacePathIsWithin('D:\\repo\\src-old\\a.rs', 'D:\\repo\\src')).toBe(false);
    expect(workspacePathIsWithin('/home/dev/Src/a.rs', '/home/dev/src')).toBe(false);
  });

  it('moves descendants to the renamed parent and keeps the platform separator', () => {
    expect(remapWorkspacePath('D:\\repo\\src\\lib\\mod.rs', 'D:\\repo\\src', 'D:\\repo\\source')).toBe('D:\\repo\\source\\lib\\mod.rs');
    expect(remapWorkspacePath('src/lib/mod.rs', 'src', 'source')).toBe('source/lib/mod.rs');
    expect(remapWorkspacePath('src/a.rs', 'src/a.rs', 'src/b.rs')).toBe('src/b.rs');
    expect(remapWorkspacePath('other/a.rs', 'src', 'source')).toBe('other/a.rs');
  });
});
