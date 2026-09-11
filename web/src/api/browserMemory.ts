import { memoryEntryValid, type MemoryCommand, type MemorySnapshot } from '@/lib/memory';

const projects = new Map<string, MemorySnapshot>();
export async function readBrowserMemory(projectId: string): Promise<MemorySnapshot> {
  if (!projects.has(projectId)) projects.set(projectId, {
    projectId, taskId: null, workspacePath: `preview/projects/${projectId}/memory.json`, taskPath: null,
    workspace: [], task: [], effective: [], limits: { entries: 100, key: 128, value: 4000, desc: 500, effectiveBytes: 32768 },
  });
  return structuredClone(projects.get(projectId)!);
}
export async function writeBrowserMemory(projectId: string, command: MemoryCommand): Promise<MemorySnapshot> {
  await readBrowserMemory(projectId);
  const data = structuredClone(projects.get(projectId)!);
  if (command.scope !== 'workspace' || (command.entry && !memoryEntryValid(command.entry, data.limits))) throw { code: 'memory.field' };
  const latest = data.workspace.find(row => row.key === command.key);
  if ((latest?.revision ?? null) !== command.expectedRevision) throw { code: 'memory.conflict', params: { key: command.key, latest } };
  if (command.entry && command.entry.key !== command.key && data.workspace.some(row => row.key === command.entry!.key)) throw { code: 'memory.conflict' };
  data.workspace = data.workspace.filter(row => row.key !== command.key);
  if (command.entry) data.workspace.push({ ...command.entry, revision: crypto.randomUUID() });
  data.effective = data.workspace.map(({ revision: _, ...entry }) => ({ ...entry, scope: 'workspace' }));
  if (data.workspace.length > data.limits.entries || new TextEncoder().encode(JSON.stringify(data.effective)).length > data.limits.effectiveBytes) throw { code: 'memory.capacity' };
  projects.set(projectId, data);
  return structuredClone(data);
}
