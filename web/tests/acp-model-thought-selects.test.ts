import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import '@/i18n';
import {
  AcpModelThoughtSelects,
  formatAcpCompositeSelection,
  nextAcpCompositeSection,
} from '@/components/acp/AcpModelThoughtSelects';
import {
  ACP_COMPOSER_CONFIG_DROPDOWN_MODAL,
  DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
} from '@/components/acp/AcpComposerConfigTrigger';

function triggerClass(markup: string, slot: string) {
  const match = markup.match(new RegExp(`data-slot="${slot}"[^>]*class="([^"]+)"`));
  expect(match).not.toBeNull();
  return match?.[1] ?? '';
}

describe('ACP composite model selector', () => {
  it('anchors the main menu to the trigger start edge by default', () => {
    expect(DEFAULT_ACP_COMPOSER_CONFIG_ALIGN).toBe('start');
  });

  it('keeps the composer menu non-modal so adjacent controls open in one click', () => {
    expect(ACP_COMPOSER_CONFIG_DROPDOWN_MODAL).toBe(false);
  });

  it('keeps only one nested selector open and ignores stale close events', () => {
    let openSection = nextAcpCompositeSection(null, 'model', true);
    expect(openSection).toBe('model');

    openSection = nextAcpCompositeSection(openSection, 'reasoning_effort', true);
    expect(openSection).toBe('reasoning_effort');

    openSection = nextAcpCompositeSection(openSection, 'model', false);
    expect(openSection).toBe('reasoning_effort');

    openSection = nextAcpCompositeSection(openSection, 'reasoning_effort', false);
    expect(openSection).toBeNull();
  });

  it('shows one unspecified state until a model or thought level is selected', () => {
    expect(formatAcpCompositeSelection(null, null, '不指定')).toBe('不指定');
    expect(formatAcpCompositeSelection('GPT-5.6-Sol', null, '不指定')).toBe('GPT-5.6-Sol');
    expect(formatAcpCompositeSelection(null, 'high', '不指定')).toBe('不指定 · high');
    expect(formatAcpCompositeSelection('GPT-5.6-Sol', 'high', '不指定')).toBe('GPT-5.6-Sol · high');
  });

  it('always renders the model config name before the selected value', () => {
    const commonProps = {
      models: [{ id: 'gpt-5.6-sol', name: 'GPT-5.6-Sol' }],
      modelValue: 'gpt-5.6-sol',
      onModelChange: () => {},
    };
    const modelOnly = renderToStaticMarkup(createElement(AcpModelThoughtSelects, commonProps));
    const composite = renderToStaticMarkup(createElement(AcpModelThoughtSelects, {
      ...commonProps,
      thoughtLevel: {
        id: 'reasoning_effort',
        category: 'thought_level',
        options: [{ value: 'high', name: 'High' }],
      },
      thoughtValue: 'high',
    }));

    expect(modelOnly).toContain('模型');
    expect(modelOnly).toContain('GPT-5.6-Sol');
    expect(composite).toContain('模型');
    expect(composite).toContain('GPT-5.6-Sol · High');

    const modelOnlyTriggerClass = triggerClass(modelOnly, 'dropdown-menu-trigger');
    const compositeTriggerClass = triggerClass(composite, 'dropdown-menu-trigger');
    for (const className of [modelOnlyTriggerClass, compositeTriggerClass]) {
      expect(className).toContain('h-9');
      expect(className).toContain('rounded-full');
      expect(className).toContain('shadow-none');
      expect(className).toContain('[&amp;&gt;svg]:size-3.5');
    }
    expect(modelOnlyTriggerClass).not.toContain('shadow-xs');
  });
});
