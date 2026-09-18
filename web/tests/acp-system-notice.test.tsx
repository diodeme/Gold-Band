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

  it('names the rolled-back options in divider copy', () => {
    const event = {
      kind: 'systemNotice',
      raw: {
        systemNotice: {
          code: ACP_SESSION_CONFIG_ROLLED_BACK_CODE,
          params: {
            items: [
              { category: 'thought_level', configId: 'effort', value: 'high' },
              { category: 'model_config', configId: 'context', value: '1m', name: 'Context' },
            ],
          },
        },
      },
    };

    expect(acpSystemNoticeCopy((key, values) => {
      if (key === 'acp.thoughtLevel') return '思考强度';
      if (key === 'acp.sessionConfigRolledBack') return `${values?.names} 不支持，系统已将其回滚为不指定。可停止对话后修改。`;
      return key;
    }, event)).toBe('思考强度 · Context 不支持，系统已将其回滚为不指定。可停止对话后修改。');
  });

  it('falls back to the option id when a model config has no protocol name', () => {
    const event = {
      kind: 'systemNotice',
      raw: {
        systemNotice: {
          code: ACP_SESSION_CONFIG_ROLLED_BACK_CODE,
          params: {
            items: [{ category: 'model_config', configId: 'fast', value: 'true' }],
          },
        },
      },
    };

    expect(acpSystemNoticeCopy((key, values) => {
      if (key === 'acp.sessionConfigRolledBack') return `${values?.names} 不支持，系统已将其回滚为不指定。可停止对话后修改。`;
      return key;
    }, event)).toBe('fast 不支持，系统已将其回滚为不指定。可停止对话后修改。');
  });

  it('renders a timeline divider instead of a banner or message bubble', () => {
    const markup = renderToStaticMarkup(createElement(AcpSystemNoticeDivider, {
      event: {
        kind: 'systemNotice',
        raw: {
          systemNotice: {
            code: ACP_SESSION_CONFIG_ROLLED_BACK_CODE,
            params: { items: [{ category: 'model_config', configId: 'fast', value: 'true', name: 'Fast' }] },
          },
        },
      },
    }));

    expect(markup).toContain('data-acp-system-notice="true"');
    expect(markup).toContain('h-px');
    expect(markup).not.toContain('data-acp-error-banner');
    expect(markup).toMatch(/Fast 不支持，系统已将其回滚为不指定。可停止对话后修改。|Fast not supported by this model, and reset to unspecified\. Stop the conversation to change them\./);
  });
});
