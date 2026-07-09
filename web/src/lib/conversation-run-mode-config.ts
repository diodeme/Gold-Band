import type { ConversationAutoConfigVm } from '@/types';

export function optionalRunModeText(value: string | null | undefined): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined;
}

export function normalizeOptionalRunModeText(value: string | null | undefined): string | undefined {
  if (typeof value !== 'string') return undefined;
  return value.trim().length > 0 ? value : undefined;
}

export function normalizeConversationAutoConfigForSubmit(
  config: ConversationAutoConfigVm | null | undefined,
): ConversationAutoConfigVm | undefined {
  if (!config) return undefined;
  return {
    ...config,
    globalGoal: normalizeOptionalRunModeText(config.globalGoal),
  };
}
