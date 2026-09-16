import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import '@/i18n';
import {
  AcpModelThoughtSelects,
  acpConfigMenuSelectionMode,
  findAcpThoughtLevel,
  formatAcpCompositeSelection,
  nextAcpCompositeSection,
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
