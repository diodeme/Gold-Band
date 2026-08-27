import { useCallback, useEffect, useState } from 'react';

export const SCHEDULED_TASK_CREATED_NOTICE_DURATION_MS = 5000;

export function useScheduledTaskCreatedNotice() {
  const [visible, setVisible] = useState(false);
  const show = useCallback(() => setVisible(true), []);
  const dismiss = useCallback(() => setVisible(false), []);

  useEffect(() => {
    if (!visible) return undefined;
    const timer = window.setTimeout(dismiss, SCHEDULED_TASK_CREATED_NOTICE_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [dismiss, visible]);

  return { visible, show, dismiss } as const;
}
