import { describe, expect, it } from 'vitest';

import {
  buildSkillSaveRequest,
  createEmptySkillForm,
  createSkillFormFromContent,
  filterSkillSyncTargets,
} from '../src/lib/skill-sheet-form';
import type { SkillContentVm, SkillMetaVm, SupportedAgentTypeVm } from '../src/types';

function skill(overrides: Partial<SkillMetaVm> = {}): SkillMetaVm {
  return {
    name: 'demo',
    description: 'Demo skill',
    source: 'project',
    directoryPath: 'D:/repo/.gold-band/skills/demo',
    agentSource: '.gold-band',
    loadWarnings: [],
    syncedAgentTypes: [],
    ...overrides,
  };
}

function content(overrides: Partial<SkillContentVm> = {}): SkillContentVm {
  return {
    meta: skill(),
    body: 'body',
    ...overrides,
  };
}

const agents: SupportedAgentTypeVm[] = [
  { agentType: 'claude-acp', label: 'Claude', iconKey: 'claude', primaryAgentDir: '.claude', compatibleAgentDirs: [], supported: true, configured: true, defaultDisplayName: 'Claude', defaultCommand: 'npx', defaultArgs: [], defaultEnv: [] },
  { agentType: 'codex-acp', label: 'Codex', iconKey: 'codex', primaryAgentDir: '.codex', compatibleAgentDirs: ['.agents'], supported: true, configured: true, defaultDisplayName: 'Codex', defaultCommand: 'npx', defaultArgs: [], defaultEnv: [] },
];

describe('skill sheet form helpers', () => {
  it('creates an empty local draft for new skills', () => {
    expect(createEmptySkillForm('project:D:/repo')).toEqual({
      name: '',
      description: '',
      body: '',
      source: 'project:D:/repo',
    });
  });

  it('hydrates the local draft from loaded skill content', () => {
    expect(createSkillFormFromContent(content({
      meta: skill({ name: 'loaded', description: 'Loaded', source: 'global' }),
      descriptionSource: 'Loaded\nfrom folded block',
      body: 'loaded body',
    }), 'project')).toEqual({
      name: 'loaded',
      description: 'Loaded\nfrom folded block',
      body: 'loaded body',
      source: 'global',
    });
  });

  it('builds create requests from project-scoped source values', () => {
    expect(buildSkillSaveRequest({
      form: { name: ' demo ', description: ' Desc ', body: 'content', source: 'project:D:/repo' },
      mode: 'create',
      editTarget: null,
      editWorkspacePath: null,
      syncTargets: ['claude-acp'],
    })).toEqual({
      name: 'demo',
      scope: 'project',
      wsPath: 'D:/repo',
      content: '---\nname: demo\ndescription: Desc\n---\n\ncontent',
      oldName: null,
      directoryPath: null,
      syncTargets: ['claude-acp'],
    });
  });

  it('builds edit requests from the original target identity', () => {
    const target = skill({ name: 'old', directoryPath: 'D:/repo/.claude/skills/old' });
    expect(buildSkillSaveRequest({
      form: { name: 'new', description: 'Next', body: 'next body', source: 'project' },
      mode: 'edit',
      editTarget: target,
      editWorkspacePath: 'D:/repo',
      syncTargets: ['codex-acp'],
    })).toEqual({
      name: 'new',
      scope: 'project',
      wsPath: 'D:/repo',
      content: '---\nname: new\ndescription: Next\n---\n\nnext body',
      oldName: 'old',
      directoryPath: 'D:/repo/.claude/skills/old',
      syncTargets: ['codex-acp'],
    });
  });

  it('encodes multiline descriptions as frontmatter block scalars', () => {
    expect(buildSkillSaveRequest({
      form: { name: 'demo', description: 'Line one\nLine two', body: 'body', source: 'global' },
      mode: 'create',
      editTarget: null,
      editWorkspacePath: null,
      syncTargets: [],
    }).content).toBe('---\nname: demo\ndescription: |\n  Line one\n  Line two\n---\n\nbody');
  });

  it('filters stale sync targets when configured agents change', () => {
    expect(filterSkillSyncTargets(['claude-acp', 'missing'], agents)).toEqual(['claude-acp']);
  });
});
