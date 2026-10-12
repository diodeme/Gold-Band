import { describe, expect, it } from 'vitest';

import en from '@/locales/en.json';
import es from '@/locales/es.json';
import jaJP from '@/locales/ja-JP.json';
import koKR from '@/locales/ko-KR.json';
import ptBR from '@/locales/pt-BR.json';
import zhCN from '@/locales/zh-CN.json';
import zhTW from '@/locales/zh-TW.json';

/// 远程任务域 locale 对等回归（M5-br）：
/// 词汇中立化迁移曾在 en/zh-CN 之外的语言里整块删除 multica 域文案且未补 remote 域新键，
/// 导致需求管理页在五个语言下回退英文、multica.* 错误码只剩通用兜底文案。
/// 本测试以 en 为基准锁定：remote 域所有叶子键在全部七个语言对等存在、插值占位符一致、
/// 后端仍在发射的 multica.* 错误码文案表完整。
/// 注意：不能经 i18n.t 判断存在性——fallbackLng 会用英文兜底掩盖缺失，必须直读资源文件。

const LOCALES = {
  en,
  'zh-CN': zhCN,
  'ja-JP': jaJP,
  es,
  'ko-KR': koKR,
  'pt-BR': ptBR,
  'zh-TW': zhTW,
} as const;

const DOMAIN_PREFIXES = [
  'remote.',
  'errors.multica.',
  'errors.remote.',
  'conversation.sidebar.remoteTask',
  'conversation.composer.remote',
  'contextManagement.skills.remoteSync',
  'acp.hiddenRemoteTaskContext',
] as const;

/// 后端仍在发射的 multica.* 错误码（src-tauri/src/multica/error.rs `code()` 码表 +
/// commands.rs 的 workspace-already-bound / workspace-not-found）。
/// not-configured 已迁移为 errors.remote.source-not-configured，不得复活旧键。
const MULTICA_ERROR_CODES = [
  'auth-failed',
  'claim-conflict',
  'connect-cancelled',
  'invalid-address',
  'network-failed',
  'register-failed',
  'runtime-offline',
  'session-resume-failed',
  'task-not-found',
  'workspace-already-bound',
  'workspace-not-found',
] as const;

function collectLeaves(node: unknown, prefix = '', out = new Map<string, string>()): Map<string, string> {
  if (typeof node === 'string') {
    out.set(prefix, node);
  } else if (node && typeof node === 'object') {
    for (const [k, v] of Object.entries(node)) {
      collectLeaves(v, prefix ? `${prefix}.${k}` : k, out);
    }
  }
  return out;
}

/// 逐段下钻查找；键名含点（reason.multica.skill.* 等字面键）时回退为拼键直查。
function lookupLeaf(root: unknown, path: string): string | undefined {
  let cur: unknown = root;
  const parts = path.split('.');
  for (let i = 0; i < parts.length; i += 1) {
    if (cur && typeof cur === 'object' && parts[i] in (cur as Record<string, unknown>)) {
      cur = (cur as Record<string, unknown>)[parts[i]];
      continue;
    }
    const joined = parts.slice(i).join('.');
    if (cur && typeof cur === 'object' && joined in (cur as Record<string, unknown>)) {
      return (cur as Record<string, unknown>)[joined] as string;
    }
    return undefined;
  }
  return typeof cur === 'string' ? cur : undefined;
}

function placeholders(text: string): string[] {
  return [...text.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]).sort();
}

const enLeaves = collectLeaves(en);
const domainKeys = [...enLeaves.keys()].filter((k) => DOMAIN_PREFIXES.some((p) => k.startsWith(p)));

describe('remote-domain locale parity', () => {
  it('covers every remote-domain key in all seven locales', () => {
    expect(domainKeys.length).toBeGreaterThan(80);
    for (const [lng, resource] of Object.entries(LOCALES)) {
      const missing = domainKeys.filter((k) => lookupLeaf(resource, k) === undefined);
      expect(missing, `${lng} missing: ${missing.join(', ')}`).toEqual([]);
    }
  });

  it('keeps interpolation placeholders identical to en in every locale', () => {
    for (const [lng, resource] of Object.entries(LOCALES)) {
      for (const key of domainKeys) {
        const translated = lookupLeaf(resource, key);
        if (translated === undefined) continue;
        expect(`${lng} ${key}: ${placeholders(translated).join(',')}`).toBe(
          `${lng} ${key}: ${placeholders(enLeaves.get(key) ?? '').join(',')}`,
        );
      }
    }
  });

  it('restores the multica error-code table in every locale', () => {
    for (const [lng, resource] of Object.entries(LOCALES)) {
      for (const code of MULTICA_ERROR_CODES) {
        expect(lookupLeaf(resource, `errors.multica.${code}`), `${lng} errors.multica.${code}`).toBeDefined();
      }
      expect(lookupLeaf(resource, 'errors.multica.not-configured'), `${lng} not-configured 应保持迁移`).toBeUndefined();
      expect(lookupLeaf(resource, 'errors.remote.source-not-configured')).toBeDefined();
    }
    // 抽查译文仍是迁移前的既有措辞（防止恢复成英文占位）。
    expect(lookupLeaf(zhCN, 'errors.multica.claim-conflict')).toBe('该任务已被其他 Daemon 领取。');
    expect(lookupLeaf(en, 'errors.multica.claim-conflict')).toBe(
      'This task has already been claimed by another daemon.',
    );
    expect(lookupLeaf(jaJP, 'contextManagement.skills.remoteSync.selectAll')).toBe('すべて選択');
  });
});
