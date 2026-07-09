import { describe, expect, it } from 'vitest';

import {
  SKILL_PROJECT_WORKSPACE_STORAGE_KEY,
  readRememberedSkillProjectWorkspace,
  rememberSkillProjectWorkspace,
} from '../src/lib/skill-workspace-memory';

class MemoryStorage {
  private values = new Map<string, string>();

  getItem(key: string) {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string) {
    this.values.set(key, value);
  }

  removeItem(key: string) {
    this.values.delete(key);
  }
}

describe('skill workspace memory', () => {
  it('returns the remembered workspace when it still exists', () => {
    const storage = new MemoryStorage();
    rememberSkillProjectWorkspace('D:/repo-a', storage);

    expect(readRememberedSkillProjectWorkspace([
      { workspacePath: 'D:/repo-a' },
      { workspacePath: 'D:/repo-b' },
    ], storage)).toBe('D:/repo-a');
  });

  it('clears stale workspace selections', () => {
    const storage = new MemoryStorage();
    storage.setItem(SKILL_PROJECT_WORKSPACE_STORAGE_KEY, 'D:/missing-repo');

    expect(readRememberedSkillProjectWorkspace([{ workspacePath: 'D:/repo-a' }], storage)).toBe('');
    expect(storage.getItem(SKILL_PROJECT_WORKSPACE_STORAGE_KEY)).toBeNull();
  });

  it('does not clear memory before the workspace list loads', () => {
    const storage = new MemoryStorage();
    storage.setItem(SKILL_PROJECT_WORKSPACE_STORAGE_KEY, 'D:/repo-a');

    expect(readRememberedSkillProjectWorkspace([], storage)).toBe('');
    expect(storage.getItem(SKILL_PROJECT_WORKSPACE_STORAGE_KEY)).toBe('D:/repo-a');
  });
});
