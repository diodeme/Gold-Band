import type { SkillContentVm, SkillMetaVm, SupportedAgentTypeVm } from '@/types';

export type SkillSheetMode = 'view' | 'create' | 'edit';

export interface SkillFormState {
  name: string;
  description: string;
  body: string;
  source: string;
}

export interface SkillSaveRequest {
  name: string;
  scope: string;
  wsPath: string | null;
  content: string;
  oldName: string | null;
  directoryPath: string | null;
  syncTargets: string[];
}

export function createEmptySkillForm(source: string): SkillFormState {
  return { name: '', description: '', body: '', source };
}

export function createSkillFormFromContent(
  content: SkillContentVm | null | undefined,
  fallbackSource: string,
): SkillFormState {
  return {
    name: content?.meta.name ?? '',
    description: content?.meta.description ?? '',
    body: content?.body ?? '',
    source: (content?.meta.source as string | undefined) ?? fallbackSource,
  };
}

export function filterSkillSyncTargets(
  current: string[],
  availableAgents: SupportedAgentTypeVm[],
) {
  const available = new Set(availableAgents.map((agent) => agent.agentType));
  return current.filter((agentType) => available.has(agentType));
}

export function buildSkillSaveRequest(input: {
  form: SkillFormState;
  mode: Exclude<SkillSheetMode, 'view'>;
  editTarget: SkillMetaVm | null;
  editWorkspacePath: string | null;
  syncTargets: string[];
}): SkillSaveRequest {
  const scope = input.form.source.startsWith('project:') ? 'project' : input.form.source;
  const wsPath = input.mode === 'edit'
    ? input.editWorkspacePath
    : (input.form.source.startsWith('project:') ? input.form.source.slice(8) : null);
  const name = input.form.name.trim();
  return {
    name,
    scope,
    wsPath,
    content: `---\nname: ${name}\ndescription: ${input.form.description.trim()}\n---\n\n${input.form.body}`,
    oldName: input.mode === 'edit' ? input.editTarget?.name ?? null : null,
    directoryPath: input.mode === 'edit' ? input.editTarget?.directoryPath ?? null : null,
    syncTargets: input.syncTargets,
  };
}
