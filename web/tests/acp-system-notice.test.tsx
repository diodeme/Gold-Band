import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { AcpSystemNoticeDivider } from '@/components/acp/AcpSystemNotice';
import '@/i18n';
import {
  ACP_SESSION_CONFIG_ROLLED_BACK_CODE,
  acpSystemNoticeCopy,
} from '@/lib/acp-system-notice';

function fakeT(prefix: string) {
  return (key: string) => `${prefix}:${key}`;
}

describe('ACP system notice', () => {
  it('maps rolled-back model config to localized divider copy', () => {
    const event = {
      kind: 'systemNotice',
      raw: {
        systemNotice: {
          code: ACP_SESSION_CONFIG_ROLLED_BACK_CODE,
          params: {
            items: [{ category: 'thought_level', configId: 'effort', value: 'high' }],
          },
        },
      },
    };

    expect(acpSystemNoticeCopy(fakeT('zh'), event)).toBe('zh:acp.sessionConfigRolledBack');
    expect(acpSystemNoticeCopy(fakeT('en'), event)).toBe('en:acp.sessionConfigRolledBack');
  });

  it('renders a timeline divider instead of a banner or message bubble', () => {
    const markup = renderToStaticMarkup(createElement(AcpSystemNoticeDivider, {
      event: {
        kind: 'systemNotice',
        raw: {
          systemNotice: {
            code: ACP_SESSION_CONFIG_ROLLED_BACK_CODE,
            params: { items: [{ category: 'model_config', configId: 'fast', value: 'true' }] },
          },
        },
      },
    }));

    expect(markup).toContain('data-acp-system-notice="true"');
    expect(markup).toContain('h-px');
    expect(markup).not.toContain('data-acp-error-banner');
    expect(markup).toMatch(/你选择的模型配置当前模型暂不支持|The selected model options/);
  });
});
