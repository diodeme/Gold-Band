/** @vitest-environment jsdom */
// 会话内口令输入卡片（连接→对话）的最小 DOM 验收：
// 1) 请求出现时卡片与口令输入框可见（用户实际可见结果，而非内部状态）；
// 2) 提交把口令交给 onSubmit（不经过模型，直接走 provide_terminal_auth 通道）；
// 3) 提交失败保留输入与错误提示，用户可重试或放弃；
// 4) 取消交给 onDismiss。
import { describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';

import '@/i18n';
import { ConversationOtpCard } from '@/components/terminal/ConversationOtpCard';
import type { TerminalOtpRequest } from '@/lib/terminal-otp-bus';

function mountCard(request: TerminalOtpRequest, handlers: {
  onSubmit: (sessionId: string, answer: string) => Promise<void>;
  onDismiss: (sessionId: string) => void;
}): { root: Root; container: HTMLElement } {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(
      <ConversationOtpCard request={request} onSubmit={handlers.onSubmit} onDismiss={handlers.onDismiss} />,
    );
  });
  return { root, container };
}

const request: TerminalOtpRequest = {
  sessionId: 'sess-otp',
  message: 'Dynamic password:',
  label: 'webank',
  origin: { projectId: 'p1', taskId: 't1', runId: 'run-1' },
};

describe('ConversationOtpCard', () => {
  // React 受控输入：必须用原型 setter 赋值再派发 input 事件才能触发 onChange。
  const typeAnswer = (input: HTMLInputElement, value: string) => {
    const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
    act(() => {
      valueSetter.call(input, value);
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
  };

  it('renders the passphrase input for the connection', () => {
    const { root, container } = mountCard(request, { onSubmit: vi.fn(), onDismiss: vi.fn() });
    const input = container.querySelector<HTMLInputElement>('input[type="password"]');
    expect(input).not.toBeNull();
    expect(input?.disabled).toBe(false);
    expect(container.textContent).toContain('webank');
    act(() => root.unmount());
    container.remove();
  });

  it('submits the typed answer through the provided channel', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const onDismiss = vi.fn();
    const { root, container } = mountCard(request, { onSubmit, onDismiss });
    const input = container.querySelector<HTMLInputElement>('input[type="password"]')!;
    // 渲染顺序固定：[0]=提交，[1]=取消（i18n 默认 zh-CN，不用文案匹配）。
    const submit = container.querySelectorAll('button')[0] as HTMLButtonElement;;
    expect(submit.disabled).toBe(true);
    typeAnswer(input, '654321');
    expect(submit.disabled).toBe(false);
    await act(async () => {
      submit.click();
    });
    expect(onSubmit).toHaveBeenCalledWith('sess-otp', '654321');
    expect(onDismiss).not.toHaveBeenCalled();
    act(() => root.unmount());
    container.remove();
  });

  it('keeps the input and shows the error when the answer is rejected', async () => {
    const onSubmit = vi.fn().mockRejectedValue(new Error('rejected'));
    const { root, container } = mountCard(request, { onSubmit, onDismiss: vi.fn() });
    const input = container.querySelector<HTMLInputElement>('input[type="password"]')!;
    // 渲染顺序固定：[0]=提交，[1]=取消（i18n 默认 zh-CN，不用文案匹配）。
    const submit = container.querySelectorAll('button')[0] as HTMLButtonElement;;
    typeAnswer(input, 'wrong');
    await act(async () => {
      submit.click();
    });
    expect(container.querySelector('[role="alert"]')?.textContent).toBeTruthy();
    expect(container.querySelector<HTMLInputElement>('input[type="password"]')).not.toBeNull();
    act(() => root.unmount());
    container.remove();
  });

  it('hands dismissal to the parent instead of submitting', () => {
    const onDismiss = vi.fn();
    const onSubmit = vi.fn();
    const { root, container } = mountCard(request, { onSubmit, onDismiss });
    const cancel = container.querySelectorAll('button')[1] as HTMLButtonElement;;
    act(() => {
      cancel.click();
    });
    expect(onDismiss).toHaveBeenCalledWith('sess-otp');
    expect(onSubmit).not.toHaveBeenCalled();
    act(() => root.unmount());
    container.remove();
  });
});
