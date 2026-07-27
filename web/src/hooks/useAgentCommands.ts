import { useCallback, useEffect, useState } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getAgentCommandCatalog } from '@/api';
import { isTauriRuntime } from '@/api/shared';
import type { AcpCommandItemVm } from '@/types';

export function useAgentCommands(
  agentType: string | null | undefined,
  workspacePath: string | null | undefined,
) {
  const requestKey = agentType?.trim() && workspacePath?.trim()
    ? `${agentType.trim()}\n${workspacePath.trim()}`
    : null;
  const [snapshot, setSnapshot] = useState<{
    key: string | null;
    commands: AcpCommandItemVm[];
  }>({ key: null, commands: [] });

  const refresh = useCallback(async () => {
    if (!requestKey || !agentType || !workspacePath) return;
    try {
      const catalog = await getAgentCommandCatalog(agentType, workspacePath);
      setSnapshot({ key: requestKey, commands: catalog?.commands ?? [] });
    } catch {
      setSnapshot({ key: requestKey, commands: [] });
    }
  }, [agentType, requestKey, workspacePath]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    void listen('gold-band://agent-commands-updated', () => {
      if (!disposed) void refresh();
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh]);

  return {
    catalogKey: requestKey,
    commands: snapshot.key === requestKey ? snapshot.commands : [],
  };
}
