import { describe, expect, it } from 'vitest';

import { promptInputTextareaSize } from '@/components/prompt-kit/prompt-input';
import { CONVERSATION_HOME_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';

describe('conversation composer autosize contract', () => {
  it('uses the narrower home layout and a compact initial textarea', () => {
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT).toMatchObject({
      contentMaxWidthClassName: 'max-w-3xl',
      opticalBottomPaddingClassName: 'pb-[clamp(4rem,8vh,5rem)]',
      textareaMinHeightClassName: 'min-h-14',
      textareaMaxHeightPx: 320,
    });
  });

  it('grows with content without showing an internal scrollbar below the cap', () => {
    expect(promptInputTextareaSize(56, 320)).toEqual({ height: '56px', overflowY: 'hidden' });
    expect(promptInputTextareaSize(216, 320)).toEqual({ height: '216px', overflowY: 'hidden' });
  });

  it('stops growing and enables internal scrolling after the cap', () => {
    expect(promptInputTextareaSize(480, 320)).toEqual({ height: '320px', overflowY: 'auto' });
  });

});
