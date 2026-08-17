import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  conversationGreetingPeriodAt,
  millisecondsUntilNextConversationGreeting,
  type ConversationGreetingPeriod,
} from '@/lib/conversation-greeting';

const GREETING_BOUNDARY_DRIFT_MS = 50;

export function ConversationGreeting() {
  const { t } = useTranslation();
  const [period, setPeriod] = useState<ConversationGreetingPeriod>(() => (
    conversationGreetingPeriodAt(new Date())
  ));

  useEffect(() => {
    let boundaryTimer: number | undefined;

    const refreshAndSchedule = () => {
      const now = new Date();
      setPeriod(conversationGreetingPeriodAt(now));
      window.clearTimeout(boundaryTimer);
      boundaryTimer = window.setTimeout(
        refreshAndSchedule,
        millisecondsUntilNextConversationGreeting(now) + GREETING_BOUNDARY_DRIFT_MS,
      );
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === 'visible') refreshAndSchedule();
    };

    refreshAndSchedule();
    window.addEventListener('focus', refreshAndSchedule);
    document.addEventListener('visibilitychange', refreshWhenVisible);

    return () => {
      window.clearTimeout(boundaryTimer);
      window.removeEventListener('focus', refreshAndSchedule);
      document.removeEventListener('visibilitychange', refreshWhenVisible);
    };
  }, []);

  return (
    <h1 className="text-3xl font-semibold tracking-tight text-title">
      {t(`conversation.home.greeting.${period}`)}
    </h1>
  );
}
