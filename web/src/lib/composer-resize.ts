/**
 * Manual composer input resizing. The composer input keeps its autosize
 * behavior by default; dragging the top grip pins an explicit height that the
 * input scroll container owns. Dragging back below the auto threshold and
 * releasing returns to autosize. The height is remembered across sessions,
 * mirroring the sheet resizeStorageKey convention.
 */

/** Matches the autosize floor (min-h-12) of the composer textarea. */
export const COMPOSER_INPUT_MIN_HEIGHT_PX = 48;
/** Dragging to or below this height and releasing returns to autosize. */
export const COMPOSER_INPUT_AUTO_DROP_PX = 64;
/** Manual height never exceeds this share of the viewport. */
export const COMPOSER_INPUT_MAX_VIEWPORT_SHARE = 0.4;
export const COMPOSER_INPUT_KEYBOARD_STEP_PX = 24;

export const COMPOSER_INPUT_HEIGHT_STORAGE_KEY = 'composer/input-height-px';

export function composerInputMaxHeightPx(): number {
  if (typeof window === 'undefined') return 320;
  return Math.max(
    COMPOSER_INPUT_AUTO_DROP_PX + 1,
    Math.floor(window.innerHeight * COMPOSER_INPUT_MAX_VIEWPORT_SHARE),
  );
}

export function clampComposerInputHeight(value: number): number {
  if (!Number.isFinite(value)) return COMPOSER_INPUT_AUTO_DROP_PX + 1;
  return Math.min(composerInputMaxHeightPx(), Math.max(COMPOSER_INPUT_MIN_HEIGHT_PX, Math.round(value)));
}

export function readStoredComposerInputHeight(): number | null {
  if (typeof window === 'undefined') return null;
  const raw = window.localStorage.getItem(COMPOSER_INPUT_HEIGHT_STORAGE_KEY);
  if (!raw) return null;
  const parsed = Number.parseInt(raw, 10);
  if (!Number.isFinite(parsed)) return null;
  // Stored heights clamp against the current viewport so a height picked on a
  // large display cannot swallow a smaller window after relaunch.
  if (parsed <= COMPOSER_INPUT_AUTO_DROP_PX) return null;
  return clampComposerInputHeight(parsed);
}

export function storeComposerInputHeight(value: number | null): void {
  if (typeof window === 'undefined') return;
  if (value === null) {
    window.localStorage.removeItem(COMPOSER_INPUT_HEIGHT_STORAGE_KEY);
    return;
  }
  window.localStorage.setItem(COMPOSER_INPUT_HEIGHT_STORAGE_KEY, String(clampComposerInputHeight(value)));
}
