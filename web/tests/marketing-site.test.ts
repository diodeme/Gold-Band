import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { CHAPTER_IDS, copy, pageHref, parseRoute, posterPath, SCENE_DIRECTORIES } from '../../marketing/site/content';
import { effectiveTheme, PREFERENCES_KEY, readPreferences, writePreferences } from '../../marketing/site/preferences';
import { createPreviewRun } from '../../marketing/shared/fixture';
import { mockErrorBlockedConversationRun } from '../src/mockData';

describe('website routing and bilingual content', () => {
  it('preserves page identity when changing language and handles unknown paths', () => {
    for (const language of ['zh', 'en'] as const) for (const page of ['home', 'documentation', 'demo'] as const) {
      expect(parseRoute(pageHref(language, page))).toEqual({ language, page });
    }
    expect(parseRoute('/en/not-a-page').page).toBe('not-found');
    expect(parseRoute('/')).toEqual({ language: 'zh', page: 'home' });
    expect(pageHref('en', 'documentation', '/product/')).toBe('/product/en/documentation');
    expect(parseRoute('/', 'en')).toEqual({ language: 'en', page: 'home' });
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
      const text = readFileSync(`marketing/site/media/${SCENE_DIRECTORIES[chapter]}/${language}-dark/events.json`, 'utf8');
      const recording = JSON.parse(text);
      const resources = JSON.parse(readFileSync(`marketing/site/media/${SCENE_DIRECTORIES[chapter]}/${language}-dark/resources.json`, 'utf8'));
      expect(resources.resources.every((resource: { url: string }) => resource.url.startsWith('/media/'))).toBe(true);
      expect(recording.events.length).toBeLessThan(12000);
      expect(recording.bytes).toBeLessThan(12 * 1024 * 1024);
      expect(recording.events.some((event: { type: number; data: { source?: number } }) => event.type === 3 && event.data.source === 0)).toBe(true);
      expect(recording.events.some((event: { type: number }) => event.type === 2)).toBe(true);
    }
  });
});
describe('site preferences and formal posters', () => {
  it('uses a versioned preference and handles unavailable or malformed storage', () => {
    const values = new Map<string, string>();
    const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value); } };
    expect(readPreferences(storage, 'en-US')).toEqual({ version: 1, language: 'en', theme: 'system' });
    writePreferences(storage, { version: 1, language: 'zh', theme: 'dark' });
    expect(readPreferences(storage, 'en').language).toBe('zh');
    values.set(PREFERENCES_KEY, '{"version":2}');
    expect(readPreferences(storage, 'fr').theme).toBe('system');
    expect(readPreferences({ getItem: () => { throw Error(); } }, 'en').language).toBe('en');
    expect(() => writePreferences({ setItem: () => { throw Error(); } }, { version: 1, language: 'zh', theme: 'light' })).not.toThrow();
    expect(effectiveTheme('system', true)).toBe('dark');
    expect(effectiveTheme('system', false)).toBe('light');
    expect(effectiveTheme('light', true)).toBe('light');
  });
  it('selects a formal matching poster for every scene, language, theme and container', () => {
    for (const chapter of CHAPTER_IDS) for (const language of ['zh', 'en'] as const) for (const theme of ['dark', 'light'] as const) for (const mobile of [false, true]) {
      const url = posterPath('/product/', language, chapter, theme, mobile);
      expect(url).toContain(`/product/media/${SCENE_DIRECTORIES[chapter]}/${language}-${theme}${mobile ? '-mobile' : ''}/`);
      expect(readFileSync(`marketing/site/${url.slice('/product/'.length)}`).length).toBeGreaterThan(0);
    }
  });
});
