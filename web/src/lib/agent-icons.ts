import { cn } from '@/lib/utils';

const AGENT_ICON_SCALE_CLASS: Record<string, string> = {
  codex: 'scale-125',
  gemini: 'scale-110',
  opencode: 'scale-110',
};

const MONOCHROME_AGENT_ICONS = new Set([
  'amp-acp',
  'claude',
  'codebuddy-code',
  'codex',
  'cursor',
  'gemini',
  'goose',
  'kimi',
  'opencode',
  'pi-acp',
  'qwen-code',
]);

export const AGENT_ICON_ACCEPT = 'image/png,image/jpeg,image/webp,image/svg+xml';
export const MAX_AGENT_ICON_BYTES = 1024 * 1024;
export const DEFAULT_AGENT_ICON_KEY = 'gold-band';

const SUPPORTED_AGENT_ICON_MIME_TYPES = new Set(AGENT_ICON_ACCEPT.split(','));

export async function readAgentIconFile(file: File): Promise<string> {
  if (!SUPPORTED_AGENT_ICON_MIME_TYPES.has(file.type)) {
    throw new Error('agent-icon.unsupported-image-type');
  }
  if (file.size <= 0 || file.size > MAX_AGENT_ICON_BYTES) {
    throw new Error('agent-icon.image-too-large');
  }
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => typeof reader.result === 'string'
      ? resolve(reader.result)
      : reject(new Error('agent-icon.invalid-image-data'));
    reader.onerror = () => reject(new Error('agent-icon.invalid-image-data'));
    reader.readAsDataURL(file);
  });
}

export function agentIconSrc(iconKey: string) {
  const icon = iconKey.trim();
  if (!icon || icon === DEFAULT_AGENT_ICON_KEY) {
    return '/logo.svg';
  }
  if (/^(?:https?:|data:|asset:|blob:|\/)/i.test(icon)) return icon;
  return `/agent-icons/${icon}.svg`;
}

export function agentIconClass(iconKey: string, className?: string) {
  const normalizedIconKey = iconKey.trim();
  return cn(
    'object-contain',
    MONOCHROME_AGENT_ICONS.has(normalizedIconKey) && 'dark:invert',
    className,
    AGENT_ICON_SCALE_CLASS[normalizedIconKey],
  );
}
