/**
 * multica 绑定 chip 的键盘删除策略（纯函数，便于把交互契约固化为可回归测试）。
 *
 * 配合 ConversationComposer 内 chip 作为正文最前的 leading adornment（首行内嵌、text-indent 让位），
 * 模拟「chip 是正文首个 token」的删除体感：仅当光标停在正文最前且无选区时，Backspace 才删除 chip。
 *
 * 判定语义：
 * - 在文本起点的 Backspace 本就是 no-op，劫持它删 chip（等同点 × 按钮）；
 * - 正文非空、光标在中间/末尾（selectionStart > 0）时照常删字，不误删 chip；
 * - 存在选区时（即便起点为 0，selectionEnd !== 0）让位给正常选区删除，不误删 chip；
 * - 已提交 slash 命令时让位 slash 控制器（chip 与 slash 互斥，slash 优先）；
 * - 无 multica 绑定时不触发。
 */
export interface MulticaBindingBackspaceInput {
  /// 触发的按键名（来自 KeyboardEvent.key）。
  key: string;
  /// 当前是否存在 multica 绑定（draft.multica !== null）。
  multicaActive: boolean;
  /// 正文是否已提交为 slash 命令（committedSlashCommand !== null）。
  hasCommittedSlashCommand: boolean;
  /// textarea 选区起点（无选区时等于光标位置）。
  selectionStart: number;
  /// textarea 选区终点（无选区时等于 selectionStart）。
  selectionEnd: number;
}

export function shouldBackspaceClearMulticaBinding(input: MulticaBindingBackspaceInput): boolean {
  return (
    input.key === 'Backspace'
    && input.multicaActive
    && !input.hasCommittedSlashCommand
    && input.selectionStart === 0
    && input.selectionEnd === 0
  );
}
