import { INTERFACE_ZOOM_STEP } from '@/lib/desktop-zoom';

export function isDesktopPageZoomShortcut(
  key: string,
  ctrlKey: boolean,
  metaKey: boolean,
) {
  return (ctrlKey || metaKey) && ['+', '=', '-', '0'].includes(key);
}

/** Accumulated pixel distance required before one wheel gesture steps the zoom. */
const CTRL_WHEEL_STEP_THRESHOLD_PX = 80;
/** Chromium reports line-mode deltas (±3 per notch); normalize to pixel distance. */
const WHEEL_LINE_HEIGHT_PX = 33;

function wheelDeltaPixels(deltaY: number, deltaMode: number, viewportHeight: number) {
  if (deltaMode === 1) return deltaY * WHEEL_LINE_HEIGHT_PX;
  if (deltaMode === 2) return deltaY * viewportHeight;
  return deltaY;
}

/**
 * Accumulates ctrl+wheel deltas until they cross one notch of distance, then
 * steps the zoom once. Precision-touchpad pinches fire dozens of tiny pixel
 * deltas per second; stepping per event would sweep the whole zoom range in a
 * single gesture. Distance below the threshold carries over between events of
 * the same gesture; the remainder resets on each emitted step.
 */
export function nextCtrlWheelZoomStep(
  remainder: number,
  deltaY: number,
  deltaMode: number,
  viewportHeight: number,
): { step: number; remainder: number } {
  const total = remainder + wheelDeltaPixels(deltaY, deltaMode, viewportHeight);
  if (Math.abs(total) < CTRL_WHEEL_STEP_THRESHOLD_PX) {
    return { step: 0, remainder: total };
  }
  return { step: total < 0 ? INTERFACE_ZOOM_STEP : -INTERFACE_ZOOM_STEP, remainder: 0 };
}

/**
 * Zoom driver installed by the app shell. When present, the native page-zoom
 * gestures (Ctrl+wheel, Ctrl+`+/-/0`) are consumed and forwarded to the app's
 * owned zoom preference instead of the webview's untracked browser zoom.
 */
export interface DesktopPageZoomDriver {
  /** Adjust zoom by a signed step (positive zooms in). */
  adjust(deltaPercent: number): void;
  /** Reset zoom to the default percent. */
  reset(): void;
}

let driver: DesktopPageZoomDriver | null = null;

export function setDesktopPageZoomDriver(next: DesktopPageZoomDriver | null) {
  driver = next;
}

export function installDesktopPageZoomGuard(target: Window = window) {
  let wheelRemainder = 0;
  const preventPageWheelZoom = (event: WheelEvent) => {
    if (!event.ctrlKey) return;
    // Native webview zoom must never diverge from the persisted preference;
    // always consume the gesture, with or without an installed driver.
    event.preventDefault();
    const { step, remainder } = nextCtrlWheelZoomStep(
      wheelRemainder,
      event.deltaY,
      event.deltaMode,
      target.innerHeight,
    );
    wheelRemainder = remainder;
    if (step !== 0) driver?.adjust(step);
  };
  const preventPageKeyboardZoom = (event: KeyboardEvent) => {
    if (!isDesktopPageZoomShortcut(event.key, event.ctrlKey, event.metaKey)) return;
    event.preventDefault();
    if (event.key === '0') {
      driver?.reset();
      return;
    }
    driver?.adjust(event.key === '-' ? -INTERFACE_ZOOM_STEP : INTERFACE_ZOOM_STEP);
  };

  target.addEventListener('wheel', preventPageWheelZoom, { capture: true, passive: false });
  target.addEventListener('keydown', preventPageKeyboardZoom, { capture: true });
  return () => {
    target.removeEventListener('wheel', preventPageWheelZoom, { capture: true });
    target.removeEventListener('keydown', preventPageKeyboardZoom, { capture: true });
  };
}
