import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { CHAPTER_IDS, copy, pageHref, parseRoute } from '../../marketing/site/content';
import { createPreviewRun } from '../../marketing/site/fixture';
import { mockErrorBlockedConversationRun } from '../src/mockData';

describe('website routing and bilingual content', () => {
  it('preserves page identity when changing language and handles unknown paths', () => {
    for (const language of ['zh', 'en'] as const) for (const page of ['home', 'documentation', 'demo'] as const) {
      expect(parseRoute(pageHref(language, page))).toEqual({ language, page });
    }
    expect(parseRoute('/en/not-a-page').page).toBe('not-found');
    expect(parseRoute('/')).toEqual({ language: 'zh', page: 'home' });
  });
  it('keeps the same four chapters in both languages', () => {
    for (const language of ['zh', 'en'] as const) expect(copy[language].chapters.map(chapter => chapter.id)).toEqual(CHAPTER_IDS);
  });
});
describe('curated product preview projections', () => {
  it('derives every lifecycle projection from the scenario and keeps the source intact', () => {
    const original = structuredClone(mockErrorBlockedConversationRun);
    for (const step of [1, 2, 3, 4, 5]) {
      const run = createPreviewRun(original, 'zh', step);
      const leaf = run.sessionTree.rounds[0].nodes[0].attempts[0];
      const done = step === 5;
      expect(run.runStatus).toBe(done ? 'completed' : 'running');
      expect(leaf.status).toBe(run.runStatus);
      expect(leaf.lifecycle?.runtime.outcome).toBe(done ? 'success' : null);
      expect(leaf.lifecycle?.acp.liveTurnActivity).toBe(done ? 'idle' : 'running');
      expect(leaf.lifecycle?.composer.canStop).toBe(!done);
      expect(leaf.runtimeDisplay.blockingError).toBe(false);
      expect(run.sessionTree.rounds[0].runtimeDisplay).toEqual(leaf.runtimeDisplay);
      expect(run.sessionTree.rounds[0].nodes[0].runtimeDisplay).toEqual(leaf.runtimeDisplay);
      expect(run.selectedSession?.events).toHaveLength(step);
      expect(run.selectedSession?.eventPage.total).toBe(step);
      expect(run.selectedSession?.systemPromptAppend).toBeNull();
      expect(run.runtimeErrorMessage).toBeNull();
    }
    expect(original).toEqual(mockErrorBlockedConversationRun);
  });
  it('ships portable, bounded bilingual recordings with actual changes', () => {
    for (const language of ['zh', 'en']) for (const chapter of CHAPTER_IDS) {
      const text = readFileSync(`marketing/site/media/${language}-${chapter}.json`, 'utf8');
      const recording = JSON.parse(text);
      expect(text).not.toContain('127.0.0.1');
      expect(recording.events.length).toBeLessThan(12000);
      expect(recording.bytes).toBeLessThan(12 * 1024 * 1024);
      expect(recording.events.some((event: { type: number; data: { source?: number } }) => event.type === 3 && event.data.source === 0)).toBe(true);
      expect(recording.events.some((event: { type: number }) => event.type === 2)).toBe(true);
    }
  });
});
