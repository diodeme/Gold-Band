export type ConversationGreetingPeriod = 'morning' | 'lateMorning' | 'noon' | 'afternoon' | 'evening' | 'lateNight';

interface ConversationGreetingTransition {
  hour: number;
  minute: number;
  period: ConversationGreetingPeriod;
}

const CONVERSATION_GREETING_TRANSITIONS: readonly ConversationGreetingTransition[] = [
  { hour: 5, minute: 0, period: 'morning' },
  { hour: 9, minute: 0, period: 'lateMorning' },
  { hour: 11, minute: 30, period: 'noon' },
  { hour: 14, minute: 0, period: 'afternoon' },
  { hour: 18, minute: 30, period: 'evening' },
  { hour: 23, minute: 30, period: 'lateNight' },
];

const minutesSinceLocalMidnight = (date: Date) => date.getHours() * 60 + date.getMinutes();
const transitionMinute = (transition: ConversationGreetingTransition) => (
  transition.hour * 60 + transition.minute
);

export function conversationGreetingPeriodAt(date: Date): ConversationGreetingPeriod {
  const currentMinute = minutesSinceLocalMidnight(date);
  let period: ConversationGreetingPeriod = 'lateNight';

  for (const transition of CONVERSATION_GREETING_TRANSITIONS) {
    if (currentMinute < transitionMinute(transition)) break;
    period = transition.period;
  }

  return period;
}

export function nextConversationGreetingBoundary(date: Date): Date {
  const currentMinute = minutesSinceLocalMidnight(date);
  const nextTransition = CONVERSATION_GREETING_TRANSITIONS.find(
    (transition) => transitionMinute(transition) > currentMinute,
  );
  const boundary = new Date(date);

  if (nextTransition) {
    boundary.setHours(nextTransition.hour, nextTransition.minute, 0, 0);
    return boundary;
  }

  const firstTransition = CONVERSATION_GREETING_TRANSITIONS[0];
  boundary.setDate(boundary.getDate() + 1);
  boundary.setHours(firstTransition.hour, firstTransition.minute, 0, 0);
  return boundary;
}

export function millisecondsUntilNextConversationGreeting(date: Date): number {
  return Math.max(0, nextConversationGreetingBoundary(date).getTime() - date.getTime());
}
