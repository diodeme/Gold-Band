import { describe, expect, it, vi } from 'vitest';
import {
  rememberPreferredGitRemote,
  resolvePreferredGitRemote,
} from '@/components/workspace/source-control/source-control-preferences';

describe('source control preferences', () => {
  it('remembers a remote by canonical repository identity across remounts', () => {
    vi.spyOn(Date, 'now').mockReturnValue(42);
    const storage = memoryStorage();

    rememberPreferredGitRemote('D:\\Repo\\.git\\', 'crystepj-max', storage);

    expect(resolvePreferredGitRemote('d:/repo/.git', ['origin', 'crystepj-max'], null, storage)).toBe('crystepj-max');
  });

  it('falls back to the upstream remote and then the first available remote when a preference is unavailable', () => {
    const storage = memoryStorage();
    rememberPreferredGitRemote('D:/repo/.git', 'removed', storage);

    expect(resolvePreferredGitRemote('D:/repo/.git', ['origin', 'fork'], 'fork/main', storage)).toBe('fork');
    expect(resolvePreferredGitRemote('D:/repo/.git', ['origin'], null, storage)).toBe('origin');
  });

  it('ignores corrupt or unsupported preference schemas', () => {
    const storage = memoryStorage('{"version":2,"remoteByRepository":{"d:/repo/.git":{"remote":"fork","updatedAt":1}}}');
    expect(resolvePreferredGitRemote('D:/repo/.git', ['origin', 'fork'], null, storage)).toBe('origin');
  });
});

function memoryStorage(initial: string | null = null) {
  let value = initial;
  return {
    getItem: () => value,
    setItem: (_key: string, next: string) => { value = next; },
  };
}
