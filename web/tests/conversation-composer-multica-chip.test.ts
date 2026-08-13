import { describe, expect, it } from 'vitest';
import { shouldBackspaceClearMulticaBinding, type MulticaBindingBackspaceInput } from '../src/lib/conversation-composer-multica-chip';

/**
 * 回归测试：multica 绑定 chip 内嵌输入框后，「Backspace 删 chip」的触发契约。
 *
 * 用户诉求：chip 移进输入框，删除键可直接删除（不限正文为空）。
 * 交互策略模拟「chip 是正文首个 token」——仅光标停在正文最前且无选区时，Backspace 才删 chip，
 * 其余情况让位给正常删字，避免误删。
 */
function input(overrides: Partial<MulticaBindingBackspaceInput> = {}): MulticaBindingBackspaceInput {
  return {
    key: 'Backspace',
    multicaActive: true,
    hasCommittedSlashCommand: false,
    selectionStart: 0,
    selectionEnd: 0,
    ...overrides,
  };
}

describe('shouldBackspaceClearMulticaBinding', () => {
  it('deletes the chip when cursor sits at the very start with no selection', () => {
    expect(shouldBackspaceClearMulticaBinding(input())).toBe(true);
  });

  it('deletes the chip even when content is non-empty (cursor at start)', () => {
    // 正文非空、但光标在最前：Backspace 在此处本就是 no-op，劫持删 chip。
    expect(shouldBackspaceClearMulticaBinding(input({ selectionStart: 0, selectionEnd: 0 }))).toBe(true);
  });

  it('does not delete when the key is not Backspace', () => {
    expect(shouldBackspaceClearMulticaBinding(input({ key: 'Delete' }))).toBe(false);
    expect(shouldBackspaceClearMulticaBinding(input({ key: 'Enter' }))).toBe(false);
  });

  it('does not delete when there is no multica binding', () => {
    expect(shouldBackspaceClearMulticaBinding(input({ multicaActive: false }))).toBe(false);
  });

  it('does not delete when a slash command is committed (slash takes priority)', () => {
    expect(shouldBackspaceClearMulticaBinding(input({ hasCommittedSlashCommand: true }))).toBe(false);
  });

  it('does not delete when the cursor is in the middle of the content', () => {
    expect(shouldBackspaceClearMulticaBinding(input({ selectionStart: 3, selectionEnd: 3 }))).toBe(false);
  });

  it('does not delete when the cursor is at the end of the content', () => {
    expect(shouldBackspaceClearMulticaBinding(input({ selectionStart: 10, selectionEnd: 10 }))).toBe(false);
  });

  it('does not delete when a selection spans text (even starting at 0)', () => {
    // 全选/部分选区时 Backspace 应删选区文本，而非误删 chip。
    expect(shouldBackspaceClearMulticaBinding(input({ selectionStart: 0, selectionEnd: 5 }))).toBe(false);
    expect(shouldBackspaceClearMulticaBinding(input({ selectionStart: 2, selectionEnd: 5 }))).toBe(false);
  });

  it('treats a missing textarea (selection -1) as non-deleting (defensive)', () => {
    expect(shouldBackspaceClearMulticaBinding(input({ selectionStart: -1, selectionEnd: -1 }))).toBe(false);
  });
});
