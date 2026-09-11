import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { CHAPTER_IDS, chapterFromHash, copy, demoHref, DESKTOP_QUERY, mediaPath, pageHref, parseRoute } from '../../marketing/site/content';
import { checkpointAt, readRecording, restoreCheckpoint } from '../../marketing/site/timeline';
import { createPreviewRun, previewTask } from '../../marketing/site/fixture';
import { mockErrorBlockedConversationRun } from '../src/mockData';
import { conversationPageForRun, conversationSourceControlWorkspacePath } from '../src/lib/conversation-navigation';

describe('website routing and bilingual content', () => {
  it('gives the embedded Demo a full viewport without changing the header or page frame', () => {
    const css = readFileSync('marketing/site/style.css', 'utf8');
    expect(css).toMatch(/\.site-demo-frame\s*\{[^}]*height:\s*100dvh/);
    expect(css).toMatch(/\.site-with-demo \.site-header\s*\{[^}]*position:\s*relative/);
    // The Demo route must not widen the page frame, which moved the header away from the other routes.
    expect(css).not.toMatch(/\.site\.site-with-demo\s*\{[^}]*max-width/);
  });
  it('hands the site language and appearance to the embedded Demo', () => {
    expect(demoHref('zh', 'dark')).toContain('language=zh');
    expect(demoHref('zh', 'dark')).toContain('theme=dark');
    expect(demoHref('en', 'light')).toContain('language=en');
    expect(demoHref('en', 'light')).toContain('theme=light');
  });
  it('keeps the Demo stage one surface step from the app window instead of inverting it', () => {
    const css = readFileSync('marketing/demo/demo.css', 'utf8');
    expect(css).toMatch(/\.demo-stage\s*\{[^}]*background:\s*var\(--gold-surface-low/);
  });
  it('opens the simulated client at the desktop client default window size', () => {
    const [mainWindow] = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).app.windows;
    const css = readFileSync('marketing/demo/demo.css', 'utf8');
    const value = (name: string) => Number(css.match(new RegExp(`${name}:\\s*([\\d.]+)`))?.[1]);
    expect(value('--demo-client-width')).toBe(mainWindow.width);
    expect(value('--demo-client-height')).toBe(mainWindow.height);
    const ratio = value('--demo-client-frame-ratio');
    const [marginPanel] = readFileSync('marketing/demo/DemoFrame.tsx', 'utf8').matchAll(/id="demo-(?:left|right)-margin" defaultSize="([\d.]+)%"/g);
    expect(ratio).toBeCloseTo(1 - 2 * Number(marginPanel[1]) / 100, 6);
    expect(css).toMatch(/\.demo-window\s*\{[^}]*calc\(\(var\(--demo-client-width\) \+ 2 \* var\(--demo-window-edge\)\) \/ var\(--demo-client-frame-ratio\)\)/);
  });
  it('keeps complete workflow interactions readable after real viewport changes and restores the overview', () => {
    const index = JSON.parse(readFileSync('marketing/site/media/checkpoints/index.json', 'utf8'));
    for (const language of ['zh', 'en']) for (const suffix of ['', '-light']) {
      const name = `${language}-during${suffix}`;
      for (const id of ['question', 'permission']) {
        const entry = index[name][id];
        expect(Math.min(280 / (entry.width * entry.mobile.width), 171 / (entry.height * entry.mobile.height)), `${name}/${id}`).toBeGreaterThanOrEqual(0.8);
      }
      for (const id of ['false', 'repair', 'true', 'deliver', 'complete']) {
        expect(index[name][id].width, `${name}/${id}`).toBe(index[name].opening.width);
        expect(index[name][id].height, `${name}/${id}`).toBe(index[name].opening.height);
      }
    }
  });
  it('provides a correctly sized static image and matching camera for every themed semantic step', () => {
    const index = JSON.parse(readFileSync('marketing/site/media/checkpoints/index.json', 'utf8'));
    for (const language of ['zh', 'en']) for (const chapter of CHAPTER_IDS) for (const theme of ['dark', 'light']) {
      const name = `${language}-${chapter}${theme === 'light' ? '-light' : ''}`;
      const { shots, duration } = readRecording(JSON.parse(readFileSync(`marketing/site/media/${name}.json`, 'utf8')));
      expect(Object.keys(index[name])).toEqual(shots.map(shot => shot.id));
      for (const [position, shot] of shots.entries()) {
        const entry = index[name][shot.id];
        for (const mode of ['desktop', 'mobile'] as const) for (const axis of ['x', 'y', 'width', 'height'] as const) expect(entry[mode][axis]).toBeCloseTo(shot[mode][axis], 12);
        expect(entry.at).toBeGreaterThanOrEqual(shot.at);
        expect(entry.at).toBeLessThan(shots[position + 1]?.at ?? duration);
        const png = readFileSync(`marketing/site/media/checkpoints/${name}-${shot.id}.png`);
        expect([...png.subarray(0, 8)]).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
        expect(png.readUInt32BE(16)).toBe(entry.width); expect(png.readUInt32BE(20)).toBe(entry.height);
      }
    }
  });
  it('addresses independent theme assets and pairs each language and scene by semantic checkpoint', () => {
    for (const language of ['zh', 'en'] as const) for (const chapter of CHAPTER_IDS) {
      const dark = readRecording(JSON.parse(readFileSync(`marketing/site/media/${language}-${chapter}.json`, 'utf8')));
      const light = readRecording(JSON.parse(readFileSync(`marketing/site/media/${language}-${chapter}-light.json`, 'utf8')));
      expect(light.shots.map(shot => shot.id)).toEqual(dark.shots.map(shot => shot.id));
      for (const shot of dark.shots) {
        const checkpoint = { id: shot.id, progress: 0.5 };
        const mapped = checkpointAt(light.shots, light.duration, restoreCheckpoint(light.shots, light.duration, checkpoint));
        expect(mapped.id).toBe(checkpoint.id);
        expect(mapped.progress).toBeCloseTo(checkpoint.progress);
      }
      for (const [theme, data] of [['dark', dark], ['light', light]] as const) {
        const snapshot = data.recording.events.find(event => event.type === 2)!;
        expect(JSON.stringify(snapshot.data)).toContain(`"data-color-scheme":"${theme}"`);
        const poster = readFileSync(`marketing/site/media/${language}-${chapter}${theme === 'light' ? '-light' : ''}.png`);
        expect([...poster.subarray(0, 8)]).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
      }
      for (const kind of ['json', 'png'] as const) {
        expect(mediaPath(language, chapter, kind, 'light')).toContain(`${language}-${chapter}-light.`);
        expect(mediaPath(language, chapter, kind, 'dark')).not.toBe(mediaPath(language, chapter, kind, 'light'));
      }
    }
  });
  it('accepts only defined chapter identities for anchor restoration', () => {
    for (const chapter of CHAPTER_IDS) expect(chapterFromHash(`#${chapter}`)).toBe(chapter);
    for (const hash of ['', '#unknown', '#during?node=one', '#during .other']) expect(chapterFromHash(hash)).toBeNull();
  });
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
  it('uses available width for desktop layout including touch laptops', () => {
    expect(DESKTOP_QUERY).toBe('(min-width: 1024px)');
  });
});
describe('curated product preview projections', () => {
  it('gives output schema and success expression readable semantic shots in both themes', () => {
    for (const language of ['zh', 'en']) for (const suffix of ['', '-light']) {
      const { shots } = readRecording(JSON.parse(readFileSync(`marketing/site/media/${language}-during${suffix}.json`, 'utf8')));
      for (const id of ['json-contract', 'success-expression']) {
        const shot = shots.find(shot => shot.id === id);
        expect(shot, `${language}${suffix}/${id}`).toBeDefined();
        expect(Math.min(280 / (1440 * shot!.mobile.width), 171 / (880 * shot!.mobile.height))).toBeGreaterThanOrEqual(0.8);
      }
    }
  });
  it('records the complete review flow with a real pointer hover and bounded desktop zoom', () => {
    for (const language of ['zh', 'en']) {
      const recording = JSON.parse(readFileSync(`marketing/site/media/${language}-after.json`, 'utf8'));
      const shots = recording.events.filter((event: { type: number; data: { tag?: string } }) => event.type === 5 && event.data.tag === 'site-shot')
        .map((event: { data: { payload: { id: string; desktop: { width: number; height: number } } } }) => event.data.payload);
      expect(shots.map((shot: { id: string }) => shot.id)).toEqual(['opening', 'report', 'report-result', 'hover-diff', 'workspace-file', 'file-saved', 'source-control', 'git-diff', 'staged', 'commit-subject', 'committed', 'closing']);
      expect(recording.events.some((event: { type: number; data: { source?: number } }) => event.type === 3 && event.data.source === 1)).toBe(true);
      for (const shot of shots) expect(Math.min(840 / (1440 * shot.desktop.width), 514 / (880 * shot.desktop.height)), `${language}/${shot.id}`).toBeLessThanOrEqual(2);
      for (const shot of shots.filter((shot: { id: string }) => ['report', 'workspace-file', 'file-saved', 'git-diff'].includes(shot.id))) {
        expect(Math.min(840 / (1440 * shot.desktop.width), 514 / (880 * shot.desktop.height)), `${language}/${shot.id}`).toBeGreaterThanOrEqual(1);
      }
      const mobileShots = recording.events.filter((event: { type: number; data: { tag?: string } }) => event.type === 5 && event.data.tag === 'site-shot').map((event: { data: { payload: { id: string; mobile: { width: number; height: number } } } }) => event.data.payload);
      for (const shot of mobileShots.filter((shot: { id: string }) => ['report-result', 'commit-subject', 'committed'].includes(shot.id))) {
        expect(Math.min(280 / (1440 * shot.mobile.width), 171 / (880 * shot.mobile.height)), `${language}/${shot.id}`).toBeGreaterThanOrEqual(0.8);
      }
    }
  });
  it('projects the same workspace into the recording run, session and source control route', () => {
    const base = structuredClone(mockErrorBlockedConversationRun);
    base.worktree = { path: '/old-worktree', branch: 'old-branch' };
    base.selectedSession!.worktreePath = '/old-worktree';
    for (const round of base.sessionTree.rounds) for (const node of round.nodes) for (const attempt of node.attempts) {
      attempt.worktreePath = '/old-worktree';
      attempt.worktreeBranch = 'old-branch';
    }
    const run = createPreviewRun(base, 'en');
    expect(conversationSourceControlWorkspacePath(conversationPageForRun(run), run)).toBeNull();
    expect(run.worktree).toBeNull();
    expect(run.selectedSession).toMatchObject({ worktreePath: null, worktreeBranch: null, cwd: '/default', providerCwd: '/default' });
  });
  it('records settings, visible conversation effects and actual reversible viewport changes', () => {
    for (const language of ['zh', 'en']) {
      const recording = JSON.parse(readFileSync(`marketing/site/media/${language}-personalize.json`, 'utf8'));
      const shots = recording.events.filter((event: { type: number; data: { tag?: string } }) => event.type === 5 && event.data.tag === 'site-shot')
        .map((event: { data: { payload: { id: string } } }) => event.data.payload.id);
      expect(shots).toEqual(['opening', 'theme-options', 'font-option', 'conversation-appearance', 'avatar-option', 'conversation-avatar',
        'layout-1', 'layout-2', 'layout-3', 'layout-4', 'layout-5', 'restore-theme', 'closing']);
      const resize = recording.events.filter((event: { type: number; data: { source?: number } }) => event.type === 3 && event.data.source === 4)
        .map((event: { data: { width: number; height: number } }) => [event.data.width, event.data.height]);
      expect(resize).toEqual([[900, 880], [600, 880], [900, 880], [1440, 880]]);
    }
  });
    it('keeps avatar choices and conversation context readable on the narrow mobile media surface in both themes', () => {
      for (const language of ['zh', 'en']) for (const suffix of ['', '-light']) {
        const { shots } = readRecording(JSON.parse(readFileSync(`marketing/site/media/${language}-personalize${suffix}.json`, 'utf8')));
        for (const id of ['avatar-option', 'conversation-avatar']) {
          const shot = shots.find(shot => shot.id === id)!;
          expect(shot).toBeDefined();
          expect(Math.min(280 / (1440 * shot.mobile.width), 171 / (880 * shot.mobile.height)), `${language}${suffix}/${id}`).toBeGreaterThanOrEqual(0.8);
        }
      }
    });
    it('preserves context instead of magnifying individual settings and avatars beyond twice their size', () => {
    for (const language of ['zh', 'en']) {
      const recording = JSON.parse(readFileSync(`marketing/site/media/${language}-personalize.json`, 'utf8'));
      for (const event of recording.events) {
        if (event.type !== 5 || event.data.tag !== 'site-shot') continue;
        const shot = event.data.payload;
        if (!['font-option', 'avatar-option', 'conversation-avatar'].includes(shot.id)) continue;
        expect(Math.min(840 / (1440 * shot.desktop.width), 514 / (880 * shot.desktop.height)), `${language}/${shot.id}`).toBeLessThanOrEqual(2);
      }
    }
  });
  it('provides the canonical latest run required to return from settings to the same task', () => {
    const run = createPreviewRun(mockErrorBlockedConversationRun, 'en');
    const task = previewTask(run, 'en');
    expect(task.latestRun).toMatchObject({ runId: run.runId, status: run.runStatus, outcome: run.runOutcome });
    expect(task).toMatchObject({ projectId: run.projectId, taskId: run.taskId, taskUuid: run.taskUuid });
    expect(task.runs).toEqual([task.latestRun]);
    expect(task.runHistoryStatus).toBe('ready');
  });
  it('keeps setup reading shots legible in the narrow mobile media surface', () => {
    const targets = new Set(['agent-claude', 'agent-codex', 'roles', 'skill-sync', 'workflow-options', 'workflow-selected', 'auto-options', 'auto-selected']);
    for (const language of ['zh', 'en']) for (const suffix of ['', '-light']) {
      const recording = JSON.parse(readFileSync(`marketing/site/media/${language}-before${suffix}.json`, 'utf8'));
      const shots = recording.events.filter((event: { type: number; data: { tag?: string } }) => event.type === 5 && event.data.tag === 'site-shot').map((event: { data: { payload: { id: string; mobile: { width: number; height: number } } } }) => event.data.payload);
      for (const id of targets) {
        const shot = shots.find((shot: { id: string }) => shot.id === id);
        expect(shot, id).toBeDefined();
        const scale = Math.min(280 / (1440 * shot.mobile.width), 171 / (880 * shot.mobile.height));
        expect(scale, `${language}/${id}`).toBeGreaterThanOrEqual(0.8);
      }
    }
  });
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
