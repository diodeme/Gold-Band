import { describe, expect, it } from 'vitest';
import {
  filterSlashCommands,
  getScrollTopForActiveSlashCommand,
  clearSlashCommandDismissal,
  matchSlashCommandQuery,
  parseCommittedSlashCommand,
  rememberSlashCommandDismissal,
  restoreSlashCommandInputFocus,
  restoreSlashCommandDismissal,
  slashCommandText,
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

  it('restores composer focus after command selection has committed', () => {
    let scheduled: (() => void) | null = null;
    let focusCount = 0;
    let selection: [number, number] | null = null;
    const input = {
      disabled: false,
      value: 'fix this',
      focus: () => { focusCount += 1; },
      setSelectionRange: (start: number, end: number) => { selection = [start, end]; },
    };

    restoreSlashCommandInputFocus(
      { current: input },
      (callback) => { scheduled = callback; },
    );

    expect(focusCount).toBe(0);
    expect(scheduled).not.toBeNull();
    scheduled?.();
    expect(focusCount).toBe(1);
    expect(selection).toEqual([8, 8]);
  });

  it('does not restore focus when the composer becomes disabled', () => {
    let scheduled: (() => void) | null = null;
    let focusCount = 0;
    const input = {
      disabled: false,
      focus: () => { focusCount += 1; },
    };

    restoreSlashCommandInputFocus(
      { current: input },
      (callback) => { scheduled = callback; },
    );
    input.disabled = true;
    scheduled?.();

    expect(focusCount).toBe(0);
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
