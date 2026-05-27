import { invoke } from '@tauri-apps/api/core';
import type { AppErrorVm, RoundSelection } from '../types';

export function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export function invokeCommand<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(name, args).catch((error) => Promise.reject(normalizeCommandError(error)));
}

export function normalizeCommandError(error: unknown): unknown {
  const direct = asCommandError(error);
  if (direct) {
    return direct;
  }
  if (error instanceof Error) {
    return parseCommandErrorString(error.message) ?? error;
  }
  if (typeof error === 'string') {
    return parseCommandErrorString(error) ?? error;
  }
  if (error && typeof error === 'object') {
    const candidate = error as {
      message?: unknown;
      error?: unknown;
      cause?: unknown;
      payload?: unknown;
    };
    for (const value of [candidate.error, candidate.cause, candidate.payload, candidate.message]) {
      const normalized = normalizeCommandError(value);
      if (asCommandError(normalized)) {
        return normalized;
      }
    }
  }
  return error;
}

export function localTimestamp(date = new Date()) {
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function toRoundSelectionInput(selection?: RoundSelection) {
  if (!selection) return selection;
  if (selection.kind === 'round' || selection.kind === 'requirement') return { kind: selection.kind, context_node_id: selection.contextNodeId };
  if (selection.kind === 'event' || selection.kind === 'log') return { kind: selection.kind, id: selection.id, node_id: selection.nodeId, attempt_id: selection.attemptId, context_node_id: selection.contextNodeId };
  if (selection.kind === 'node') return { kind: selection.kind, node_id: selection.nodeId, attempt_id: selection.attemptId, context_node_id: selection.contextNodeId };
  if (selection.kind === 'worker-ref') return { kind: selection.kind, node_id: selection.nodeId, attempt_id: selection.attemptId, context_node_id: selection.contextNodeId };
  return { kind: selection.kind, node_id: selection.nodeId, attempt_id: selection.attemptId, name: selection.name, context_node_id: selection.contextNodeId };
}

function parseCommandErrorString(value: string) {
  const trimmed = value.trim();
  const direct = parseCommandErrorJson(trimmed);
  if (direct) {
    return direct;
  }
  const start = trimmed.indexOf('{');
  const end = trimmed.lastIndexOf('}');
  if (start >= 0 && end > start) {
    return parseCommandErrorJson(trimmed.slice(start, end + 1));
  }
  return null;
}

function parseCommandErrorJson(value: string) {
  try {
    return asCommandError(JSON.parse(value));
  } catch {
    return null;
  }
}

function asCommandError(value: unknown): AppErrorVm | null {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const candidate = value as { code?: unknown; params?: unknown };
  if (typeof candidate.code !== 'string') {
    return null;
  }
  return {
    code: candidate.code,
    params: candidate.params && typeof candidate.params === 'object' ? candidate.params as Record<string, unknown> : {},
  };
}
