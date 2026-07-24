import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

import type { ElicitationPropertySchema } from '../../src/components/acp/ElicitationCard';
import { elicitationOptions, elicitationQuestionText } from '../../src/components/acp/ElicitationCard';

describe('ElicitationCard question text', () => {
  it('uses a high-contrast semantic treatment for selected options', () => {
    const source = readFileSync(
      path.resolve(__dirname, '../../src/components/acp/ElicitationCard.tsx'),
      'utf8',
    );

    expect(source).toContain(
      'border-accent-foreground/55 bg-accent text-accent-foreground shadow-[inset_3px_0_0_var(--accent-foreground)]',
    );
    expect(source).toContain('border-accent-foreground bg-accent-foreground text-background');
    expect(source).not.toContain('bg-accent-foreground text-accent');
    expect(source).toContain('aria-pressed={sel}');
    expect(source).toContain('aria-pressed={selected}');
    expect(source).not.toContain('border-primary bg-primary/5');
    expect(source).not.toContain('text-primary shrink-0');
  });

  it('uses the schema field description as the visible question', () => {
    expect(
      elicitationQuestionText(
        'Please answer the following questions.',
        '你学习这个 Claude Code 源码项目到现在，最让你印象深刻的模块是哪个？',
        0,
        '请选择一个答案',
      ),
    ).toBe('你学习这个 Claude Code 源码项目到现在，最让你印象深刻的模块是哪个？');
  });

  it('does not show generic provider prompt text as the question', () => {
    expect(
      elicitationQuestionText(
        'Please answer the following questions.',
        undefined,
        0,
        '请选择一个答案',
      ),
    ).toBe('请选择一个答案');
  });

  it('uses the matching line for multi-step messages', () => {
    expect(
      elicitationQuestionText('第一题\n第二题', undefined, 1, '请选择一个答案'),
    ).toBe('第二题');
  });

  it('uses the request message when schema only provides a short title', () => {
    expect(
      elicitationQuestionText(
        '除了打印问候语，你还希望这个小脚本涵盖哪些功能？（可多选）',
        undefined,
        0,
        '请选择一个答案',
      ),
    ).toBe('除了打印问候语，你还希望这个小脚本涵盖哪些功能？（可多选）');
  });

  it('recognizes array questions with items.anyOf as multi-select schema', () => {
    const property: ElicitationPropertySchema = {
      type: 'array',
      title: '功能组合',
      items: {
        anyOf: [
          { const: '交互问候', title: '交互问候 — 读取用户输入并个性化回复' },
          { const: '时间戳', title: '时间戳 — 输出当前时间戳' },
        ],
      },
    };

    expect(elicitationOptions(property)).toEqual([
      { value: '交互问候', label: '交互问候 — 读取用户输入并个性化回复' },
      { value: '时间戳', label: '时间戳 — 输出当前时间戳' },
    ]);
    expect(property.items?.anyOf?.map((option) => option.const)).toEqual([
      '交互问候',
      '时间戳',
    ]);
  });
});
