import type { AcpUiEventVm } from '@/types';

export const ACP_SESSION_CONFIG_ROLLED_BACK_CODE = 'acp.session-config-rolled-back';
export const ACP_ROLLED_BACK_CONFIG_NAME_SEPARATOR = ' · ';

type Translate = (key: string, values?: Record<string, unknown>) => string;

type RolledBackConfigItem = {
  category: string;
  configId: string;
  value: string;
  name: string | null;
};

function rawObject(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

export function acpSystemNoticePayload(event: Pick<AcpUiEventVm, 'raw'> | null | undefined) {
  const raw = rawObject(event?.raw);
  const notice = rawObject(raw?.systemNotice);
  const code = typeof notice?.code === 'string' ? notice.code.trim() : '';
  if (!code) return null;
  return {
    code,
    params: rawObject(notice?.params) ?? {},
  };
}

function rolledBackConfigItems(params: Record<string, unknown>): RolledBackConfigItem[] {
  const items = params.items;
  if (!Array.isArray(items)) return [];
  return items.flatMap((item) => {
    const raw = rawObject(item);
    if (!raw) return [];
    const category = typeof raw.category === 'string' ? raw.category.trim() : '';
    const configId = typeof raw.configId === 'string' ? raw.configId.trim() : '';
    const value = typeof raw.value === 'string' ? raw.value.trim() : '';
    const name = typeof raw.name === 'string' ? raw.name.trim() : '';
    if (!category && !configId) return [];
    return [{
      category,
      configId,
      value,
      name: name || null,
    }];
  });
}

export function acpRolledBackConfigNames(
  t: Translate,
  items: RolledBackConfigItem[],
): string[] {
  const names: string[] = [];
  const seen = new Set<string>();
  const ordered = [
    ...items.filter((item) => item.category === 'thought_level'),
    ...items.filter((item) => item.category !== 'thought_level'),
  ];
  for (const item of ordered) {
    const name = item.category === 'thought_level'
      ? t('acp.thoughtLevel')
      : (item.name || item.configId);
    if (!name || seen.has(name)) continue;
    seen.add(name);
    names.push(name);
  }
  return names;
}

export function acpSystemNoticeCopy(
  t: Translate,
  event: Pick<AcpUiEventVm, 'kind' | 'raw'> | null | undefined,
) {
  const notice = acpSystemNoticePayload(event);
  if (!notice) return null;
  if (notice.code === ACP_SESSION_CONFIG_ROLLED_BACK_CODE) {
    const names = acpRolledBackConfigNames(t, rolledBackConfigItems(notice.params))
      .join(ACP_ROLLED_BACK_CONFIG_NAME_SEPARATOR);
    return t('acp.sessionConfigRolledBack', { names });
  }
  return null;
}
