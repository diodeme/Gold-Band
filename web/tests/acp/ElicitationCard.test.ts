import { describe, expect, it } from 'vitest';

import { stepMessage } from '../../src/components/acp/ElicitationCard';

describe('ElicitationCard question text', () => {
  it('uses the schema field title as the visible question', () => {
    expect(
      stepMessage(
        'Please answer the following questions.',
        '你希望项目主要面向哪个方向？',
        0,
        '请选择一个答案',
      ),
    ).toBe('你希望项目主要面向哪个方向？');
  });

  it('does not show generic provider prompt text as the question', () => {
    expect(
      stepMessage(
        'Please answer the following questions.',
        undefined,
        0,
        '请选择一个答案',
      ),
    ).toBe('请选择一个答案');
  });

  it('uses the matching line for multi-step messages', () => {
    expect(
      stepMessage('第一题\n第二题', undefined, 1, '请选择一个答案'),
    ).toBe('第二题');
  });
});
