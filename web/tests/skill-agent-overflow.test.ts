import { describe, expect, it } from 'vitest';

import { calculateSkillAgentOverflowLayout } from '../src/lib/skill-agent-overflow';

describe('skill agent overflow layout', () => {
  it('shows all agents while they fit on one row', () => {
    expect(calculateSkillAgentOverflowLayout(128, 0, 5)).toEqual({
      visibleSourceCount: 0,
      visibleSyncCount: 5,
      hiddenCount: 0,
    });
  });

  it('uses the second row before introducing an overflow trigger', () => {
    expect(calculateSkillAgentOverflowLayout(132, 0, 10)).toEqual({
      visibleSourceCount: 0,
      visibleSyncCount: 10,
      hiddenCount: 0,
    });
  });

  it('reserves the last two-row slot for the overflow trigger', () => {
    expect(calculateSkillAgentOverflowLayout(132, 0, 11)).toEqual({
      visibleSourceCount: 0,
      visibleSyncCount: 9,
      hiddenCount: 2,
    });
  });

  it('keeps the source-to-sync divider attached to the first sync agent', () => {
    expect(calculateSkillAgentOverflowLayout(84, 1, 3)).toEqual({
      visibleSourceCount: 1,
      visibleSyncCount: 3,
      hiddenCount: 0,
    });
  });
});
