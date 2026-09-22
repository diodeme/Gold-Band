import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

import { promptInputTextareaSize } from '@/components/prompt-kit/prompt-input';
import { CONVERSATION_HOME_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';

const composerSource = readFileSync(
  new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url),
  'utf-8',
);

describe('conversation composer autosize contract', () => {
  it('uses the narrower home layout and a compact initial textarea', () => {
    expect(CONVERSATION_HOME_COMPOSER_LAYOUT).toMatchObject({
      contentMaxWidthClassName: 'max-w-3xl',
      opticalBottomPaddingClassName: 'pb-[clamp(4rem,8vh,5rem)]',
      promptInputClassName: 'relative rounded-2xl border-border bg-card/60 px-2.5 py-2 shadow-sm',
      textareaClassName: 'min-h-12 py-2 text-sm leading-6 text-foreground placeholder:text-muted-foreground w-full overflow-y-hidden px-0',
      // 滚动收口在包装层（max-h-80 = 320px），不在 textarea 自身。
      inputScrollContainerClassName: 'relative min-w-0 max-h-80 overflow-y-auto',
    });
  });

  it('grows with content without showing an internal scrollbar below the cap', () => {
    expect(promptInputTextareaSize(56, 320)).toEqual({ height: '56px', overflowY: 'hidden' });
    expect(promptInputTextareaSize(216, 320)).toEqual({ height: '216px', overflowY: 'hidden' });
  });

  it('stops growing and enables internal scrolling after the cap', () => {
    expect(promptInputTextareaSize(480, 320)).toEqual({ height: '320px', overflowY: 'auto' });
  });

  it('null maxHeight means uncapped autosize: the textarea never scrolls itself', () => {
    expect(promptInputTextareaSize(56, null)).toEqual({ height: '56px', overflowY: 'hidden' });
    expect(promptInputTextareaSize(480, null)).toEqual({ height: '480px', overflowY: 'hidden' });
  });

  it('delegates composer scrolling to the wrapper so the chip scrolls with the first line', () => {
    // 会话主页 composer：textarea 不再自带 320px 上限，滚动统一由包装层接管，
    // multica chip / 斜杠标签（绝对定位在包装层内）随正文首行滚出视野，不再悬浮遮挡。
    expect(composerSource).toContain('maxHeight={null}');
    expect(composerSource).not.toContain('textareaMaxHeightPx');
    expect(composerSource).toContain(
      '<div className={CONVERSATION_HOME_COMPOSER_LAYOUT.inputScrollContainerClassName}>',
    );
    // chip 与斜杠标签的绝对定位 span 必须位于滚动包装层内部（在其开标签之后渲染）。
    // 两类 leading adornment 统一使用 main 引入的 COMPOSER_LEADING_ADORNMENT_SLOT_CLASS_NAME
    // 槽位常量（absolute z-10 inline-flex … + left-0 top-2），不再各自硬编码类名字符串。
    const wrapperIndex = composerSource.indexOf(
      'CONVERSATION_HOME_COMPOSER_LAYOUT.inputScrollContainerClassName',
    );
    const chipIndex = composerSource.indexOf(
      '${COMPOSER_LEADING_ADORNMENT_SLOT_CLASS_NAME} left-0 top-2',
    );
    expect(wrapperIndex).toBeGreaterThan(-1);
    expect(chipIndex).toBeGreaterThan(wrapperIndex);
  });
});
