/**
 * 终端选区 → 会话 composer 的引用桥（连接→对话）。
 *
 * 终端面板（右侧工作区）与对话 composer 分属不同组件树；选区引用用模块级
 * 事件桥跨树传递。仅主窗口生效——detached 终端是独立 webview，桥无订阅者，
 * 引用按钮在 detached 场景不渲染（其会话无对应 composer）。
 */

export interface TerminalQuoteEvent {
  text: string;
  nonce: number;
}

let nonce = 0;
type Listener = (event: TerminalQuoteEvent) => void;
const listeners = new Set<Listener>();

export function emitTerminalQuote(text: string): void {
  const event = { text, nonce: ++nonce };
  for (const listener of listeners) listener(event);
}

export function onTerminalQuote(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
