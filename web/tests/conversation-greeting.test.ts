import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';
import {
  conversationGreetingPeriodAt,
  millisecondsUntilNextConversationGreeting,
  nextConversationGreetingBoundary,
} from '../src/lib/conversation-greeting';

const greetingSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/ConversationGreeting.tsx', import.meta.url)),
  'utf8',
);
const homeSource = readFileSync(
  fileURLToPath(new URL('../src/pages/ConversationHomePage.tsx', import.meta.url)),
  'utf8',
);

describe('conversation greeting periods in the system local timezone', () => {
  it.each([
    [new Date(2026, 6, 24, 4, 59), 'lateNight'],
    [new Date(2026, 6, 24, 5, 0), 'morning'],
    [new Date(2026, 6, 24, 8, 59), 'morning'],
    [new Date(2026, 6, 24, 9, 0), 'lateMorning'],
    [new Date(2026, 6, 24, 11, 29), 'lateMorning'],
    [new Date(2026, 6, 24, 11, 30), 'noon'],
    [new Date(2026, 6, 24, 13, 59), 'noon'],
    [new Date(2026, 6, 24, 14, 0), 'afternoon'],
    [new Date(2026, 6, 24, 18, 29), 'afternoon'],
    [new Date(2026, 6, 24, 18, 30), 'evening'],
    [new Date(2026, 6, 24, 23, 29), 'evening'],
    [new Date(2026, 6, 24, 23, 30), 'lateNight'],
  ])('maps %s to %s', (date, expected) => {
    expect(conversationGreetingPeriodAt(date)).toBe(expected);
  });

  it('schedules the next same-day boundary without polling', () => {
    const now = new Date(2026, 6, 24, 8, 50, 15, 250);
    const boundary = new Date(2026, 6, 24, 9, 0, 0, 0);

    expect(nextConversationGreetingBoundary(now)).toEqual(boundary);
    expect(millisecondsUntilNextConversationGreeting(now)).toBe(boundary.getTime() - now.getTime());
  });

  it('schedules 05:00 on the next local day after the final boundary', () => {
    const now = new Date(2026, 6, 24, 23, 45);

    expect(nextConversationGreetingBoundary(now)).toEqual(new Date(2026, 6, 25, 5, 0));
  });
});

describe('conversation greeting rendering contract', () => {
  it('isolates time state from the composer and refreshes only at boundaries or resume events', () => {
    expect(homeSource).toContain('<ConversationGreeting />');
    expect(greetingSource).toContain('text-foreground/80');
    expect(greetingSource).not.toContain('tracking-tight text-foreground"');
    expect(greetingSource).not.toContain('ConversationHelloMark');
    expect(greetingSource).toContain('window.setTimeout');
    expect(greetingSource).not.toContain('setInterval');
    expect(greetingSource).toContain("window.addEventListener('focus'");
    expect(greetingSource).toContain("document.addEventListener('visibilitychange'");
    expect(greetingSource).not.toContain('location.reload');
  });
});
