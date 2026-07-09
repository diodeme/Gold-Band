import { describe, expect, it } from 'vitest';

import { skillStorageHint } from '../src/lib/skill-storage-hint';

describe('skillStorageHint', () => {
  it('uses .gold-band defaults when creating a global skill', () => {
    expect(skillStorageHint({
      source: 'global',
      editing: false,
    })).toBe('Available across every project. Saved to ~/.gold-band/skills/<name>/SKILL.md');
  });

  it('uses .gold-band defaults when creating a project skill', () => {
    expect(skillStorageHint({
      source: 'project:E:\\AI_PROJECT\\Gold-Band',
      editing: false,
    })).toBe('Project-level. Saved to <project>/.gold-band/skills/<name>/SKILL.md');
  });

  it('allows UI copy to be localized by the caller', () => {
    expect(skillStorageHint({
      source: 'project',
      editing: true,
      directoryPath: 'E:\\AI_PROJECT\\Gold-Band\\.gold-band\\skills\\test-skill',
      workspacePath: 'E:\\AI_PROJECT\\Gold-Band',
      translate: (key, params) => {
        if (key === 'contextManagement.skills.storageProject') {
          return `项目级。保存到 ${params?.path ?? ''}`;
        }
        return params?.path ?? '';
      },
    })).toBe('项目级。保存到 <project>/.gold-band/skills/test-skill/SKILL.md');
  });

  it('shows the actual project agent-native path while editing', () => {
    expect(skillStorageHint({
      source: 'project',
      editing: true,
      directoryPath: 'E:\\AI_PROJECT\\Gold-Band\\.claude\\skills\\native-skill',
      workspacePath: 'E:\\AI_PROJECT\\Gold-Band',
    })).toBe('Project-level. Saved to <project>/.claude/skills/native-skill/SKILL.md');
  });

  it('falls back to the actual absolute path for global native skills', () => {
    expect(skillStorageHint({
      source: 'global',
      editing: true,
      directoryPath: 'C:\\Users\\stevendeng\\.claude\\skills\\native-skill',
    })).toBe('Available across every project. Saved to C:/Users/stevendeng/.claude/skills/native-skill/SKILL.md');
  });

  it('keeps custom external project directories visible as absolute paths', () => {
    expect(skillStorageHint({
      source: 'project',
      editing: true,
      directoryPath: 'D:\\custom-agent-home\\skills\\native-skill',
      workspacePath: 'E:\\AI_PROJECT\\Gold-Band',
    })).toBe('Project-level. Saved to D:/custom-agent-home/skills/native-skill/SKILL.md');
  });
});
