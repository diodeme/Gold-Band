import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { shouldBackspaceClearMulticaBinding, type MulticaBindingBackspaceInput } from '../src/lib/conversation-composer-multica-chip';

const composerSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)),
  'utf8',
);

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

/**
 * 回归测试：multica 绑定 chip 的主题适配配色契约。
 *
 * chip 必须使用 accent/accent-foreground 配对——主题契约中保证双 scheme 对比度的「强调表面」
 * canonical 配对（permission-card、recipe hover/selected 同源）。严禁退回 primary 半透明染色：
 * primary 是「按钮表面」token，不保证单独作前景色时与背景有对比（tech-neutral dark 下
 * primary #2d2d2d 与 composer 背景 #1b1b1b 几乎重合，chip 文字/背景/边框全部不可辨认）。
 */
describe('multica binding chip theme contrast', () => {
  // chip 渲染段：从 multicaBinding 分支起、到 textarea 之前的 JSX（均为单行断言，CRLF 安全）。
  const chipSegment = composerSource.split('multicaBinding ? (')[1]?.split('<PromptInputTextarea')[0] ?? '';

  it('tints the chip from the guaranteed-contrast accent/accent-foreground pair', () => {
    expect(chipSegment).toContain('border-accent-foreground/15 bg-accent');
    expect(chipSegment).toContain('font-medium text-accent-foreground');
    expect(chipSegment).toContain('hover:bg-accent-foreground/15');
  });

  it('never tints the multica chip from primary (surface token, not a contrast-safe foreground)', () => {
    // 匹配 text-primary / bg-primary/10 / border-primary/30 / hover:bg-primary/20 等 primary 染色；
    // primary-foreground（带后缀）不在禁止范围。
    expect(chipSegment).not.toMatch(/-primary(?!-)/);
  });
});
