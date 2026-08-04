import { cn } from '@/lib/utils';

const AGENT_ICON_SCALE_CLASS: Record<string, string> = {
  codex: 'scale-125',
  gemini: 'scale-110',
  opencode: 'scale-110',
  kimi: 'scale-110',
};

export function agentIconSrc(iconKey: string) {
  if (iconKey === 'gold-band') {
    return '/logo.svg';
  }
  return `/agent-icons/${iconKey}.svg`;
}

export function agentIconClass(iconKey: string, className?: string) {
  return cn('object-contain', className, AGENT_ICON_SCALE_CLASS[iconKey]);
}
