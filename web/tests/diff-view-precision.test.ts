import { describe, expect, it } from 'vitest';
import { diff } from '@codemirror/merge';
import {
  DIFF_VIEW_SCAN_LIMIT,
  DIFF_VIEW_TIMEOUT_MS,
} from '@/components/workspace/files/TurnFileWorkspacePanel';

describe('large source diff precision', () => {
  it('does not collapse distributed edits into a whole-file replacement', () => {
    const before = Array.from(
      { length: 4_000 },
      (_, index) => `fn item_${index}() { shared_call(${index % 37}); }`,
    );
    const after = before.map((line, index) => index % 30 === 0 ? `${line} changed` : line);
    const changes = diff(
      `${before.join('\n')}\n`,
      `${after.join('\n')}\n`,
      { scanLimit: DIFF_VIEW_SCAN_LIMIT, timeout: DIFF_VIEW_TIMEOUT_MS },
    );

    expect(changes.length).toBeGreaterThan(100);
    expect(Math.max(...changes.map((change) => change.toB - change.fromB))).toBeLessThan(64);
  });
});
