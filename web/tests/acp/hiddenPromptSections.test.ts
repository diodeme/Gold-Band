import { describe, expect, it } from 'vitest';
import { visiblePromptText } from '../../src/components/acp/HiddenPromptMessageContent';
import { parseGoldBandHiddenSections } from '../../src/components/acp/hiddenPromptSections';

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
    expect(visiblePromptText('\r\n\n# Requirement', false)).toBe('\r\n\n# Requirement');
  });
});
