import { z } from 'zod';
import { normalizeAppearancePreference } from '@/theme';
import { MAX_FONT_STACK_FAMILIES, MAX_FONT_FAMILY_CODE_POINTS } from '@/theme-contract';
import type { AppearancePreference, PreferencesVm } from '@/types';

export const DEMO_PREFERENCES_KEY = 'gold-band.demo.preferences.v1';
export const DEMO_LAYOUT_KEY = 'gold-band.demo.layout.v1';
const layoutSchema = z.object({
  'sidebar.width': z.number().finite().positive().optional(),
  'rightWorkspace.width': z.number().finite().positive().optional(),
  'pinned.collapsed': z.boolean().optional(),
}).strict();
export function readDemoLayout(storage?: Pick<Storage, 'getItem'>): Record<string, unknown> {
  try {
    const parsed = layoutSchema.safeParse(JSON.parse(storage?.getItem(DEMO_LAYOUT_KEY) ?? '{}'));
    return parsed.success ? parsed.data : {};
  } catch { return {}; }
}
export function writeDemoLayout(storage: Pick<Storage, 'setItem'> | undefined, layout: Record<string, unknown>) {
  const validated = layoutSchema.parse(layout);
  try { storage?.setItem(DEMO_LAYOUT_KEY, JSON.stringify(validated)); } catch { /* Layout remains usable in memory. */ }
}
const typographySchema = z.object({
  fontStack: z.discriminatedUnion('source', [
    z.object({ source: z.literal('theme') }),
    z.object({ source: z.literal('custom'), families: z.array(z.string().min(1).max(MAX_FONT_FAMILY_CODE_POINTS)).min(1).max(MAX_FONT_STACK_FAMILIES) }),
  ]),
  fontSize: z.discriminatedUnion('source', [
    z.object({ source: z.literal('theme') }),
    z.object({ source: z.literal('custom'), px: z.number().finite() }),
  ]),
});
const savedSchema = z.object({
  version: z.literal(1),
  appearance: z.unknown().transform((value) => normalizeAppearancePreference(value as AppearancePreference)),
  typography: z.object({ ui: typographySchema, editor: typographySchema }),
  language: z.enum(['en', 'zh-cn']),
});

export function readDemoPreferences(storage: Pick<Storage, 'getItem'> | undefined, defaults: PreferencesVm): PreferencesVm {
  try {
    const saved = savedSchema.safeParse(JSON.parse(storage?.getItem(DEMO_PREFERENCES_KEY) ?? 'null'));
    return saved.success ? { ...defaults, appearance: saved.data.appearance, language: saved.data.language,
      personalization: { ...defaults.personalization, typography: saved.data.typography } } : defaults;
  } catch { return defaults; }
}

export function writeDemoPreferences(storage: Pick<Storage, 'setItem'> | undefined, preferences: PreferencesVm) {
  try {
    storage?.setItem(DEMO_PREFERENCES_KEY, JSON.stringify({ version: 1, appearance: preferences.appearance,
      typography: preferences.personalization.typography, language: preferences.language }));
  } catch { /* Preferences remain usable when browser storage is unavailable. */ }
}
