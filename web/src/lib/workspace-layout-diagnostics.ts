const WORKSPACE_LAYOUT_DIAGNOSTIC_LIMIT = 2_000;
export const WORKSPACE_LAYOUT_DEBUG_STORAGE_KEY = 'goldBand.debug.workspaceLayout';

export type WorkspaceLayoutDiagnosticStage =
  | 'auto-collapse-evaluated'
  | 'group-layout-sync'
  | 'group-layout-change'
  | 'group-layout-changed'
  | 'presentation-committed'
  | 'user-resize-intent';

export interface WorkspaceLayoutDiagnosticRecord {
  sequence: number;
  recordedAt: string;
  elapsedMs: number | null;
  stage: WorkspaceLayoutDiagnosticStage;
  details: Record<string, unknown>;
}

type WorkspaceLayoutDiagnosticBridge = {
  clear: () => void;
  copy: () => Promise<string>;
  exportJson: () => string;
  snapshot: () => WorkspaceLayoutDiagnosticRecord[];
};

declare global {
  interface Window {
    __goldBandWorkspaceLayoutDiagnostics?: WorkspaceLayoutDiagnosticBridge;
  }
}

export function createWorkspaceLayoutDiagnosticBuffer(limit: number) {
  const capacity = Math.max(1, Math.floor(limit));
  const records: WorkspaceLayoutDiagnosticRecord[] = [];
  return {
    append(record: WorkspaceLayoutDiagnosticRecord) {
      records.push(record);
      if (records.length > capacity) records.splice(0, records.length - capacity);
    },
    clear() {
      records.splice(0, records.length);
    },
    snapshot() {
      return records.map((record) => ({
        ...record,
        details: cloneDiagnosticDetails(record.details),
      }));
    },
  };
}

const diagnosticBuffer = createWorkspaceLayoutDiagnosticBuffer(
  WORKSPACE_LAYOUT_DIAGNOSTIC_LIMIT,
);
let diagnosticSequence = 0;
const workspaceLayoutDiagnosticsEnabled = readWorkspaceLayoutDiagnosticsEnabled();

export function isWorkspaceLayoutDiagnosticsEnabled() {
  return workspaceLayoutDiagnosticsEnabled;
}

function readWorkspaceLayoutDiagnosticsEnabled() {
  if (typeof window === 'undefined') return false;
  try {
    return window.localStorage.getItem(WORKSPACE_LAYOUT_DEBUG_STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

export function recordWorkspaceLayoutDiagnostic(
  stage: WorkspaceLayoutDiagnosticStage,
  createDetails: () => Record<string, unknown>,
) {
  if (!isWorkspaceLayoutDiagnosticsEnabled()) return;
  installWorkspaceLayoutDiagnosticBridge();
  diagnosticBuffer.append({
    sequence: ++diagnosticSequence,
    recordedAt: new Date().toISOString(),
    elapsedMs: typeof performance === 'undefined' ? null : Math.round(performance.now()),
    stage,
    details: createDetails(),
  });
}

export function exportWorkspaceLayoutDiagnostics() {
  return JSON.stringify({
    version: 1,
    exportedAt: new Date().toISOString(),
    records: diagnosticBuffer.snapshot(),
  }, null, 2);
}

export async function copyWorkspaceLayoutDiagnostics() {
  const json = exportWorkspaceLayoutDiagnostics();
  console.info(`[GoldBand][Workspace layout diagnostics]\n${json}`);
  try {
    await navigator.clipboard.writeText(json);
  } catch (clipboardError) {
    if (!copyWorkspaceLayoutDiagnosticsFallback(json)) throw clipboardError;
  }
  return json;
}

export function installWorkspaceLayoutDiagnosticShortcut(target: Window = window) {
  if (!isWorkspaceLayoutDiagnosticsEnabled()) return () => undefined;
  installWorkspaceLayoutDiagnosticBridge();
  const onKeyDown = (event: KeyboardEvent) => {
    if (
      event.code !== 'KeyL'
      || !event.ctrlKey
      || !event.altKey
      || !event.shiftKey
      || event.metaKey
    ) return;
    event.preventDefault();
    void copyWorkspaceLayoutDiagnostics().catch((error: unknown) => {
      console.error('[GoldBand][Workspace layout diagnostics] copy failed', error);
    });
  };
  target.addEventListener('keydown', onKeyDown, { capture: true });
  return () => target.removeEventListener('keydown', onKeyDown, { capture: true });
}

function installWorkspaceLayoutDiagnosticBridge() {
  if (typeof window === 'undefined' || window.__goldBandWorkspaceLayoutDiagnostics) return;
  window.__goldBandWorkspaceLayoutDiagnostics = {
    clear: () => diagnosticBuffer.clear(),
    snapshot: () => diagnosticBuffer.snapshot(),
    exportJson: exportWorkspaceLayoutDiagnostics,
    copy: copyWorkspaceLayoutDiagnostics,
  };
}

function cloneDiagnosticDetails(details: Record<string, unknown>) {
  try {
    return JSON.parse(JSON.stringify(details)) as Record<string, unknown>;
  } catch {
    return { serializationError: true };
  }
}

function copyWorkspaceLayoutDiagnosticsFallback(json: string) {
  if (typeof document === 'undefined' || typeof document.execCommand !== 'function') return false;
  const textarea = document.createElement('textarea');
  textarea.value = json;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.append(textarea);
  textarea.select();
  try {
    return document.execCommand('copy');
  } finally {
    textarea.remove();
  }
}
