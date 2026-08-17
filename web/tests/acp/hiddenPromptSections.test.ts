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
      { type: 'hidden', title: 'Gold Band runtime context', text: 'secret' },
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
      { type: 'hidden', title: 'A', text: 'one' },
      { type: 'visible', text: 'middle' },
      { type: 'hidden', title: 'B', text: 'two' },
    ]);
  });

  it('unescapes literal hidden closing tags inside hidden content', () => {
    const parts = parseGoldBandHiddenSections('<hidden data-gold-band-hidden="true">literal <\\/hidden></hidden>');

    expect(parts).toEqual([
      { type: 'hidden', title: undefined, text: 'literal </hidden>' },
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
