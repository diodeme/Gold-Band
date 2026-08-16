import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import {
  AcpSingleConfigMenu,
  resolveAcpSingleConfigMenuValue,
  UNSPECIFIED_ACP_CONFIG_VALUE,
} from '@/components/acp/AcpSingleConfigMenu';
import {
  ACP_COMPOSER_CONFIG_DROPDOWN_MODAL,
  isAcpComposerConfigValueOverflowing,
} from '@/components/acp/AcpComposerConfigTrigger';
import { TooltipProvider } from '@/components/ui/tooltip';

function renderMenu(props: React.ComponentProps<typeof AcpSingleConfigMenu>) {
  return renderToStaticMarkup(createElement(
    TooltipProvider,
    null,
    createElement(AcpSingleConfigMenu, props),
  ));
}

describe('ACP single config menu', () => {
  it('uses the shared non-modal menu behavior for one-click switching', () => {
    expect(ACP_COMPOSER_CONFIG_DROPDOWN_MODAL).toBe(false);
  });

  it('maps the unspecified radio item to an empty override', () => {
    expect(resolveAcpSingleConfigMenuValue(UNSPECIFIED_ACP_CONFIG_VALUE)).toBeNull();
    expect(resolveAcpSingleConfigMenuValue('full_access')).toBe('full_access');
  });

  it('only treats text wider than its visible value slot as truncated', () => {
    expect(isAcpComposerConfigValueOverflowing({ clientWidth: 120, scrollWidth: 121 } as HTMLElement)).toBe(false);
    expect(isAcpComposerConfigValueOverflowing({ clientWidth: 120, scrollWidth: 240 } as HTMLElement)).toBe(true);
    expect(isAcpComposerConfigValueOverflowing(null)).toBe(false);
  });

  it('renders the config label and selected value through a dropdown trigger', () => {
    const markup = renderMenu({
      label: '权限',
      value: 'full_access',
      options: [{ id: 'full_access', name: 'Full access' }],
      unspecifiedLabel: '不指定',
      onValueChange: () => {},
    });

    expect(markup).toContain('data-slot="dropdown-menu-trigger"');
    expect(markup).toContain('权限');
    expect(markup).toContain('Full access');
    expect(markup).toContain('data-acp-config-value="true"');
    expect(markup).not.toContain('title=');
    expect(markup).toContain('shadow-none');
  });

  it('accepts a layout-owned trigger class without replacing shared trigger styles', () => {
    const markup = renderMenu({
      label: '权限',
      value: null,
      options: [],
      unspecifiedLabel: '不指定',
      onValueChange: () => {},
      triggerClassName: 'w-full max-w-none',
    });

    expect(markup).toContain('w-full max-w-none');
    expect(markup).toContain('rounded-full');
  });
});
