const ACTIVITY_DEBOUNCE_MS = 60_000;
const ACTIVITY_EVENTS = ['pointerdown', 'keydown', 'focus'] as const;

type ActivityEventTarget = Pick<Window, 'addEventListener' | 'removeEventListener'>;

export function registerHeartbeatActivityListeners(
  reportActivity: () => void,
  target: ActivityEventTarget = window,
  now: () => number = Date.now,
) {
  let lastReportedAt = Number.NEGATIVE_INFINITY;
  const onActivity = () => {
    const currentTime = now();
    if (currentTime - lastReportedAt < ACTIVITY_DEBOUNCE_MS) return;
    lastReportedAt = currentTime;
    reportActivity();
  };

  for (const eventName of ACTIVITY_EVENTS) {
    target.addEventListener(eventName, onActivity);
  }

  return () => {
    for (const eventName of ACTIVITY_EVENTS) {
      target.removeEventListener(eventName, onActivity);
    }
  };
}
