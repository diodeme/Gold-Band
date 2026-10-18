import { describe, expect, it } from 'vitest';
import {
  buildSlashCatalog,
  committedRoleSnapshot,
  composerTextFromPromptRole,
  filterSlashCommands,
  getScrollTopForActiveSlashCommand,
  groupsForComposerMenuTrigger,
  clearSlashCommandDismissal,
  matchComposerMenuQuery,
  matchComposerMenuQueryAt,
  replaceComposerMenuQuery,
  matchSlashCommandQuery,
  mergeSlashCommandSources,
  parseCommittedSlashCommand,
  parseCommittedSlashItem,
  rememberSlashCommandDismissal,
  restoreSlashCommandDismissal,
  slashCommandText,
  slashSendableText,
  slashTagDisplayName,
  slashTokenFromName,
  unwrapSelectedSlashCommand,
} from '../src/lib/slash-command';

describe('slash command input contract', () => {
  it('opens only for a standalone slash query and accepts namespaced skills', () => {
    expect(matchSlashCommandQuery('/')).toBe('');
    expect(matchSlashCommandQuery('/ckm:design')).toBe('ckm:design');
    expect(matchSlashCommandQuery('/review.fix-v2')).toBe('review.fix-v2');
    expect(matchSlashCommandQuery('/测试')).toBe('测试');
  });

  it('closes after whitespace or punctuation separators', () => {
    expect(matchSlashCommandQuery('/review ')).toBeNull();
    expect(matchSlashCommandQuery('/review,')).toBeNull();
    expect(matchSlashCommandQuery('/review，')).toBeNull();
    expect(matchSlashCommandQuery('please /review')).toBeNull();
  });

  it('filters by command name and inserts ordinary ACP text', () => {
    const commands = [
      { name: 'ckm:design', description: 'Design' },
      { name: 'review', description: 'Review' },
    ];
    expect(filterSlashCommands(commands, 'DES')).toEqual([commands[0]]);
    expect(slashCommandText('/ckm:design')).toBe('/ckm:design ');
  });

  it('merges current session commands with a worktree skill scan', () => {
    const sessionCommands = [
      { name: 'review', description: 'Current ACP command', inputHint: 'target' },
      { name: 'status', description: 'Current session status' },
    ];
    const worktreeSkills = [
      { name: 'REVIEW', description: 'Scanned Skill metadata' },
      { name: 'project-skill', description: 'Visible in the worktree' },
    ];

    expect(mergeSlashCommandSources(sessionCommands, worktreeSkills)).toEqual([
      sessionCommands[0],
      sessionCommands[1],
      worktreeSkills[1],
    ]);
  });

  it('keeps scanned Skills available before a worktree command cache exists', () => {
    const worktreeSkills = [
      { name: 'project-skill', description: 'Visible in the worktree' },
    ];

    expect(mergeSlashCommandSources(null, worktreeSkills)).toEqual(worktreeSkills);
  });

  it('ignores malformed commands from the raw ACP session payload', () => {
    expect(mergeSlashCommandSources([
      null,
      { description: 'missing name' },
      { name: 42, description: 'invalid name' },
      { name: '/review', description: 42, inputHint: false },
    ], [])).toEqual([
      { name: 'review', description: '' },
    ]);
  });

  it('unwraps a newly selected command on the first Backspace without deleting its text', () => {
    const commands = [{ name: 'review', description: 'Review' }];

    expect(unwrapSelectedSlashCommand('/review ', commands, 1, 1)).toBe('/review');
    expect(unwrapSelectedSlashCommand('/review ', commands, 0, 0)).toBe('/review');
  });

  it('leaves ordinary Backspace behavior to the textarea after the command tag is unwrapped', () => {
    const commands = [{ name: 'review', description: 'Review' }];

    expect(unwrapSelectedSlashCommand('/review', commands, 7, 7)).toBeNull();
    expect(unwrapSelectedSlashCommand('/review fix', commands, 4, 4)).toBeNull();
    expect(unwrapSelectedSlashCommand('/review ', commands, 0, 1)).toBeNull();
  });

  it('decorates only a known leading command after a separator and preserves the raw suffix', () => {
    const commands = [
      { name: 'review', description: 'Review' },
      { name: 'ckm:design', description: 'Design' },
      { name: 'ckm:design-system', description: 'Design system' },
    ];
    expect(parseCommittedSlashCommand('/review fix this', commands)).toEqual({
      command: commands[0],
      prefix: '/review',
      suffix: ' fix this',
    });
    expect(parseCommittedSlashCommand('/CKM:DESIGN，调整页面', commands)).toEqual({
      command: commands[1],
      prefix: '/CKM:DESIGN',
      suffix: '，调整页面',
    });
    expect(parseCommittedSlashCommand('/review', commands)).toBeNull();
    expect(parseCommittedSlashCommand('/revie fix this', commands)).toBeNull();
    expect(parseCommittedSlashCommand('/unknown fix this', commands)).toBeNull();
    expect(parseCommittedSlashCommand('/ckm:design-system', commands)).toBeNull();
    expect(parseCommittedSlashCommand('/ckm:design-system ', commands)).toEqual({
      command: commands[2],
      prefix: '/ckm:design-system',
      suffix: ' ',
    });
  });

  it('never backtracks valid command punctuation into a separator', () => {
    const commands = [
      { name: 'ckm:design', description: 'Design' },
      { name: 'review.fix', description: 'Review fix' },
    ];
    expect(parseCommittedSlashCommand('/ckm:design-system', commands)).toBeNull();
    expect(parseCommittedSlashCommand('/review.fix-more', commands)).toBeNull();
    expect(parseCommittedSlashCommand('/ckm:design，继续', commands)).toEqual({
      command: commands[0],
      prefix: '/ckm:design',
      suffix: '，继续',
    });
  });

  it('keeps the active keyboard item inside the visible command viewport', () => {
    expect(getScrollTopForActiveSlashCommand({
      containerScrollTop: 0,
      containerHeight: 266,
      itemOffsetTop: 290,
      itemOffsetHeight: 36,
    })).toBe(60);
    expect(getScrollTopForActiveSlashCommand({
      containerScrollTop: 60,
      containerHeight: 266,
      itemOffsetTop: 38,
      itemOffsetHeight: 36,
    })).toBe(38);
    expect(getScrollTopForActiveSlashCommand({
      containerScrollTop: 38,
      containerHeight: 266,
      itemOffsetTop: 74,
      itemOffsetHeight: 36,
    })).toBe(38);
  });

  it('keeps dismissal across remounts until input or agent context changes', () => {
    const codexContext = 'codex-acp\nD:/workspace';
    const claudeContext = 'claude-acp\nD:/workspace';
    clearSlashCommandDismissal(codexContext);
    clearSlashCommandDismissal(claudeContext);

    rememberSlashCommandDismissal(codexContext, '/');
    expect(restoreSlashCommandDismissal(codexContext, '/', true)).toBe(true);
    expect(restoreSlashCommandDismissal(claudeContext, '/', true)).toBe(false);

    expect(restoreSlashCommandDismissal(codexContext, '/r', true)).toBe(false);
    expect(restoreSlashCommandDismissal(codexContext, '/', true)).toBe(false);
  });
});
describe('slash role catalog', () => {
  const profiles = [
    { id: 'pf-dev', name: 'Development and Testing', summary: 'short', content: '完整角色定义\n第二段' },
    { id: 'pf-cicd', name: 'CI/CD', summary: 'pipeline', content: 'CI role' },
  ];
  const commands = [
    { name: 'CI-CD', description: 'Agent CI command' },
    { name: 'review', description: 'Review' },
  ];

  it('keeps roles on @ without a brand heading and commands on / with the Agent heading', () => {
    const groups = buildSlashCatalog('Agent', profiles, commands);
    expect(groups.map((group) => group.id)).toEqual(['roles', 'agent']);
    expect(groups[0].heading).toBe('');
    expect(groups[1].heading).toBe('Agent');
    expect(groups[0].items.map((item) => item.name)).toEqual(['Development-and-Testing', 'CI-CD']);
    expect(matchComposerMenuQuery('@')).toEqual({ trigger: '@', query: '' });
    expect(matchComposerMenuQuery('@CI')).toEqual({ trigger: '@', query: 'CI' });
    expect(matchSlashCommandQuery('@CI')).toBeNull();
    expect(groupsForComposerMenuTrigger(groups, '/').map((group) => group.id)).toEqual(['agent']);
    expect(groupsForComposerMenuTrigger(groups, '@').map((group) => group.id)).toEqual(['roles']);
    expect(slashTokenFromName('CI/CD')).toBe('CI-CD');
    expect(slashTagDisplayName('/dataviz')).toBe('dataviz');
    expect(slashTagDisplayName('@开发')).toBe('开发');
  });

  it('keeps both catalog rows when a role and agent command share a token name', () => {
    const groups = buildSlashCatalog('Agent', profiles, commands);
    const names = groups.flatMap((group) => group.items.map((item) => `${item.kind}:${item.name}`));
    expect(names.filter((name) => name.endsWith(':CI-CD'))).toEqual(['role:CI-CD', 'command:CI-CD']);
  });

  it('commits @ to a role and / to an agent command when names collide', () => {
    const items = buildSlashCatalog('Agent', profiles, commands).flatMap((group) => group.items);
    expect(parseCommittedSlashItem('@CI-CD ', items)?.item).toMatchObject({
      kind: 'role',
      id: 'pf-cicd',
    });
    expect(parseCommittedSlashItem('/CI-CD ', items)?.item).toMatchObject({
      kind: 'command',
      id: 'CI-CD',
    });
  });

  it('sends the user suffix for a role tag and keeps agent command text intact', () => {
    const items = buildSlashCatalog('Agent', profiles, commands).flatMap((group) => group.items);
    const role = parseCommittedSlashItem('@CI-CD 帮我改代码', items);
    const command = parseCommittedSlashItem('/review 帮我改代码', items);
    expect(slashSendableText('@CI-CD 帮我改代码', role)).toBe(' 帮我改代码');
    expect(committedRoleSnapshot(role)).toEqual({
      profileId: 'pf-cicd',
      name: 'CI/CD',
      content: 'CI role',
    });
    expect(slashSendableText('/review 帮我改代码', command)).toBe('/review 帮我改代码');
    expect(committedRoleSnapshot(command)).toBeNull();
  });

  it('restores a composer tag prefix from the queued role snapshot', () => {
    expect(composerTextFromPromptRole({ name: '开发' }, '')).toBe('@开发 ');
    expect(composerTextFromPromptRole({ name: '开发' }, '爱仕达')).toBe('@开发 爱仕达');
    expect(composerTextFromPromptRole(null, '继续')).toBe('继续');
  });
});

describe('menu query before existing text', () => {
  it('reads the trigger from the start of the input up to the caret', () => {
    expect(matchComposerMenuQueryAt('@hello', 1)).toEqual({ trigger: '@', query: '' });
    expect(matchComposerMenuQueryAt('/rehello', 3)).toEqual({ trigger: '/', query: 're' });
    expect(matchComposerMenuQueryAt('hello @', 7)).toBeNull();
    expect(matchComposerMenuQueryAt('@hello', null)).toBeNull();
  });

  it('replaces only the query and keeps one separator before the remaining text', () => {
    expect(replaceComposerMenuQuery('/rehello', 3, '/review ')).toEqual({ input: '/review hello', caret: 8 });
    expect(replaceComposerMenuQuery('/re hello', 3, '/review ')).toEqual({ input: '/review hello', caret: 7 });
    expect(replaceComposerMenuQuery('@srchello', 4, '')).toEqual({ input: 'hello', caret: 0 });
    expect(replaceComposerMenuQuery('/re', 3, '/review ')).toEqual({ input: '/review ', caret: 8 });
  });
});
