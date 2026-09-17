import type { AcpUiEventVm } from '@/types';

export const ACP_SESSION_CONFIG_ROLLED_BACK_CODE = 'acp.session-config-rolled-back';

type Translate = (key: string, values?: Record<string, unknown>) => string;

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

export function acpSystemNoticeCopy(
  t: Translate,
  event: Pick<AcpUiEventVm, 'kind' | 'raw'> | null | undefined,
) {
  const notice = acpSystemNoticePayload(event);
  if (!notice) return null;
  if (notice.code === ACP_SESSION_CONFIG_ROLLED_BACK_CODE) {
    return t('acp.sessionConfigRolledBack');
  }
  return null;
}
