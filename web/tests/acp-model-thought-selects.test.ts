import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import {
  AcpModelThoughtSelects,
  acpConfigMenuSelectionMode,
  acpCompositeConfigSections,
  acpCompositeSectionLabel,
  findAcpThoughtLevel,
  formatAcpCompositeSelection,
  formatAcpCompositeSelectionParts,
  isAuthoringConfigOptionValueAllowed,
  nextAcpCompositeSection,
  remapAcpThoughtLevelOverride,
  retainAcpModelBoundOverrides,
  switchAcpModelBoundOverrides,
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
  const tag = markup.match(new RegExp(`<[^>]*data-slot="${slot}"[^>]*>`));
  expect(tag).not.toBeNull();
  const className = tag?.[0].match(/class="([^"]+)"/);
  expect(className).not.toBeNull();
  return className?.[1] ?? '';
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

  it('reuses the current model config when the selected model has not been observed', () => {
    const options = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'gpt-5.6-luna',
        options: [{ value: 'cursor-grok-4.6', name: 'Cursor Grok 4.6' }, { value: 'gpt-5.6-luna', name: '5.6 Luna' }],
      },
      {
        id: 'context',
        category: 'model_config',
        name: 'Context',
        options: [{ value: '272k', name: '272K' }, { value: '1m', name: '1M' }],
      },
      {
        id: 'effort',
        category: 'thought_level',
        options: [{ value: 'high', name: 'High' }],
      },
    ];
    expect(retainAcpModelBoundOverrides(
      { context: '1m', effort: 'high', theme: 'dark' },
      options,
      'cursor-grok-4.6',
    )).toEqual({ context: '1m', effort: 'high', theme: 'dark' });
    expect(acpCompositeConfigSections(options, 'cursor-grok-4.6', {
      context: '1m',
      effort: 'high',
    }).map((section) => [section.id, section.valueLabel])).toEqual([
      ['effort', 'High'],
      ['context', '1M'],
    ]);
  });

  it('reuses the authoring current table when switching to an unobserved model', () => {
    const options = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'gpt-5.6-luna',
        options: [
          { value: 'cursor-grok-4.6', name: 'Cursor Grok 4.6' },
          { value: 'gpt-5.6-luna', name: '5.6 Luna' },
          { value: 'gpt-5.2', name: 'GPT-5.2' },
        ],
      },
      {
        id: 'context',
        category: 'model_config',
        name: 'Context',
        options: [{ value: '272k', name: '272K' }, { value: '1m', name: '1M' }],
      },
      {
        id: 'reasoning',
        category: 'thought_level',
        options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
      },
    ];
    const catalogs = {
      'gpt-5.6-luna': [
        {
          id: 'reasoning',
          category: 'thought_level',
          options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
        },
        {
          id: 'context',
          category: 'model_config',
          name: 'Context',
          options: [{ value: '272k', name: '272K' }, { value: '1m', name: '1M' }],
        },
      ],
      'cursor-grok-4.6': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          name: 'Fast',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
      ],
    };

    expect(retainAcpModelBoundOverrides(
      { context: '1m', reasoning: 'low' },
      options,
      'gpt-5.2',
      catalogs,
    )).toEqual({ context: '1m', reasoning: 'low' });
    expect(acpCompositeConfigSections(options, 'gpt-5.2', {
      context: '1m',
      reasoning: 'low',
    }, catalogs).map((section) => section.id)).toEqual(['reasoning', 'context']);
    expect(acpCompositeConfigSections(options, 'cursor-grok-4.6', {
      context: '1m',
      reasoning: 'low',
    }, catalogs).map((section) => section.id)).toEqual(['effort', 'fast']);
  });

  it('shows the selected model\'s observed Fast even when Doctor current is another model', () => {
    const options = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'gpt-5.6-luna',
        options: [{ value: 'cursor-grok-4.6', name: 'Cursor Grok 4.6' }, { value: 'gpt-5.6-luna', name: '5.6 Luna' }],
      },
      {
        id: 'context',
        category: 'model_config',
        name: 'Context',
        options: [{ value: '272k', name: '272K' }, { value: '1m', name: '1M' }],
      },
      {
        id: 'reasoning',
        category: 'thought_level',
        options: [{ value: 'high', name: 'High' }],
      },
    ];
    const catalogs = {
      'cursor-grok-4.6': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'high', name: 'High' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          name: 'Fast',
          options: [{ value: 'true', name: 'On' }, { value: 'false', name: 'Off' }],
        },
      ],
      'gpt-5.6-luna': [
        {
          id: 'reasoning',
          category: 'thought_level',
          options: [{ value: 'high', name: 'High' }],
        },
        {
          id: 'context',
          category: 'model_config',
          name: 'Context',
          options: [{ value: '272k', name: '272K' }, { value: '1m', name: '1M' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          name: 'Fast',
          options: [{ value: 'true', name: 'On' }, { value: 'false', name: 'Off' }],
        },
      ],
    };
    expect(retainAcpModelBoundOverrides(
      { fast: 'true', context: '1m', effort: 'high' },
      options,
      'cursor-grok-4.6',
      catalogs,
    )).toEqual({ effort: 'high', fast: 'true' });
    expect(retainAcpModelBoundOverrides(
      { fast: 'true', context: '1m', effort: 'high' },
      options,
      'gpt-5.6-luna',
      catalogs,
    )).toEqual({ reasoning: 'high', context: '1m', fast: 'true' });
    expect(acpCompositeConfigSections(options, 'cursor-grok-4.6', {
      fast: 'true',
      context: '1m',
      effort: 'high',
    }, catalogs).map((section) => section.id)).toEqual(['effort', 'fast']);
    expect(acpCompositeConfigSections(options, 'gpt-5.6-luna', {
      fast: 'true',
      context: '1m',
      effort: 'high',
    }, catalogs).map((section) => section.id)).toEqual(['reasoning', 'context', 'fast']);
  });

  it('restores Grok Extra High after switching to Mini whose catalog cannot keep it', () => {
    const options = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'grok-4.6',
        options: [
          { value: 'grok-4.6', name: 'Cursor Grok 4.6' },
          { value: 'gpt-5-mini', name: 'GPT-5 Mini' },
        ],
      },
    ];
    const catalogs = {
      'grok-4.6': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [
            { value: 'high', name: 'High' },
            { value: 'extra-high', name: 'Extra High' },
          ],
        },
        {
          id: 'fast',
          category: 'model_config',
          name: 'Fast',
          options: [{ value: 'true', name: 'On' }, { value: 'false', name: 'Off' }],
        },
      ],
      'gpt-5-mini': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [
            { value: 'low', name: 'Low' },
            { value: 'high', name: 'High' },
          ],
        },
      ],
    };
    const strippedOnMini = retainAcpModelBoundOverrides(
      { effort: 'extra-high', fast: 'true' },
      options,
      'gpt-5-mini',
      catalogs,
    );
    expect(strippedOnMini).toEqual({});
    expect(retainAcpModelBoundOverrides(strippedOnMini, options, 'grok-4.6', catalogs)).toEqual({});

    const toMini = switchAcpModelBoundOverrides({
      remembered: {},
      previousModelId: 'grok-4.6',
      nextModelId: 'gpt-5-mini',
      currentOverrides: { effort: 'extra-high', fast: 'true' },
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    expect(toMini.overrides).toEqual({});
    expect(toMini.remembered['grok-4.6']).toEqual({ effort: 'extra-high', fast: 'true' });
    expect(toMini.remembered['gpt-5-mini']).toEqual({});

    const toGrok = switchAcpModelBoundOverrides({
      remembered: toMini.remembered,
      previousModelId: 'gpt-5-mini',
      nextModelId: 'grok-4.6',
      currentOverrides: toMini.overrides,
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    expect(toGrok.overrides).toEqual({ effort: 'extra-high', fast: 'true' });
  });

  it('does not remap leftover Grok Fast Off onto Fable thinking Off', () => {
    const fableRows = [
      {
        id: 'thinking',
        category: 'thought_level',
        name: 'Thinking',
        options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
      },
      {
        id: 'effort',
        category: 'thought_level',
        name: 'Effort',
        options: [{ value: 'high', name: 'High' }, { value: 'extra-high', name: 'Extra High' }],
      },
      {
        id: 'context',
        category: 'model_config',
        name: 'Context',
        options: [{ value: '1m', name: '1M' }],
      },
    ];
    const grokRows = [
      {
        id: 'effort',
        category: 'thought_level',
        name: 'Effort',
        options: [{ value: 'high', name: 'High' }, { value: 'extra-high', name: 'Extra High' }],
      },
      {
        id: 'fast',
        category: 'model_config',
        name: 'Fast',
        options: [{ value: 'true', name: 'On' }, { value: 'false', name: 'Off' }],
      },
    ];
    const doctorConfigOptions = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'claude-fable',
        options: [
          { value: 'claude-fable', name: 'Fable' },
          { value: 'grok-4.6', name: 'Grok' },
        ],
      },
      ...fableRows,
    ];
    const catalogs = {
      'claude-fable': fableRows,
      'grok-4.6': grokRows,
    };

    expect(retainAcpModelBoundOverrides(
      { effort: 'high', fast: 'false' },
      doctorConfigOptions,
      'claude-fable',
      catalogs,
    )).toEqual({ effort: 'high' });

    const switched = switchAcpModelBoundOverrides({
      remembered: {},
      previousModelId: 'grok-4.6',
      nextModelId: 'claude-fable',
      currentOverrides: { effort: 'high', fast: 'false' },
      configOptions: doctorConfigOptions,
      modelBoundCatalogs: catalogs,
    });
    expect(switched.overrides).toEqual({ effort: 'high' });
    expect(switched.overrides.thinking).toBeUndefined();
  });

  it('does not overwrite an already valid thought value when remapping another thought id', () => {
    expect(remapAcpThoughtLevelOverride(
      { effort: 'high', reasoning: 'medium' },
      [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'medium', name: 'Medium' }, { value: 'high', name: 'High' }],
        },
      ],
      [
        {
          id: 'reasoning',
          category: 'thought_level',
          options: [{ value: 'medium', name: 'Medium' }, { value: 'high', name: 'High' }],
        },
      ],
    )).toEqual({ effort: 'high' });
  });

  it('seeds the first visit from current overrides and restores an empty remembered slot', () => {
    const options = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'grok-4.6',
        options: [
          { value: 'grok-4.6', name: 'Grok' },
          { value: 'gpt-5-mini', name: 'Mini' },
        ],
      },
    ];
    const catalogs = {
      'grok-4.6': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'high', name: 'High' }, { value: 'extra-high', name: 'Extra High' }],
        },
      ],
      'gpt-5-mini': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
        },
      ],
    };

    const firstVisit = switchAcpModelBoundOverrides({
      remembered: {},
      previousModelId: 'grok-4.6',
      nextModelId: 'gpt-5-mini',
      currentOverrides: { effort: 'high' },
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    expect(firstVisit.overrides).toEqual({ effort: 'high' });
    expect(firstVisit.remembered['gpt-5-mini']).toEqual({ effort: 'high' });

    const leaveMiniEmpty = switchAcpModelBoundOverrides({
      remembered: firstVisit.remembered,
      previousModelId: 'gpt-5-mini',
      nextModelId: 'grok-4.6',
      currentOverrides: {},
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    expect(leaveMiniEmpty.remembered['gpt-5-mini']).toEqual({});

    const restoreEmptyMini = switchAcpModelBoundOverrides({
      remembered: leaveMiniEmpty.remembered,
      previousModelId: 'grok-4.6',
      nextModelId: 'gpt-5-mini',
      currentOverrides: { effort: 'extra-high' },
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    expect(restoreEmptyMini.overrides).toEqual({});
  });

  it('overwrites a model slot with the state at leave, not the first visit', () => {
    const options = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'grok-4.6',
        options: [
          { value: 'grok-4.6', name: 'Grok' },
          { value: 'gpt-5-mini', name: 'Mini' },
        ],
      },
    ];
    const catalogs = {
      'grok-4.6': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'high', name: 'High' }, { value: 'extra-high', name: 'Extra High' }],
        },
      ],
      'gpt-5-mini': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
        },
      ],
    };
    const afterFirstLeave = switchAcpModelBoundOverrides({
      remembered: {},
      previousModelId: 'grok-4.6',
      nextModelId: 'gpt-5-mini',
      currentOverrides: { effort: 'extra-high' },
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    const afterEdit = switchAcpModelBoundOverrides({
      remembered: afterFirstLeave.remembered,
      previousModelId: 'gpt-5-mini',
      nextModelId: 'grok-4.6',
      currentOverrides: afterFirstLeave.overrides,
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    const leaveEditedGrok = switchAcpModelBoundOverrides({
      remembered: afterEdit.remembered,
      previousModelId: 'grok-4.6',
      nextModelId: 'gpt-5-mini',
      currentOverrides: { effort: 'high' },
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    const backToGrok = switchAcpModelBoundOverrides({
      remembered: leaveEditedGrok.remembered,
      previousModelId: 'gpt-5-mini',
      nextModelId: 'grok-4.6',
      currentOverrides: leaveEditedGrok.overrides,
      configOptions: options,
      modelBoundCatalogs: catalogs,
    });
    expect(backToGrok.overrides).toEqual({ effort: 'high' });
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
      ['Thinking', 'Extra High'],
      ['Fast', 'On'],
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

    expect(markup).toContain('Composer 2.5 · Extra High · On');
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

  it('keeps the composite thought and Fast menu after selecting an observed model that is not Doctor current', () => {
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
      modelBoundCatalogs: {
        'gpt-5.6-terra': [
          {
            id: 'effort',
            category: 'thought_level',
            name: 'Effort',
            options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
          },
          {
            id: 'fast',
            category: 'model_config',
            name: 'Fast',
            options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
          },
        ],
      },
      configOptionValues: { fast: 'true', effort: 'high' },
      onModelChange: () => {},
      onConfigOptionChange: () => {},
    });

    expect(markup).toContain('5.6 Terra · High · On');
    expect(markup).not.toContain('Effort');
  });

  it('does not paint Luna Context onto Grok, and still shows Grok Fast from the Grok catalog', () => {
    const markup = renderSelect({
      models: [
        { id: 'cursor-grok-4.6', name: 'Cursor Grok 4.6' },
        { id: 'gpt-5.6-luna', name: '5.6 Luna' },
      ],
      modelValue: 'cursor-grok-4.6',
      configOptions: [
        {
          id: 'model',
          category: 'model',
          currentValue: 'gpt-5.6-luna',
          options: [{ value: 'cursor-grok-4.6', name: 'Cursor Grok 4.6' }, { value: 'gpt-5.6-luna', name: '5.6 Luna' }],
        },
        {
          id: 'context',
          category: 'model_config',
          name: 'Context',
          options: [{ value: '272k', name: '272K' }, { value: '1m', name: '1M' }],
        },
        {
          id: 'reasoning',
          category: 'thought_level',
          name: 'Effort',
          options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
        },
      ],
      modelBoundCatalogs: {
        'cursor-grok-4.6': [
          {
            id: 'effort',
            category: 'thought_level',
            options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
          },
          {
            id: 'fast',
            category: 'model_config',
            name: 'Fast',
            options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
          },
        ],
        'gpt-5.6-luna': [
          {
            id: 'reasoning',
            category: 'thought_level',
            options: [{ value: 'low', name: 'Low' }, { value: 'high', name: 'High' }],
          },
          {
            id: 'context',
            category: 'model_config',
            name: 'Context',
            options: [{ value: '272k', name: '272K' }, { value: '1m', name: '1M' }],
          },
          {
            id: 'fast',
            category: 'model_config',
            name: 'Fast',
            options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
          },
        ],
      },
      configOptionValues: { fast: 'true', context: '1m', effort: 'high' },
      onModelChange: () => {},
      onConfigOptionChange: () => {},
    });

    expect(markup).toContain('Cursor Grok 4.6 · High · On');
    expect(markup).not.toContain('1M');
    expect(markup).not.toContain('Context');
    expect(markup).not.toContain('Effort');
  });

  it('labels thought_level as the unified thought control and keeps Fast on the Agent name', () => {
    expect(acpCompositeSectionLabel({
      id: 'effort',
      category: 'thought_level',
      name: 'Effort',
    }, '思考强度')).toBe('思考强度（effort）');
    expect(acpCompositeSectionLabel({
      id: 'deep_think',
      category: 'thought_level',
      name: 'Deep Think',
    }, '思考强度')).toBe('思考强度（deep_think）');
    expect(acpCompositeSectionLabel({
      id: 'fast',
      category: 'model_config',
      name: 'Fast',
    }, '思考强度')).toBe('Fast');
  });

  it('maps named config options and splits multiple thought_level rows', () => {
    const translate = (key: string, options?: Record<string, unknown>) => {
      const names: Record<string, string> = {
        'acp.configOption.thinking': '深度思考',
        'acp.configOption.effort': '思考强度',
        'acp.configOption.context': '上下文',
      };
      return names[key] ?? String(options?.defaultValue ?? key);
    };

    expect(acpCompositeSectionLabel({
      id: 'thinking',
      category: 'thought_level',
      name: 'Thinking',
    }, '思考强度', 1, translate)).toBe('思考强度（thinking）');
    expect(acpCompositeSectionLabel({
      id: 'thinking',
      category: 'thought_level',
      name: 'Thinking',
    }, '思考强度', 2, translate)).toBe('深度思考（thinking）');
    expect(acpCompositeSectionLabel({
      id: 'effort',
      category: 'thought_level',
      name: 'Effort',
    }, '思考强度', 2, translate)).toBe('思考强度（effort）');
    expect(acpCompositeSectionLabel({
      id: 'context',
      category: 'model_config',
      name: 'Context',
    }, '思考强度', 1, translate)).toBe('上下文（context）');
    expect(acpCompositeSectionLabel({
      id: 'fast',
      category: 'model_config',
      name: 'Fast',
    }, '思考强度', 2, translate)).toBe('Fast');
  });

  it('renders distinct Fable thought labels instead of two 思考强度 rows', () => {
    const sections = acpCompositeConfigSections([
      {
        id: 'model',
        category: 'model',
        currentValue: 'claude-fable-5-1',
        options: [{ value: 'claude-fable-5-1', name: 'Claude Fable 5.1' }],
      },
      {
        id: 'thinking',
        category: 'thought_level',
        name: 'Thinking',
        options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
      },
      {
        id: 'effort',
        category: 'thought_level',
        name: 'Effort',
        options: [{ value: 'medium', name: 'Medium' }, { value: 'high', name: 'High' }],
      },
      {
        id: 'context',
        category: 'model_config',
        name: 'Context',
        options: [{ value: '300k', name: '300K' }, { value: '1m', name: '1M' }],
      },
    ], 'claude-fable-5-1', { thinking: 'false', effort: 'medium' });
    const thoughtLevelCount = sections.filter((section) => section.category === 'thought_level').length;
    const translate = (key: string, options?: Record<string, unknown>) => i18n.t(key, options);

    expect(sections.map((section) => acpCompositeSectionLabel(
      section,
      translate('acp.thoughtLevel'),
      thoughtLevelCount,
      translate,
    ))).toEqual(['深度思考（thinking）', '思考强度（effort）', '上下文（context）']);
    expect(acpCompositeSectionLabel(
      sections[0],
      'Reasoning',
      thoughtLevelCount,
      (key, options) => i18n.t(key, { ...options, lng: 'en' }),
    )).toBe('Thinking');
    expect(acpCompositeSectionLabel(
      { id: 'effort', category: 'thought_level', name: 'Effort' },
      i18n.t('acp.thoughtLevel', { lng: 'en' }),
      1,
      (key, options) => i18n.t(key, { ...options, lng: 'en' }),
    )).toBe('Reasoning (effort)');
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

  it('keeps High across thought option ids and puts Context after 思考强度', () => {
    const lunaCatalog = [
      {
        id: 'model',
        category: 'model',
        currentValue: 'gpt-5.6-luna',
        options: [{ value: 'grok-4.6', name: 'Cursor Grok 4.6' }, { value: 'gpt-5.6-luna', name: 'GPT-5.6 Luna' }],
      },
      {
        id: 'context',
        category: 'model_config',
        name: 'Context',
        options: [{ value: '272k', name: '272K' }, { value: '1m', name: '1M' }],
      },
      {
        id: 'reasoning',
        category: 'thought_level',
        name: 'Reasoning',
        options: [{ value: 'medium', name: 'Medium' }, { value: 'high', name: 'High' }],
      },
      {
        id: 'fast',
        category: 'model_config',
        name: 'Fast',
        options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'Fast' }],
      },
    ];

    expect(remapAcpThoughtLevelOverride(
      { effort: 'high', fast: 'false' },
      lunaCatalog,
    )).toEqual({ reasoning: 'high', fast: 'false' });
    expect(remapAcpThoughtLevelOverride(
      { effort: 'high', fast: 'false' },
      [
        {
          id: 'thinking',
          category: 'thought_level',
          name: 'Thinking',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
        {
          id: 'effort',
          category: 'thought_level',
          name: 'Effort',
          options: [{ value: 'high', name: 'High' }],
        },
        {
          id: 'context',
          category: 'model_config',
          name: 'Context',
          options: [{ value: '1m', name: '1M' }],
        },
      ],
      [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'high', name: 'High' }, { value: 'extra-high', name: 'Extra High' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          name: 'Fast',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
      ],
    )).toEqual({ effort: 'high', fast: 'false' });
    expect(remapAcpThoughtLevelOverride(
      { reasoning: 'high', fast: 'false' },
      [
        {
          id: 'thinking',
          category: 'thought_level',
          name: 'Thinking',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
        {
          id: 'effort',
          category: 'thought_level',
          name: 'Effort',
          options: [{ value: 'high', name: 'High' }],
        },
        {
          id: 'context',
          category: 'model_config',
          name: 'Context',
          options: [{ value: '1m', name: '1M' }],
        },
      ],
      [
        {
          id: 'reasoning',
          category: 'thought_level',
          options: [{ value: 'medium', name: 'Medium' }, { value: 'high', name: 'High' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
      ],
    )).toEqual({ effort: 'high', fast: 'false' });
    expect(acpCompositeConfigSections(
      lunaCatalog,
      'gpt-5.6-luna',
      { effort: 'high', fast: 'false' },
    ).map((section) => [section.id, section.name, section.valueLabel])).toEqual([
      ['reasoning', 'Reasoning', 'High'],
      ['context', 'Context', null],
      ['fast', 'Fast', 'Off'],
    ]);
  });

  it('allows Grok effort and fast against the projected catalog while Doctor current belongs to Luna', () => {
    const current = [
      {
        id: 'reasoning',
        category: 'thought_level',
        options: [{ value: 'high', name: 'High' }],
      },
      {
        id: 'context',
        category: 'model_config',
        options: [{ value: '1m', name: '1M' }],
      },
    ];
    const catalogs = {
      'grok-4.6': [
        {
          id: 'effort',
          category: 'thought_level',
          options: [{ value: 'high', name: 'High' }],
        },
        {
          id: 'fast',
          category: 'model_config',
          options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
        },
      ],
      'gpt-5.6-luna': current,
    };

    expect(isAuthoringConfigOptionValueAllowed('effort', 'high', current, catalogs, 'grok-4.6')).toBe(true);
    expect(isAuthoringConfigOptionValueAllowed('fast', 'false', current, catalogs, 'grok-4.6')).toBe(true);
    expect(isAuthoringConfigOptionValueAllowed('effort', 'bogus', current, catalogs, 'grok-4.6')).toBe(false);
    expect(isAuthoringConfigOptionValueAllowed('reasoning', 'high', current, catalogs, 'grok-4.6')).toBe(true);
    expect(isAuthoringConfigOptionValueAllowed('theme', 'dark', current, catalogs, 'grok-4.6')).toBe(false);
  });
});
