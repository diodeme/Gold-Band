export interface MemoryEntry { key: string; value: string; desc: string }
export interface MemoryRecord extends MemoryEntry { revision: string }
export interface MemoryCommand {
  scope: 'workspace' | 'task'; key: string; expectedRevision: string | null; entry: MemoryEntry | null;
}
export interface MemorySnapshot {
  projectId: string; taskId: string | null; workspacePath: string; taskPath: string | null;
  workspace: MemoryRecord[]; task: MemoryRecord[];
  effective: (MemoryEntry & { scope: 'workspace' | 'task' })[];
  limits: { entries: number; key: number; value: number; desc: number; effectiveBytes: number };
}
export interface MemoryError { code: string; params?: { path?: string; latest?: MemoryRecord | null; key?: string } }
export function memoryError(error: unknown): MemoryError {
  return error && typeof error === 'object' && 'code' in error ? error as MemoryError : { code: 'memory.io' };
}
export function memoryEntryValid(entry: MemoryEntry, limits: MemorySnapshot['limits']) {
  return Boolean(entry.key.trim()) && (['key', 'value', 'desc'] as const).every(field => [...entry[field]].length <= limits[field]);
}
