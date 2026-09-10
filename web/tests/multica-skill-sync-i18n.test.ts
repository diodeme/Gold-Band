import { describe, expect, it } from 'vitest';

import i18n from '@/i18n';

/// 「从 Multica 同步 SKILL」弹窗文案的 zh/en 对等回归：
/// 1) 所有 multicaSync key 两种语言都可解析（缺失时 i18next 原样返回 key，断言不等于 key 即非缺失）。
/// 2) 含点错误码（multica.skill.has-files 等）作为 reason.* 子 key 可解析——i18next
///    ignoreJSONStructure 默认开启，扁平 key 含点也能命中；此处固化该前提，升级 i18next 行为变化时此处报警。
/// 3) 插值（覆盖数量 / 汇总统计）两种语言都生效。

const BASE = 'contextManagement.skills.multicaSync';
const KEYS = [
  'action',
  'title',
  'workspace',
  'reload',
  'newBadge',
  'existsBadge',
  'confirm',
  'confirmWithOverwrite',
  'empty',
  'notConnected',
  'loadFailed',
  'pulling',
  'reportTitle',
  'reportSummary',
  'outcomeCreated',
  'outcomeOverwritten',
  'outcomeSkipped',
  'outcomeFailed',
] as const;
const REASON_CODES = [
  'multica.skill.has-files',
  'multica.skill.not-found',
  'multica.skill.name-collision',
  'multica.remote-error',
  'multica.not-connected',
] as const;
const LANGUAGES = ['zh-CN', 'en'] as const;

describe('Multica skill sync localization', () => {
  it('resolves every multicaSync key in both languages', () => {
    for (const lng of LANGUAGES) {
      for (const key of KEYS) {
        const full = `${BASE}.${key}`;
        expect(i18n.t(full, { lng }), `${lng} ${key}`).not.toBe(full);
      }
    }
  });

  it('resolves dotted error-code reason keys in both languages', () => {
    for (const lng of LANGUAGES) {
      for (const code of REASON_CODES) {
        const full = `${BASE}.reason.${code}`;
        expect(i18n.t(full, { lng }), `${lng} ${code}`).not.toBe(full);
      }
    }
  });

  it('interpolates counts in confirm and report strings', () => {
    expect(i18n.t(`${BASE}.confirmWithOverwrite`, { lng: 'zh-CN', count: 3 })).toContain('3');
    expect(i18n.t(`${BASE}.confirmWithOverwrite`, { lng: 'en', count: 2 })).toContain('2');
    const summary = i18n.t(`${BASE}.reportSummary`, {
      lng: 'en',
      created: 1,
      overwritten: 2,
      skipped: 3,
      failed: 4,
    });
    expect(summary).toContain('Created 1');
    expect(summary).toContain('Failed 4');
  });
});
