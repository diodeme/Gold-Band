import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import {
  ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT,
  AcpSystemNoticeDivider,
} from '@/components/acp/AcpSystemNotice';
import i18n from '@/i18n';
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

    expect(acpSystemNoticeCopy((key, values) => i18n.t(key, values), event))
      .toBe('当前Agent暂不支持配置：思考强度（effort）：high · 上下文（context）：1m。系统已将其回滚为不指定。可停止对话后修改。');
  });

  it('names multiple thought_level rollbacks without collapsing them', () => {
    const event = {
      kind: 'systemNotice',
      raw: {
        systemNotice: {
          code: ACP_SESSION_CONFIG_ROLLED_BACK_CODE,
          params: {
            items: [
              { category: 'thought_level', configId: 'thinking', value: 'true', name: 'Thinking' },
              { category: 'thought_level', configId: 'effort', value: 'medium', name: 'Effort' },
            ],
          },
        },
      },
    };

    expect(acpSystemNoticeCopy((key, values) => i18n.t(key, values), event))
      .toBe('当前Agent暂不支持配置：深度思考（thinking）：true · 思考强度（effort）：medium。系统已将其回滚为不指定。可停止对话后修改。');
  });

  it('includes the rejected permission mode value', () => {
    const event = {
      kind: 'systemNotice',
      raw: {
        systemNotice: {
          code: ACP_SESSION_CONFIG_ROLLED_BACK_CODE,
          params: {
            items: [{ category: 'mode', configId: 'mode', value: 'bypassPermissions' }],
          },
        },
      },
    };

    expect(acpSystemNoticeCopy((key, values) => i18n.t(key, values), event))
      .toBe('当前Agent暂不支持配置：mode：bypassPermissions。系统已将其回滚为不指定。可停止对话后修改。');
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

    expect(acpSystemNoticeCopy((key, values) => i18n.t(key, values), event))
      .toBe('当前Agent暂不支持配置：fast：true。系统已将其回滚为不指定。可停止对话后修改。');
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
    expect(markup).toMatch(/当前Agent暂不支持配置：Fast：true。系统已将其回滚为不指定。可停止对话后修改。|This Agent does not currently support: Fast: true\. The system has rolled it back to unspecified\. You can change it after stopping the conversation\./);
  });

  it('shrinks the side rules to a floor before wrapping the notice copy', () => {
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

    expect(markup).toContain(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.rootClassName);
    expect(markup).toContain(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.lineClassName);
    expect(markup).toContain(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.copyClassName);
    expect(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.rootClassName).toContain('min-w-0');
    expect(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.lineClassName).toContain('flex-1');
    expect(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.lineClassName).toContain('min-w-6');
    expect(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.lineClassName).toContain('bg-border');
    expect(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.lineClassName).not.toContain('bg-border/');
    expect(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.copyClassName).toContain('min-w-0');
    expect(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.copyClassName).not.toMatch(/max-w-/);
    expect(ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.copyClassName).not.toContain('shrink-0');
  });
});
