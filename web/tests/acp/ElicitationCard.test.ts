import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

import type {
  ElicitationField,
  ElicitationPropertySchema,
} from '../../src/components/acp/ElicitationCard';
import {
  buildElicitationContent,
  clearElicitationFieldAnswer,
  elicitationFieldDraft,
  elicitationFormMessage,
  elicitationOptions,
  elicitationQuestionText,
  normalizeElicitationFields,
  replaceElicitationFieldAnswer,
} from '../../src/components/acp/ElicitationCard';

const multiField: ElicitationField = {
  key: 'question_a',
  isSelect: true,
  isMulti: true,
  isCustom: false,
  hasCustomVariant: false,
  options: [
    { value: 'A1', label: 'A1' },
    { value: 'A2', label: 'A2' },
  ],
};

const singleField: ElicitationField = {
  key: 'question_b',
  isSelect: true,
  isMulti: false,
  isCustom: false,
  hasCustomVariant: false,
  options: [{ value: 'B1', label: 'B1' }],
};

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

  it('preserves the complete multiline request message', () => {
    const message =
      'Round 11 | 组件：反馈列表管理页 + 反馈详情页 | 歧义：23.5%\n\n管理端 API 与菜单的权限标识如何设计？';
    expect(
      elicitationQuestionText(message, undefined, 0, '请选择一个答案'),
    ).toBe(message);
    expect(elicitationFormMessage(message)).toBe(message);
  });

  it('does not parse natural-language lines as separate questions', () => {
    const source = readFileSync(
      path.resolve(__dirname, '../../src/components/acp/ElicitationCard.tsx'),
      'utf8',
    );

    expect(source).not.toContain('.split("\\n")');
    expect(source).not.toContain('lines[index]');
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

  it('keeps a 0.44 global customAnswer as an independent response field', () => {
    const fields = normalizeElicitationFields({
      type: 'object',
      properties: {
        customAnswer: {
          type: 'string',
          title: 'Other',
          description: 'Type your own answer.',
        },
        question_0: {
          type: 'string',
          title: '权限设计',
          oneOf: [{ const: 'admin-only', title: 'admin-only' }],
        },
      },
    });

    expect(fields.map((field) => field.key)).toEqual(['question_0', 'customAnswer']);
    expect(fields[0].hasCustomVariant).toBe(false);
    expect(fields[1]).toMatchObject({ isCustom: true, key: 'customAnswer' });
  });

  it('pairs a 0.45.1 question_n_custom field by naming convention', () => {
    const fields = normalizeElicitationFields({
      type: 'object',
      properties: {
        question_0: {
          type: 'string',
          oneOf: [{ const: 'A', title: 'A' }],
        },
        question_0_custom: { type: 'string', title: 'Other' },
      },
    });

    expect(fields).toHaveLength(1);
    expect(fields[0]).toMatchObject({
      key: 'question_0',
      hasCustomVariant: true,
      customVariantKey: 'question_0_custom',
    });
  });

  it('uses 0.63 custom-answer metadata as the exact association', () => {
    const fields = normalizeElicitationFields({
      type: 'object',
      properties: {
        question_0: {
          type: 'string',
          oneOf: [{ const: 'A', title: 'A' }],
        },
        answer_elsewhere: {
          type: 'string',
          _meta: {
            _askUserQuestionCustomAnswer: {
              questionId: 'question_0',
              isCustomAnswer: true,
            },
          },
        },
        unrelated: { type: 'string', title: '备注' },
      },
    });

    expect(fields[0].customVariantKey).toBe('answer_elsewhere');
    expect(fields[1]).toMatchObject({ key: 'unrelated', isCustom: true });
  });

  it('preserves option descriptions and Claude previews structurally', () => {
    const property: ElicitationPropertySchema = {
      type: 'string',
      oneOf: [
        {
          const: 'Grid',
          title: 'Grid',
          description: 'Cards in a responsive grid',
          _meta: {
            '_claude/askUserQuestionOption': {
              preview: '```\n[ ] [ ] [ ]\n```',
            },
          },
        },
      ],
    };

    expect(elicitationOptions(property)).toEqual([
      {
        value: 'Grid',
        label: 'Grid',
        description: 'Cards in a responsive grid',
        preview: '```\n[ ] [ ] [ ]\n```',
      },
    ]);
  });

  it('restores a confirmed multi-select answer when navigating back', () => {
    const answers = replaceElicitationFieldAnswer({}, multiField, ['A1', 'A2']);

    expect(elicitationFieldDraft(answers, multiField)).toMatchObject({
      multiValues: ['A1', 'A2'],
      customActive: false,
    });
  });

  it('removes an earlier answer when the user returns and skips that question', () => {
    let answers = replaceElicitationFieldAnswer({}, multiField, ['A1']);
    answers = replaceElicitationFieldAnswer(answers, singleField, 'B1');

    const skippedAnswers = clearElicitationFieldAnswer(answers, multiField);

    expect(buildElicitationContent([multiField, singleField], skippedAnswers)).toEqual({
      question_b: 'B1',
    });
  });

  it('clears the custom sibling value when a predefined option replaces it', () => {
    const field: ElicitationField = {
      ...singleField,
      customVariantKey: 'question_b_custom',
      hasCustomVariant: true,
    };
    let answers = replaceElicitationFieldAnswer(
      {},
      field,
      '自定义答案',
      'question_b_custom',
    );

    expect(buildElicitationContent([field], answers)).toEqual({
      question_b_custom: '自定义答案',
    });

    expect(elicitationFieldDraft(answers, field)).toMatchObject({
      customActive: true,
      customText: '自定义答案',
    });

    answers = replaceElicitationFieldAnswer(answers, field, 'B1');

    expect(buildElicitationContent([field], answers)).toEqual({ question_b: 'B1' });
  });
});
