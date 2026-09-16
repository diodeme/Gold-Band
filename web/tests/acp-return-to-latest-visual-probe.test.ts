/** @vitest-environment jsdom */

import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  captureAcpReturnToLatestVisualSnapshot,
  startAcpReturnToLatestVisualProbe,
} from '@/lib/acp-return-to-latest-visual-probe';

function rect(top: number, left: number, width: number, height: number): DOMRect {
  return {
    x: left,
    y: top,
    top,
    left,
    width,
    height,
    right: left + width,
    bottom: top + height,
    toJSON: () => ({}),
  } as DOMRect;
}

function setRect(element: Element, value: DOMRect) {
  vi.spyOn(element, 'getBoundingClientRect').mockReturnValue(value);
}

function createFixture() {
  const frame = document.createElement('div');
  frame.dataset.conversationViewportFrame = 'true';
  frame.style.setProperty('--conversation-viewport-footer-height', '96px');
  const viewport = document.createElement('div');
  viewport.dataset.conversationViewport = 'true';
  const content = document.createElement('div');
  content.style.paddingBottom = '96px';
  const timeline = document.createElement('div');
  timeline.dataset.acpConversationRail = 'timeline';
  timeline.textContent = 'private message content must not enter diagnostics';
  content.append(timeline);
  viewport.append(content);
  const footerLayer = document.createElement('div');
  footerLayer.dataset.conversationViewportFooter = 'true';
  const footer = document.createElement('div');
  footer.dataset.acpConversationFooter = 'viewport';
  const button = document.createElement('button');
  button.dataset.acpReturnToLatest = 'true';
  button.textContent = 'return to latest private label';
  const composer = document.createElement('div');
  composer.dataset.acpConversationRail = 'composer';
  footer.append(button, composer);
  footerLayer.append(footer);
  frame.append(viewport, footerLayer);
  document.body.append(frame);

  Object.defineProperties(viewport, {
    clientHeight: { configurable: true, value: 600 },
    scrollHeight: { configurable: true, value: 1_900 },
    scrollTop: { configurable: true, writable: true, value: 1_140 },
  });
  setRect(frame, rect(10, 20, 800, 700));
  setRect(viewport, rect(10, 20, 800, 700));
  setRect(content, rect(-1_130, 20, 800, 1_900));
  setRect(timeline, rect(-1_100, 40, 760, 1_750));
  setRect(footerLayer, rect(614, 20, 800, 96));
  setRect(footer, rect(614, 20, 800, 96));
  setRect(button, rect(570, 650, 140, 32));
  setRect(composer, rect(622, 40, 760, 80));
  vi.stubGlobal('ResizeObserver', undefined);
  vi.stubGlobal('elementsFromPoint', undefined);
  Object.defineProperty(document, 'elementsFromPoint', {
    configurable: true,
    value: () => [button, footer, footerLayer, frame],
  });

  return { button, composer, content, footer, footerLayer, frame, timeline, viewport };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('ACP return-to-latest visual probe', () => {
  it('captures layout, compositing styles, hit testing, and scroll geometry without text', () => {
    const fixture = createFixture();
    fixture.button.disabled = true;

    const snapshot = captureAcpReturnToLatestVisualSnapshot({
      button: fixture.button,
      content: fixture.content,
      viewport: fixture.viewport,
    });

    expect(snapshot.button?.rect).toMatchObject({ top: 570, left: 650, width: 140, height: 32 });
    expect(snapshot.viewport).toMatchObject({
      scrollTop: 1_140,
      scrollHeight: 1_900,
      clientHeight: 600,
      distanceFromBottom: 160,
    });
    expect(snapshot.footerLayer?.rect).toMatchObject({ top: 614, height: 96 });
    expect(snapshot.composer?.rect).toMatchObject({ top: 622, height: 80 });
    expect(snapshot.content).toMatchObject({ paddingBottom: '96px' });
    expect(snapshot.hitTest).toMatchObject({ buttonOwnsTopElement: true });
    expect(snapshot.hitTest?.stack).toHaveLength(4);
    expect(snapshot.button?.style).toHaveProperty('transform');
    expect(snapshot.button?.element.attributes).toMatchObject({ disabled: '' });
    expect(JSON.stringify(snapshot)).not.toContain('private message');
    expect(JSON.stringify(snapshot)).not.toContain('private label');
  });

  it('correlates visual frames with React commits under one bounded probe identity', () => {
    const fixture = createFixture();
    const frames: FrameRequestCallback[] = [];
    let now = 100;
    const records: Array<{ event: string; details: Record<string, unknown> }> = [];
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    vi.stubGlobal('cancelAnimationFrame', vi.fn());

    const probe = startAcpReturnToLatestVisualProbe({
      button: fixture.button,
      content: fixture.content,
      viewport: fixture.viewport,
      getDiagnosticDetails: () => ({ sessionIdentity: 'session-a' }),
      now: () => now,
      record: (event, details) => records.push({ event, details }),
    });

    expect(records[0]).toMatchObject({
      event: 'visual-probe-start',
      details: { sessionIdentity: 'session-a' },
    });
    fixture.viewport.dispatchEvent(new KeyboardEvent('keydown', { key: 'x' }));
    fixture.viewport.dispatchEvent(new Event('scrollend'));
    fixture.viewport.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp' }));
    expect(records.filter((record) => record.event === 'visual-input')).toEqual([
      expect.objectContaining({
        details: expect.objectContaining({ inputType: 'keydown', key: 'ArrowUp' }),
      }),
    ]);
    probe.recordReactCommit();
    setRect(fixture.button, rect(568, 650, 140, 32));
    now = 140;
    frames.shift()?.(now);
    probe.stop('test-stop');
    const recordCountAfterStop = records.length;
    now = 180;
    frames.shift()?.(now);
    expect(records).toHaveLength(recordCountAfterStop);

    const probeIds = new Set(records.map((record) => record.details.probeId));
    expect(probeIds.size).toBe(1);
    expect(records.map((record) => record.event)).toEqual(expect.arrayContaining([
      'visual-probe-start',
      'react-commit',
      'visual-frame',
      'visual-probe-stop',
    ]));
    expect(records.find((record) => record.event === 'visual-frame')?.details)
      .toHaveProperty('changedFields');
    expect(records.at(-1)).toMatchObject({
      event: 'visual-probe-stop',
      details: { reason: 'test-stop' },
    });
  });
});
