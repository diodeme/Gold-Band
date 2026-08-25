import { useCallback, useEffect, useMemo, useState } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getAgentCommandCatalog } from '@/api';
import { isTauriRuntime } from '@/api/shared';
import { mergeSlashCommandSources } from '@/lib/slash-command';
import type { AcpCommandItemVm } from '@/types';

const EMPTY_AGENT_COMMANDS: readonly AcpCommandItemVm[] = [];

export function useAgentCommands(
  agentType: string | null | undefined,
  workspacePath: string | null | undefined,
  sessionCommands?: readonly unknown[] | null,
) {
  const requestKey = agentType?.trim() && workspacePath?.trim()
    ? `${agentType.trim()}\n${workspacePath.trim()}`
    : null;
  const [snapshot, setSnapshot] = useState<{
    key: string | null;
    commands: AcpCommandItemVm[];
    skillCommands: AcpCommandItemVm[];
  }>({ key: null, commands: [], skillCommands: [] });

  const refresh = useCallback(async () => {
    if (!requestKey || !agentType || !workspacePath) return;
    try {
      const catalog = await getAgentCommandCatalog(agentType, workspacePath);
      setSnapshot({
        key: requestKey,
        commands: catalog?.commands ?? [],
        skillCommands: catalog?.skillCommands ?? [],
      });
    } catch {
      setSnapshot({ key: requestKey, commands: [], skillCommands: [] });
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

  const cachedCommands = snapshot.key === requestKey ? snapshot.commands : EMPTY_AGENT_COMMANDS;
  const scannedSkillCommands = snapshot.key === requestKey
    ? snapshot.skillCommands
    : EMPTY_AGENT_COMMANDS;
  const fallbackCommands = sessionCommands == null ? cachedCommands : scannedSkillCommands;
  const commands = useMemo(
    () => mergeSlashCommandSources(sessionCommands, fallbackCommands),
    [fallbackCommands, sessionCommands],
  );

  return {
    catalogKey: requestKey,
    commands,
  };
}
