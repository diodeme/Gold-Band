import { getRuntimeApi } from '@/api/client';
import type { PreferencesVm } from '@/types';

export const INTERFACE_ZOOM_MIN = 50;
export const INTERFACE_ZOOM_MAX = 250;
export const INTERFACE_ZOOM_DEFAULT = 100;
/** Step used by the settings slider and the Ctrl+wheel / Ctrl+`+/-/0` shortcuts. */
export const INTERFACE_ZOOM_STEP = 5;

export function normalizeInterfaceZoomPercent(value: number): number {
  if (!Number.isFinite(value)) return INTERFACE_ZOOM_DEFAULT;
  const stepped = Math.round(value / INTERFACE_ZOOM_STEP) * INTERFACE_ZOOM_STEP;
  return clampInterfaceZoomPercent(stepped);
}

/** Clamp-only bound for values whose exact number the user chose (manual input). */
export function clampInterfaceZoomPercent(value: number): number {
  if (!Number.isFinite(value)) return INTERFACE_ZOOM_DEFAULT;
  return Math.min(INTERFACE_ZOOM_MAX, Math.max(INTERFACE_ZOOM_MIN, Math.round(value)));
}

/** Result of parsing manual zoom input: invalid text, or a clamped percent. */
export type InterfaceZoomInput =
  | { kind: 'invalid' }
  | { kind: 'valid'; percent: number };

/**
 * Parses the manual zoom input. Non-numeric text (including empty input and
 * stray characters) is invalid and must not change the zoom; a numeric value
 * outside 50–250 clamps to the nearest bound and the slider follows.
 */
export function parseInterfaceZoomInput(raw: string): InterfaceZoomInput {
  const trimmed = raw.trim();
  if (!/^[+-]?\d+(\.\d+)?$/.test(trimmed)) return { kind: 'invalid' };
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) return { kind: 'invalid' };
  return { kind: 'valid', percent: clampInterfaceZoomPercent(parsed) };
}

/**
 * Applies a zoom percent to the desktop webview without persisting it.
 * Persistence flows through the single-field `saveInterfaceZoomPercent`
 * (settings slider and zoom shortcuts) or the full `saveDesktopPreferences`
 * save from the settings page.
 */
export async function previewInterfaceZoomPercent(percent: number): Promise<number> {
  return getRuntimeApi().applyDesktopZoom(normalizeInterfaceZoomPercent(percent));
}

export function withInterfaceZoomPercent(
  personalization: PreferencesVm['personalization'],
  percent: number,
): PreferencesVm['personalization'] {
  // Clamp-only: manual input keeps the exact integer the user typed.
  return { ...personalization, interfaceZoomPercent: clampInterfaceZoomPercent(percent) };
}
