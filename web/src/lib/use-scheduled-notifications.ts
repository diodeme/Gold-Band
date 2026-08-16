import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';

import {
  sendScheduledNativeNotification,
  subscribeScheduledNotifications,
} from '../api';
import { scheduledNotificationCopy } from './scheduled-task-notifications';

export function useScheduledNotifications(): void {
  const { t } = useTranslation();
  const translateRef = useRef(t);
  useEffect(() => {
    translateRef.current = t;
  }, [t]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void subscribeScheduledNotifications((event) => {
      if (!active) return;
      const copy = scheduledNotificationCopy(event, translateRef.current);
      void sendScheduledNativeNotification({ ...event, ...copy });
    }).then((off) => {
      if (active) unlisten = off;
      else off();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
}
