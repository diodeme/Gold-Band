export function isDesktopPageZoomShortcut(
  key: string,
  ctrlKey: boolean,
  metaKey: boolean,
) {
  return (ctrlKey || metaKey) && ['+', '=', '-', '0'].includes(key);
}

export function installDesktopPageZoomGuard(target: Window = window) {
  const preventPageWheelZoom = (event: WheelEvent) => {
    if (event.ctrlKey) event.preventDefault();
  };
  const preventPageKeyboardZoom = (event: KeyboardEvent) => {
    if (isDesktopPageZoomShortcut(event.key, event.ctrlKey, event.metaKey)) {
      event.preventDefault();
    }
  };

  target.addEventListener('wheel', preventPageWheelZoom, { capture: true, passive: false });
  target.addEventListener('keydown', preventPageKeyboardZoom, { capture: true });
  return () => {
    target.removeEventListener('wheel', preventPageWheelZoom, { capture: true });
    target.removeEventListener('keydown', preventPageKeyboardZoom, { capture: true });
  };
}
