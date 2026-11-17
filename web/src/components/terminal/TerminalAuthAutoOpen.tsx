import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { subscribeTerminalEvents } from '@/api';
import {
  useRightWorkspace,
  useRightWorkspaceCommands,
} from '@/components/workspace/right-workspace-context';

/**
 * 干预式自动展开（连接→对话 OTP 代输）：agent 驱动的远程会话需要动态口令时，
 * 后端发出 authPrompt 事件——此时用户往往还在会话视图里，终端面板未必打开。
 * 本组件在 App 壳层订阅该事件并自动把终端面板展开到右侧工作区（attention 高亮），
 * 让口令输入框直接呈现在用户面前，而不是等用户自己想到去开面板。
 *
 * 只对 authPrompt 干预事件触发；普通输出不打断用户当前的工作区状态。
 * 必须挂在 `RightWorkspaceProvider` 内部（依赖 openResource / scopeKey）。
 */
export function TerminalAuthAutoOpen() {
  const { t } = useTranslation();
  const { openResource, scopeKey } = useRightWorkspace();

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void subscribeTerminalEvents((event) => {
      if (event.event !== 'authPrompt') return;
      void (async () => {
        try {
          await openResource({
            kind: 'terminal',
            key: 'terminal',
            scopeKey: scopeKey ?? '',
            title: t('terminal.panelTitle'),
            attention: true,
          });
        } catch {
          // best-effort: 面板展开失败时口令输入框仍存在于终端面板（用户手动打开可见）
        }
      })();
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [openResource, scopeKey, t]);

  return null;
}
