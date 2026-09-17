import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import '@/i18n';
import {
  AcpModelThoughtSelects,
  acpConfigMenuSelectionMode,
  acpCompositeConfigSections,
  acpCompositeSectionLabel,
  findAcpThoughtLevel,
  formatAcpCompositeSelection,
  formatAcpCompositeSelectionParts,
  nextAcpCompositeSection,
  retainAcpModelBoundOverrides,
  updateAcpConfigOptionOverride,
} from '@/components/acp/AcpModelThoughtSelects';
import {
  ACP_COMPOSER_CONFIG_DROPDOWN_MODAL,
  DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
  keepAcpConfigMenuOpenOnSelect,
} from '@/components/acp/AcpComposerConfigTrigger';
import { TooltipProvider } from '@/components/ui/tooltip';

function renderSelect(props: React.ComponentProps<typeof AcpModelThoughtSelects>) {
  return renderToStaticMarkup(createElement(
    TooltipProvider,
    null,
    createElement(AcpModelThoughtSelects, props),
  ));
}

function triggerClass(markup: string, slot: string) {
  const match = markup.match(new RegExp(`data-slot="${slot}"[^>]*class="([^"]+)"`));
  expect(match).not.toBeNull();
  return match?.[1] ?? '';
}

describe('ACP composite model selector', () => {
  it('resolves thought-level capabilities without depending on provider-specific option ids', () => {
    expect(findAcpThoughtLevel([
      { id: 'theme', category: 'appearance', options: [] },
      { id: 'reasoning_effort', category: 'thought_level', options: [{ value: 'high', name: 'High' }] },
    ])?.id).toBe('reasoning_effort');
    expect(findAcpThoughtLevel(null)).toBeNull();
  });

  it('updates generic config option overrides immutably and removes unspecified values', () => {
    const current = { theme: 'dark', reasoning_effort: 'medium' };
    const updated = updateAcpConfigOptionOverride(current, 'reasoning_effort', 'high');
    const cleared = updateAcpConfigOptionOverride(updated, 'reasoning_effort', null);

    expect(current).toEqual({ theme: 'dark', reasoning_effort: 'medium' });
    expect(updated).toEqual({ theme: 'dark', reasoning_effort: 'high' });
    expect(cleared).toEqual({ theme: 'dark' });
  });

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

  it('keeps the composite config menu open after selecting a model or thought level', () => {
    const event = new Event('select', { cancelable: true });

    keepAcpConfigMenuOpenOnSelect(event);

    expect(event.defaultPrevented).toBe(true);
  });

  it('uses selection capability to distinguish close-on-select and composite menus', () => {
    expect(acpConfigMenuSelectionMode(null)).toBe('single');
    expect(acpConfigMenuSelectionMode({
      id: 'reasoning_effort',
      category: 'thought_level',
      options: [],
    })).toBe('single');
    expect(acpConfigMenuSelectionMode({
      id: 'reasoning_effort',
      category: 'thought_level',
      options: [{ value: 'high', name: 'High' }],
    })).toBe('composite');
    expect(acpConfigMenuSelectionMode(null, [{
      options: [{ value: 'true', name: 'On' }],
    }])).toBe('composite');
  });

  it('keeps thought and Fast overrides when the selected model is not the catalog current model', () => {
    const options = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'gpt-5.6-sol',
        options: [{ value: 'gpt-5.6-sol', name: '5.6 Sol' }, { value: 'gpt-5.6-terra', name: '5.6 Terra' }],
      },
      {
        id: 'fast',
        category: 'model_config',
        options: [{ value: 'true', name: 'On' }, { value: 'false', name: 'Off' }],
      },
      {
        id: 'effort',
        category: 'thought_level',
        options: [{ value: 'high', name: 'High' }],
      },
    ];
    expect(retainAcpModelBoundOverrides(
      { fast: 'true', effort: 'high', theme: 'dark' },
      options,
      'gpt-5.6-terra',
    )).toEqual({ fast: 'true', effort: 'high', theme: 'dark' });
    expect(retainAcpModelBoundOverrides(
      { fast: 'true', effort: 'max', theme: 'dark' },
      options,
      'gpt-5.6-terra',
    )).toEqual({ fast: 'true', theme: 'dark' });
    expect(acpCompositeConfigSections([
      {
        id: 'model',
        category: 'model',
        currentValue: 'gpt-5.6-sol',
        options: [{ value: 'gpt-5.6-sol', name: '5.6 Sol' }, { value: 'gpt-5.6-terra', name: '5.6 Terra' }],
      },
      {
        id: 'fast',
        category: 'model_config',
        name: 'Fast',
        options: [{ value: 'true', name: 'On' }],
      },
      {
        id: 'effort',
        category: 'thought_level',
        name: 'Effort',
        options: [{ value: 'high', name: 'High' }],
      },
    ], 'gpt-5.6-terra', { fast: 'true', effort: 'high' }).map((section) => [section.id, section.valueLabel])).toEqual([
      ['fast', 'On'],
      ['effort', 'High'],
    ]);
  });

  it('shows one unspecified state until a model or thought level is selected', () => {
    expect(formatAcpCompositeSelection(null, null, '不指定')).toBe('不指定');
    expect(formatAcpCompositeSelection('GPT-5.6-Sol', null, '不指定')).toBe('GPT-5.6-Sol');
    expect(formatAcpCompositeSelection(null, 'high', '不指定')).toBe('不指定 · high');
    expect(formatAcpCompositeSelection('GPT-5.6-Sol', 'high', '不指定')).toBe('GPT-5.6-Sol · high');
    expect(formatAcpCompositeSelectionParts(['Composer 2.5', null, 'Extra High'], '不指定')).toBe('Composer 2.5 · Extra High');
    expect(formatAcpCompositeSelectionParts([null, 'On', null], '不指定')).toBe('不指定 · On');
    expect(formatAcpCompositeSelectionParts(['Composer 2.5', 'On', 'Extra High'], '不指定')).toBe('Composer 2.5 · On · Extra High');
    expect(acpCompositeConfigSections([
      {
        id: 'model',
        category: 'model',
        currentValue: 'composer-2.5',
        options: [{ value: 'composer-2.5', name: 'Composer 2.5' }],
      },
      {
        id: 'fast',
        category: 'model_config',
        name: 'Fast',
        options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
      },
      {
        id: 'effort',
        category: 'thought_level',
        name: 'Thinking',
        options: [{ value: 'low', name: 'Low' }, { value: 'extra-high', name: 'Extra High' }],
      },
    ], 'composer-2.5', { fast: 'true', effort: 'extra-high' }).map((section) => [section.name, section.valueLabel])).toEqual([
      ['Fast', 'On'],
      ['Thinking', 'Extra High'],
    ]);
  });

  it('always renders the model config name before the selected value', () => {
    const commonProps = {
      models: [{ id: 'gpt-5.6-sol', name: 'GPT-5.6-Sol' }],
      modelValue: 'gpt-5.6-sol',
      onModelChange: () => {},
    };
    const modelOnly = renderSelect(commonProps);
    const composite = renderSelect({
      ...commonProps,
      thoughtLevel: {
        id: 'reasoning_effort',
        category: 'thought_level',
        options: [{ value: 'high', name: 'High' }],
      },
      thoughtValue: 'high',
    });

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

  it('forwards the layout-owned trigger class in model-only and composite modes', () => {
    const commonProps = {
      models: [{ id: 'gpt-5.6-sol', name: 'GPT-5.6-Sol' }],
      modelValue: 'gpt-5.6-sol',
      onModelChange: () => {},
      triggerClassName: 'w-full max-w-none',
    };
    const modelOnly = renderSelect(commonProps);
    const composite = renderSelect({
      ...commonProps,
      thoughtLevel: {
        id: 'reasoning_effort',
        category: 'thought_level',
        options: [{ value: 'high', name: 'High' }],
      },
      thoughtValue: 'high',
    });

    expect(triggerClass(modelOnly, 'dropdown-menu-trigger')).toContain('w-full max-w-none');
    expect(triggerClass(composite, 'dropdown-menu-trigger')).toContain('w-full max-w-none');
  });

  it('places official model_config options in the same composite menu as thought level', () => {
    const markup = renderSelect({
      models: [{ id: 'composer-2.5', name: 'Composer 2.5' }],
      modelValue: 'composer-2.5',
      configOptions: [
        {
          id: 'model',
          category: 'model',
          currentValue: 'composer-2.5',
          options: [{ value: 'composer-2.5', name: 'Composer 2.5' }, { value: 'grok-4.6', name: 'Cursor Grok 4.6' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          name: 'Fast',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
        {
          id: 'effort',
          category: 'thought_level',
          name: 'Thinking',
          options: [{ value: 'low', name: 'Low' }, { value: 'extra-high', name: 'Extra High' }],
        },
      ],
      configOptionValues: { fast: 'true', effort: 'extra-high' },
      onModelChange: () => {},
      onConfigOptionChange: () => {},
    });

    expect(markup).toContain('Composer 2.5 · On · Extra High');
  });

  it('opens a composite menu when only official model_config options exist', () => {
    const markup = renderSelect({
      models: [{ id: 'composer-2.5', name: 'Composer 2.5' }],
      modelValue: 'composer-2.5',
      configOptions: [
        {
          id: 'model',
          category: 'model',
          currentValue: 'composer-2.5',
          options: [{ value: 'composer-2.5', name: 'Composer 2.5' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          name: 'Fast',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
      ],
      configOptionValues: { fast: 'true' },
      onModelChange: () => {},
      onConfigOptionChange: () => {},
    });

    expect(markup).toContain('Composer 2.5 · On');
  });

  it('keeps the composite thought and Fast menu after selecting a model that is not the catalog current model', () => {
    const markup = renderSelect({
      models: [
        { id: 'gpt-5.6-sol', name: '5.6 Sol' },
        { id: 'gpt-5.6-terra', name: '5.6 Terra' },
      ],
      modelValue: 'gpt-5.6-terra',
      configOptions: [
        {
          id: 'model',
          category: 'model',
          currentValue: 'gpt-5.6-sol',
          options: [{ value: 'gpt-5.6-sol', name: '5.6 Sol' }, { value: 'gpt-5.6-terra', name: '5.6 Terra' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          name: 'Fast',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
        {
          id: 'effort',
          category: 'thought_level',
          name: 'Effort',
          options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
        },
      ],
      configOptionValues: { fast: 'true', effort: 'high' },
      onModelChange: () => {},
      onConfigOptionChange: () => {},
    });

    expect(markup).toContain('5.6 Terra · On · High');
    expect(markup).not.toContain('Effort');
  });

  it('labels thought_level as the unified thought control and keeps Fast on the Agent name', () => {
    expect(acpCompositeSectionLabel({
      id: 'effort',
      category: 'thought_level',
      name: 'Effort',
    }, '思考强度')).toBe('思考强度');
    expect(acpCompositeSectionLabel({
      id: 'deep_think',
      category: 'thought_level',
      name: 'Deep Think',
    }, '思考强度')).toBe('思考强度');
    expect(acpCompositeSectionLabel({
      id: 'fast',
      category: 'model_config',
      name: 'Fast',
    }, '思考强度')).toBe('Fast');
  });

  it('forwards disabled state to model-only and composite triggers', () => {
    const commonProps = {
      models: [{ id: 'gpt-5.6-sol', name: 'GPT-5.6-Sol' }],
      modelValue: 'gpt-5.6-sol',
      onModelChange: () => {},
      disabled: true,
    };
    const modelOnly = renderSelect(commonProps);
    const composite = renderSelect({
      ...commonProps,
      thoughtLevel: {
        id: 'reasoning_effort',
        category: 'thought_level',
        options: [{ value: 'high', name: 'High' }],
      },
      thoughtValue: 'high',
    });

    expect(modelOnly.match(/<button[^>]*data-slot="dropdown-menu-trigger"[^>]*>/)?.[0]).toContain('disabled=""');
    expect(composite.match(/<button[^>]*data-slot="dropdown-menu-trigger"[^>]*>/)?.[0]).toContain('disabled=""');
  });
});
