import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getAgentCommandCatalog } from '@/api';
import { isTauriRuntime } from '@/api/shared';
import { mergeSlashCommandSources } from '@/lib/slash-command';
import type { AcpCommandCatalogVm, AcpCommandItemVm } from '@/types';

const EMPTY_AGENT_COMMANDS: readonly AcpCommandItemVm[] = [];
type CatalogUpdate = { agentType?: string; projectId?: string; workspacePath?: string };
type Subscription = {
  listeners: Set<(catalog: AcpCommandCatalogVm | null) => void>;
  catalog: AcpCommandCatalogVm | null;
  running: boolean;
  pending: boolean;
  scheduled: boolean;
  refresh: () => void;
};
// Entries exist only while a scope is mounted; consumers share work, not an unbounded cache.
const subscriptions = new Map<string, Subscription>();

function subscribe(key: string, agentType: string, workspacePath: string,
  listener: (catalog: AcpCommandCatalogVm | null) => void) {
  let entry = subscriptions.get(key);
  if (!entry) {
    const created: Subscription = { listeners: new Set(), catalog: null,
      running: false, pending: false, scheduled: false, refresh: () => {} };
    created.refresh = () => {
      if (created.scheduled) return;
      created.scheduled = true;
      queueMicrotask(async () => {
        created.scheduled = false;
        if (subscriptions.get(key) !== created) return;
        if (created.running) { created.pending = true; return; }
        created.running = true;
        try {
          const catalog = await getAgentCommandCatalog(agentType, workspacePath);
          if (subscriptions.get(key) !== created) return;
          if (sameCatalog(created.catalog, catalog)) return;
          created.catalog = catalog;
          for (const notify of created.listeners) notify(catalog);
        } catch {
          // Background failure retains the last successful projection.
        } finally {
          created.running = false;
          if (created.pending) { created.pending = false; created.refresh(); }
        }
      });
    };
    entry = created;
    subscriptions.set(key, entry);
    entry.refresh();
  }
  entry.listeners.add(listener);
  listener(entry.catalog);
  const current = entry;
  return { entry, release: () => {
    current.listeners.delete(listener);
    if (!current.listeners.size && subscriptions.get(key) === current) subscriptions.delete(key);
  } };
}

function sameCommands(left: readonly AcpCommandItemVm[] | null | undefined, right: readonly AcpCommandItemVm[] | null | undefined) {
  if (left === right) return true;
  if (!left || !right || left.length !== right.length) return false;
  return left.every((item, index) => item.name === right[index].name
    && item.description === right[index].description && item.inputHint === right[index].inputHint);
}

function sameCatalog(left: AcpCommandCatalogVm | null, right: AcpCommandCatalogVm | null) {
  if (!left || !right) return left === right;
  return left.projectId === right.projectId
    && sameCommands(left.commands, right.commands)
    && sameCommands(left.skillCommands, right.skillCommands);
}

export function useAgentCommands(
  agentType: string | null | undefined,
  workspacePath: string | null | undefined,
  sessionCommands?: readonly unknown[] | null,
) {
  const requestKey = agentType?.trim() && workspacePath?.trim()
    ? JSON.stringify([agentType.trim(), workspacePath.trim()])
    : null;
  const [snapshot, setSnapshot] = useState<{ key: string; catalog: AcpCommandCatalogVm | null } | null>(null);
  const subscriptionRef = useRef<Subscription | null>(null);
  useEffect(() => {
    if (!requestKey || !agentType || !workspacePath) return;
    const lease = subscribe(requestKey, agentType, workspacePath,
      catalog => setSnapshot({ key: requestKey, catalog }));
    subscriptionRef.current = lease.entry;
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    if (isTauriRuntime()) void listen<CatalogUpdate>('gold-band://agent-commands-updated', ({ payload }) => {
      if (disposed || (payload.agentType && payload.agentType !== agentType)
        || (payload.workspacePath && payload.workspacePath !== workspacePath)
        || (payload.projectId && lease.entry.catalog && payload.projectId !== lease.entry.catalog.projectId)) return;
      lease.entry.refresh();
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
      if (subscriptionRef.current === lease.entry) subscriptionRef.current = null;
      lease.release();
    };
  }, [requestKey, agentType, workspacePath]);
  // Skills live on disk and can change without any event; re-read when a menu opens.
  const revalidate = useCallback(() => subscriptionRef.current?.refresh(), []);

  const catalog = snapshot?.key === requestKey ? snapshot?.catalog : null;
  const cachedCommands = catalog?.commands ?? EMPTY_AGENT_COMMANDS;
  const scannedSkillCommands = catalog?.skillCommands ?? EMPTY_AGENT_COMMANDS;
  const fallbackCommands = sessionCommands == null ? cachedCommands : scannedSkillCommands;
  const commands = useMemo(
    () => mergeSlashCommandSources(sessionCommands, fallbackCommands),
    [fallbackCommands, sessionCommands],
  );

  return {
    catalogKey: requestKey,
    commands,
    revalidate,
  };
}
