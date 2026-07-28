import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import {
  AcpSingleConfigMenu,
  resolveAcpSingleConfigMenuValue,
  UNSPECIFIED_ACP_CONFIG_VALUE,
} from '@/components/acp/AcpSingleConfigMenu';
import { ACP_COMPOSER_CONFIG_DROPDOWN_MODAL } from '@/components/acp/AcpComposerConfigTrigger';

describe('ACP single config menu', () => {
  it('uses the shared non-modal menu behavior for one-click switching', () => {
    expect(ACP_COMPOSER_CONFIG_DROPDOWN_MODAL).toBe(false);
  });

  it('maps the unspecified radio item to an empty override', () => {
    expect(resolveAcpSingleConfigMenuValue(UNSPECIFIED_ACP_CONFIG_VALUE)).toBeNull();
    expect(resolveAcpSingleConfigMenuValue('full_access')).toBe('full_access');
  });

  it('renders the config label and selected value through a dropdown trigger', () => {
    const markup = renderToStaticMarkup(createElement(AcpSingleConfigMenu, {
      label: '权限',
      value: 'full_access',
      options: [{ id: 'full_access', name: 'Full access' }],
      unspecifiedLabel: '不指定',
      onValueChange: () => {},
    }));

    expect(markup).toContain('data-slot="dropdown-menu-trigger"');
    expect(markup).toContain('权限');
    expect(markup).toContain('Full access');
    expect(markup).toContain('shadow-none');
  });
});
