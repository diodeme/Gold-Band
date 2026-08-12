import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import {
  normalizeScheduledTimeInput,
  ScheduledTimePicker,
} from '@/components/scheduled-tasks/ScheduledTimePicker';
import i18n from '@/i18n';

describe('ScheduledTimePicker', () => {
  it('renders a directly editable text input with a separate picker trigger', async () => {
    await i18n.changeLanguage('zh-CN');
    const html = renderToStaticMarkup(
      <ScheduledTimePicker value="12:23" onValueChange={() => undefined} />,
    );
    expect(html).toContain('12:23');
    expect(html).toContain('type="text"');
    expect(html).toContain('inputMode="numeric"');
    expect(html).toContain('placeholder="HH:mm"');
    expect(html).toContain('aria-label="时间"');
    expect(html).toContain('aria-label="打开时间选择器"');
    expect(html).not.toContain('type="time"');
  });

  it.each([
    ['09:05', '09:05'],
    ['9:5', '09:05'],
    ['0905', '09:05'],
    ['905', '09:05'],
    ['23:59', '23:59'],
    ['24:00', null],
    ['12:60', null],
    ['12', null],
    ['', null],
  ])('normalizes direct input %s to %s', (input, expected) => {
    expect(normalizeScheduledTimeInput(input)).toBe(expected);
  });

  it('exposes invalid input state through the shadcn input', async () => {
    await i18n.changeLanguage('zh-CN');
    const html = renderToStaticMarkup(
      <ScheduledTimePicker value="12:23" invalid onValueChange={() => undefined} />,
    );
    expect(html).toContain('aria-invalid="true"');
  });
});
