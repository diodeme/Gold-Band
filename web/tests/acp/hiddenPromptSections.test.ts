import { describe, expect, it } from 'vitest';
import { projectHiddenPromptDisplayParts, visiblePromptText } from '../../src/components/acp/HiddenPromptMessageContent';
import {
  parseGoldBandHiddenSections,
  resolveGoldBandHiddenSection,
} from '../../src/components/acp/hiddenPromptSections';

describe('Gold Band hidden prompt sections', () => {
  it('splits visible and Gold Band hidden sections in order', () => {
    const parts = parseGoldBandHiddenSections('visible\n<hidden data-gold-band-hidden="true" title="Gold Band runtime context">secret</hidden>\nnext');

    expect(parts).toEqual([
      { type: 'visible', text: 'visible\n' },
      { type: 'hidden', title: 'Gold Band runtime context', text: 'secret', show: true },
      { type: 'visible', text: '\nnext' },
    ]);
  });

  it('keeps ordinary hidden tags visible', () => {
    const content = 'before <hidden>not gold band</hidden> after';

    expect(parseGoldBandHiddenSections(content)).toEqual([
      { type: 'visible', text: content },
    ]);
  });

  it('keeps malformed Gold Band hidden tags visible', () => {
    const content = 'before <hidden data-gold-band-hidden="true">missing close';

    expect(parseGoldBandHiddenSections(content)).toEqual([
      { type: 'visible', text: content },
    ]);
  });

  it('keeps multiple hidden sections ordered', () => {
    const parts = parseGoldBandHiddenSections('<hidden data-gold-band-hidden="true" title="A">one</hidden>middle<hidden data-gold-band-hidden="true" title="B">two</hidden>');

    expect(parts).toEqual([
      { type: 'hidden', title: 'A', text: 'one', show: true },
      { type: 'visible', text: 'middle' },
      { type: 'hidden', title: 'B', text: 'two', show: true },
    ]);
  });

  it('unescapes literal hidden closing tags inside hidden content', () => {
    const parts = parseGoldBandHiddenSections('<hidden data-gold-band-hidden="true">literal <\\/hidden></hidden>');

    expect(parts).toEqual([
      { type: 'hidden', title: undefined, text: 'literal </hidden>', show: true },
    ]);
  });

  it('trims display-only blank lines after hidden sections', () => {
    expect(visiblePromptText('\r\n\n# Requirement', true)).toBe('# Requirement');
    expect(visiblePromptText('  \r\n\t\n# Requirement', true)).toBe('# Requirement');
    expect(visiblePromptText('\r\n\n# Requirement', false)).toBe('\r\n\n# Requirement');
  });

  it('coalesces visible fragments after grouped hidden sections without spacer rows', () => {
    const display = projectHiddenPromptDisplayParts(parseGoldBandHiddenSections([
      '<hidden data-gold-band-hidden="true" title="Gold Band stable system prompt">system</hidden>',
      '',
      '<hidden data-gold-band-hidden="true" title="Gold Band runtime context">runtime</hidden>',
      '',
      '# Requirement',
      'hi',
    ].join('\n')));

    expect(display.map(({ part }) => part.type)).toEqual(['hidden', 'hidden', 'visible']);
    expect(display[2]?.part).toEqual({ type: 'visible', text: '# Requirement\nhi' });
  });

  it('parses show=false for audit access but omits that section from the message projection', () => {
    const parts = parseGoldBandHiddenSections([
      '用户消息',
      '<hidden data-gold-band-hidden="true" show="false" title="Gold Band runtime control">resume</hidden>',
    ].join('\n'));

    expect(parts[1]).toEqual({
      type: 'hidden',
      title: 'Gold Band runtime control',
      text: 'resume',
      show: false,
    });
    expect(projectHiddenPromptDisplayParts(parts)).toEqual([{
      part: { type: 'visible', text: '用户消息\n' },
      sourceIndex: 2,
    }]);
  });

  it('resolves a hidden section only from the exact canonical event revision and part index', () => {
    const events = [{
      id: 'prompt-1',
      seq: 21,
      endedSeq: 23,
      timestamp: '2026-08-17T10:00:00Z',
      kind: 'userTextDelta',
      content: [
        '<hidden data-gold-band-hidden="true" title="Gold Band stable system prompt">system</hidden>',
        '<hidden data-gold-band-hidden="true" title="Gold Band runtime context">runtime</hidden>',
        '# Requirement',
      ].join('\n'),
    }];

    expect(resolveGoldBandHiddenSection(events, {
      eventId: 'prompt-1',
      eventSeq: 23,
      partIndex: 2,
    })).toEqual({
      type: 'hidden',
      title: 'Gold Band runtime context',
      text: 'runtime',
      show: true,
    });
    expect(resolveGoldBandHiddenSection(events, {
      eventId: 'prompt-1',
      eventSeq: 22,
      partIndex: 2,
    })).toBeNull();
    expect(resolveGoldBandHiddenSection(events, {
      eventId: 'prompt-2',
      eventSeq: 23,
      partIndex: 2,
    })).toBeNull();
  });
});
