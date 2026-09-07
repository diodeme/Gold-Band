import { recordAcpStreamingDiagnostic } from '@/lib/acp-streaming-diagnostics';

const VISUAL_PROBE_SAMPLE_INTERVAL_MS = 32;
const VISUAL_PROBE_HEARTBEAT_MS = 500;
const VISUAL_PROBE_INITIAL_WINDOW_MS = 10_000;
const VISUAL_PROBE_ACTIVITY_WINDOW_MS = 4_000;
const VISUAL_PROBE_RECORD_LIMIT = 720;
const VISUAL_PROBE_HIT_TEST_LIMIT = 8;
const VISUAL_PROBE_ANCESTOR_LIMIT = 8;

const VISUAL_PROBE_SCROLL_KEYS = new Set([
  ' ',
  'ArrowDown',
  'ArrowUp',
  'End',
  'Home',
  'PageDown',
  'PageUp',
]);

const VISUAL_PROBE_ATTRIBUTE_FILTER = [
  'aria-hidden',
  'class',
  'data-state',
  'disabled',
  'hidden',
  'style',
];

const VISUAL_PROBE_LAYER_STYLE_PROPERTIES = [
  'animation-name',
  'backdrop-filter',
  'clip-path',
  'contain',
  'content-visibility',
  'display',
  'filter',
  'isolation',
  'mix-blend-mode',
  'opacity',
  'overflow',
  'pointer-events',
  'position',
  'scale',
  'transform',
  'transition-property',
  'translate',
  'visibility',
  'will-change',
  'z-index',
] as const;

const VISUAL_PROBE_ELEMENT_ATTRIBUTES = [
  'aria-hidden',
  'data-acp-conversation-footer',
  'data-acp-conversation-rail',
  'data-acp-return-to-latest',
  'data-conversation-viewport',
  'data-conversation-viewport-footer',
  'data-conversation-viewport-frame',
  'data-slot',
  'data-state',
  'disabled',
  'hidden',
  'role',
] as const;

type VisualProbeRecord = (
  event: string,
  details: Record<string, unknown>,
) => void;

type VisualProbeRect = {
  x: number;
  y: number;
  top: number;
  right: number;
  bottom: number;
  left: number;
  width: number;
  height: number;
};

type VisualProbeElementSnapshot = {
  element: ReturnType<typeof describeVisualProbeElement>;
  connected: boolean;
  clientRectCount: number;
  rect: VisualProbeRect;
  style: Record<string, string>;
};

export type AcpReturnToLatestVisualSnapshot = {
  button: VisualProbeElementSnapshot;
  footerLayer: VisualProbeElementSnapshot | null;
  footerSurface: VisualProbeElementSnapshot | null;
  composer: VisualProbeElementSnapshot | null;
  frame: VisualProbeElementSnapshot | null;
  timeline: VisualProbeElementSnapshot | null;
  viewport: (VisualProbeElementSnapshot & {
    scrollTop: number;
    scrollHeight: number;
    clientHeight: number;
    scrollWidth: number;
    clientWidth: number;
    distanceFromBottom: number;
  }) | null;
  content: (VisualProbeElementSnapshot & {
    paddingBottom: string;
  }) | null;
  hitTest: {
    centerX: number;
    centerY: number;
    buttonOwnsTopElement: boolean;
    stack: Array<ReturnType<typeof describeVisualProbeElement>>;
  } | null;
  ancestorLayers: VisualProbeElementSnapshot[];
  animations: Array<Record<string, unknown>>;
  devicePixelRatio: number | null;
  visualViewport: {
    width: number;
    height: number;
    offsetLeft: number;
    offsetTop: number;
    scale: number;
  } | null;
};

export type AcpReturnToLatestVisualProbe = {
  recordReactCommit: () => void;
  stop: (reason?: string) => void;
};

export type AcpReturnToLatestVisualProbeOptions = {
  button: HTMLButtonElement;
  viewport: HTMLElement | null;
  content: HTMLElement | null;
  getDiagnosticDetails: () => Record<string, unknown>;
  now?: () => number;
  record?: VisualProbeRecord;
};

let nextVisualProbeId = 0;
let nextVisualProbeElementId = 0;
const visualProbeElementIds = new WeakMap<Element, number>();

export function captureAcpReturnToLatestVisualSnapshot({
  button,
  viewport,
  content,
}: Pick<AcpReturnToLatestVisualProbeOptions, 'button' | 'viewport' | 'content'>): AcpReturnToLatestVisualSnapshot {
  return captureVisualSnapshot({ button, viewport, content }, true);
}

function captureVisualSnapshot(
  {
    button,
    viewport,
    content,
  }: Pick<AcpReturnToLatestVisualProbeOptions, 'button' | 'viewport' | 'content'>,
  includeComputedStyles: boolean,
): AcpReturnToLatestVisualSnapshot {
  const footerSurface = button.closest<HTMLElement>(
    '[data-acp-conversation-footer="viewport"]',
  );
  const footerLayer = button.closest<HTMLElement>(
    '[data-conversation-viewport-footer="true"]',
  );
  const frame = button.closest<HTMLElement>(
    '[data-conversation-viewport-frame="true"]',
  );
  const composer = footerSurface?.querySelector<HTMLElement>(
    '[data-acp-conversation-rail="composer"]',
  ) ?? null;
  const timeline = frame?.querySelector<HTMLElement>(
    '[data-acp-conversation-rail="timeline"]',
  ) ?? null;
  const buttonSnapshot = captureVisualProbeElement(button, includeComputedStyles);

  return {
    button: buttonSnapshot,
    footerLayer: captureOptionalVisualProbeElement(footerLayer, includeComputedStyles),
    footerSurface: captureOptionalVisualProbeElement(footerSurface, includeComputedStyles),
    composer: captureOptionalVisualProbeElement(composer, includeComputedStyles),
    frame: captureOptionalVisualProbeElement(frame, includeComputedStyles),
    timeline: captureOptionalVisualProbeElement(timeline, includeComputedStyles),
    viewport: viewport
      ? {
          ...captureVisualProbeElement(viewport, includeComputedStyles),
          scrollTop: roundVisualProbeNumber(viewport.scrollTop),
          scrollHeight: roundVisualProbeNumber(viewport.scrollHeight),
          clientHeight: roundVisualProbeNumber(viewport.clientHeight),
          scrollWidth: roundVisualProbeNumber(viewport.scrollWidth),
          clientWidth: roundVisualProbeNumber(viewport.clientWidth),
          distanceFromBottom: roundVisualProbeNumber(
            viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight,
          ),
        }
      : null,
    content: content
      ? {
          ...captureVisualProbeElement(content, includeComputedStyles),
          paddingBottom: readComputedStyle(content, 'padding-bottom'),
        }
      : null,
    hitTest: captureVisualProbeHitTest(button, buttonSnapshot.rect),
    ancestorLayers: includeComputedStyles
      ? captureVisualProbeAncestorLayers(button, frame)
      : [],
    animations: captureVisualProbeAnimations(footerLayer ?? footerSurface ?? button),
    devicePixelRatio:
      typeof window === 'undefined'
        ? null
        : roundVisualProbeNumber(window.devicePixelRatio),
    visualViewport: captureVisualViewport(),
  };
}

export function startAcpReturnToLatestVisualProbe({
  button,
  viewport,
  content,
  getDiagnosticDetails,
  now = () => performance.now(),
  record = (event, details) => {
    recordAcpStreamingDiagnostic('return-to-latest-trace', () => ({
      event,
      ...details,
    }));
  },
}: AcpReturnToLatestVisualProbeOptions): AcpReturnToLatestVisualProbe {
  nextVisualProbeId += 1;
  const probeId = `return-to-latest-visual-${nextVisualProbeId}`;
  let stopped = false;
  let frameId: number | null = null;
  let recordCount = 0;
  let sampleCount = 0;
  let reactCommitCount = 0;
  let lastSampleAt = Number.NEGATIVE_INFINITY;
  let lastRecordedAt = Number.NEGATIVE_INFINITY;
  let activeUntil = now() + VISUAL_PROBE_INITIAL_WINDOW_MS;
  let lastSnapshot = captureAcpReturnToLatestVisualSnapshot({
    button,
    viewport,
    content,
  });
  let lastFrameSnapshot = captureVisualSnapshot({ button, viewport, content }, false);
  let recordLimitReported = false;

  const diagnosticDetails = () => {
    try {
      return getDiagnosticDetails();
    } catch (error) {
      return { diagnosticDetailsError: String(error) };
    }
  };

  const emit = (event: string, details: Record<string, unknown> = {}) => {
    if (recordCount >= VISUAL_PROBE_RECORD_LIMIT) return;
    if (
      recordCount === VISUAL_PROBE_RECORD_LIMIT - 1
      && event !== 'visual-probe-stop'
    ) {
      if (recordLimitReported) return;
      recordLimitReported = true;
      recordCount += 1;
      record('visual-probe-record-limit', {
        ...diagnosticDetails(),
        probeId,
        recordSequence: recordCount,
        recordLimit: VISUAL_PROBE_RECORD_LIMIT,
      });
      return;
    }
    recordCount += 1;
    record(event, {
      ...diagnosticDetails(),
      probeId,
      recordSequence: recordCount,
      ...details,
    });
  };

  const scheduleFrame = () => {
    if (stopped || frameId !== null || !button.isConnected) return;
    frameId = requestAnimationFrame(handleFrame);
  };

  const activate = () => {
    if (stopped) return;
    activeUntil = Math.max(activeUntil, now() + VISUAL_PROBE_ACTIVITY_WINDOW_MS);
    scheduleFrame();
  };

  const captureSnapshot = (includeComputedStyles = true) => captureVisualSnapshot(
    { button, viewport, content },
    includeComputedStyles,
  );

  function handleFrame() {
    frameId = null;
    if (stopped || !button.isConnected) return;
    const sampledAt = now();
    if (
      sampledAt <= activeUntil
      && sampledAt - lastSampleAt >= VISUAL_PROBE_SAMPLE_INTERVAL_MS
    ) {
      const snapshot = captureSnapshot(false);
      const changedFields = changedVisualSnapshotFields(lastFrameSnapshot, snapshot);
      sampleCount += 1;
      lastSampleAt = sampledAt;
      if (
        changedFields.length > 0
        || sampledAt - lastRecordedAt >= VISUAL_PROBE_HEARTBEAT_MS
      ) {
        emit('visual-frame', {
          sampledAtMs: roundVisualProbeNumber(sampledAt),
          sampleSequence: sampleCount,
          changedFields,
          snapshot,
        });
        lastRecordedAt = sampledAt;
      }
      lastFrameSnapshot = snapshot;
      lastSnapshot = snapshot;
    }
    if (sampledAt <= activeUntil) scheduleFrame();
  }

  const handleActivity = (event: Event) => {
    const keyboard = event instanceof KeyboardEvent ? event : null;
    if (keyboard && !VISUAL_PROBE_SCROLL_KEYS.has(keyboard.key)) return;
    activate();
    if (
      event.type === 'wheel'
      || event.type === 'pointerdown'
      || event.type === 'keydown'
    ) {
      const wheel = event instanceof WheelEvent ? event : null;
      emit('visual-input', {
        inputType: event.type,
        wheelDeltaX: wheel ? roundVisualProbeNumber(wheel.deltaX) : null,
        wheelDeltaY: wheel ? roundVisualProbeNumber(wheel.deltaY) : null,
        wheelDeltaMode: wheel?.deltaMode ?? null,
        key: keyboard?.key ?? null,
      });
    }
  };

  const activityEvents = ['keydown', 'pointerdown', 'scroll', 'scrollend', 'wheel'] as const;
  for (const eventName of activityEvents) {
    viewport?.addEventListener(eventName, handleActivity, { passive: true });
  }

  const observedElements = visualProbeObservedElements(button, viewport, content);
  const resizeObserver = typeof ResizeObserver === 'undefined'
    ? null
    : new ResizeObserver((entries) => {
        activate();
        emit('visual-resize', {
          entries: entries.slice(0, 12).map((entry) => ({
            target: describeVisualProbeElement(entry.target),
            width: roundVisualProbeNumber(entry.contentRect.width),
            height: roundVisualProbeNumber(entry.contentRect.height),
          })),
          snapshot: captureSnapshot(),
        });
      });
  for (const element of observedElements) resizeObserver?.observe(element);

  const mutationRoot = button.closest<HTMLElement>(
    '[data-conversation-viewport-footer="true"]',
  ) ?? button.parentElement;
  const mutationObserver = typeof MutationObserver === 'undefined' || !mutationRoot
    ? null
    : new MutationObserver((mutations) => {
        activate();
        emit('visual-mutation', {
          mutations: mutations.slice(0, 12).map((mutation) => ({
            type: mutation.type,
            target: describeVisualProbeElement(mutation.target as Element),
            attributeName: mutation.attributeName,
            oldValue: truncateVisualProbeValue(mutation.oldValue),
            newValue: mutation.attributeName
              ? truncateVisualProbeValue(
                  (mutation.target as Element).getAttribute(mutation.attributeName),
                )
              : null,
            added: Array.from(mutation.addedNodes)
              .filter((node): node is Element => node instanceof Element)
              .slice(0, 6)
              .map(describeVisualProbeElement),
            removed: Array.from(mutation.removedNodes)
              .filter((node): node is Element => node instanceof Element)
              .slice(0, 6)
              .map(describeVisualProbeElement),
          })),
          snapshot: captureSnapshot(),
        });
      });
  if (mutationObserver && mutationRoot) {
    mutationObserver.observe(mutationRoot, {
      attributeFilter: VISUAL_PROBE_ATTRIBUTE_FILTER,
      attributeOldValue: true,
      attributes: true,
      childList: true,
      subtree: true,
    });
  }

  const animationRoot = mutationRoot ?? button;
  const visualLifecycleEvents = [
    'animationcancel',
    'animationend',
    'animationiteration',
    'animationstart',
    'transitioncancel',
    'transitionend',
    'transitionrun',
    'transitionstart',
  ] as const;
  const handleVisualLifecycleEvent = (event: Event) => {
    activate();
    const animationEvent = event as AnimationEvent;
    const transitionEvent = event as TransitionEvent;
    emit('visual-lifecycle-event', {
      lifecycleEvent: event.type,
      target: event.target instanceof Element
        ? describeVisualProbeElement(event.target)
        : null,
      animationName: animationEvent.animationName || null,
      propertyName: transitionEvent.propertyName || null,
      elapsedTimeMs: roundVisualProbeNumber(
        Number(animationEvent.elapsedTime ?? transitionEvent.elapsedTime ?? 0) * 1_000,
      ),
      snapshot: captureSnapshot(),
    });
  };
  for (const eventName of visualLifecycleEvents) {
    animationRoot.addEventListener(eventName, handleVisualLifecycleEvent, true);
  }

  emit('visual-probe-start', {
    sampleIntervalMs: VISUAL_PROBE_SAMPLE_INTERVAL_MS,
    initialWindowMs: VISUAL_PROBE_INITIAL_WINDOW_MS,
    activityWindowMs: VISUAL_PROBE_ACTIVITY_WINDOW_MS,
    recordLimit: VISUAL_PROBE_RECORD_LIMIT,
    snapshot: lastSnapshot,
  });
  scheduleFrame();

  return {
    recordReactCommit() {
      if (stopped) return;
      reactCommitCount += 1;
      activate();
      emit('react-commit', {
        reactCommitCount,
        button: describeVisualProbeElement(button),
        buttonConnected: button.isConnected,
      });
    },
    stop(reason = 'button-detach') {
      if (stopped) return;
      stopped = true;
      if (frameId !== null) cancelAnimationFrame(frameId);
      frameId = null;
      resizeObserver?.disconnect();
      mutationObserver?.disconnect();
      for (const eventName of activityEvents) {
        viewport?.removeEventListener(eventName, handleActivity);
      }
      for (const eventName of visualLifecycleEvents) {
        animationRoot.removeEventListener(eventName, handleVisualLifecycleEvent, true);
      }
      emit('visual-probe-stop', {
        reason,
        sampleCount,
        reactCommitCount,
        finalSnapshot: lastSnapshot,
      });
    },
  };
}

function visualProbeObservedElements(
  button: HTMLButtonElement,
  viewport: HTMLElement | null,
  content: HTMLElement | null,
) {
  const candidates = [
    button,
    viewport,
    content,
    button.closest<HTMLElement>('[data-conversation-viewport-footer="true"]'),
    button.closest<HTMLElement>('[data-acp-conversation-footer="viewport"]'),
    button.closest<HTMLElement>('[data-conversation-viewport-frame="true"]'),
    button.closest<HTMLElement>('[data-acp-conversation-footer="viewport"]')
      ?.querySelector<HTMLElement>('[data-acp-conversation-rail="composer"]'),
  ];
  return Array.from(new Set(candidates.filter((element): element is HTMLElement => Boolean(element))));
}

function captureOptionalVisualProbeElement(
  element: HTMLElement | null,
  includeComputedStyles: boolean,
) {
  return element ? captureVisualProbeElement(element, includeComputedStyles) : null;
}

function captureVisualProbeElement(
  element: HTMLElement,
  includeComputedStyles = true,
): VisualProbeElementSnapshot {
  return {
    element: describeVisualProbeElement(element),
    connected: element.isConnected,
    clientRectCount: element.getClientRects().length,
    rect: captureVisualProbeRect(element.getBoundingClientRect()),
    style: includeComputedStyles ? captureVisualProbeLayerStyle(element) : {},
  };
}

function captureVisualProbeRect(rect: DOMRect): VisualProbeRect {
  return {
    x: roundVisualProbeNumber(rect.x),
    y: roundVisualProbeNumber(rect.y),
    top: roundVisualProbeNumber(rect.top),
    right: roundVisualProbeNumber(rect.right),
    bottom: roundVisualProbeNumber(rect.bottom),
    left: roundVisualProbeNumber(rect.left),
    width: roundVisualProbeNumber(rect.width),
    height: roundVisualProbeNumber(rect.height),
  };
}

function captureVisualProbeLayerStyle(element: HTMLElement) {
  const style = getComputedStyle(element);
  return Object.fromEntries(
    VISUAL_PROBE_LAYER_STYLE_PROPERTIES.map((property) => [
      property,
      style.getPropertyValue(property),
    ]),
  );
}

function captureVisualProbeHitTest(
  button: HTMLButtonElement,
  rect: VisualProbeRect,
) {
  if (typeof document.elementsFromPoint !== 'function') return null;
  const centerX = rect.left + rect.width / 2;
  const centerY = rect.top + rect.height / 2;
  let stack: Element[];
  try {
    stack = document.elementsFromPoint(centerX, centerY);
  } catch {
    return null;
  }
  const top = stack[0] ?? null;
  return {
    centerX: roundVisualProbeNumber(centerX),
    centerY: roundVisualProbeNumber(centerY),
    buttonOwnsTopElement: Boolean(top && (top === button || button.contains(top))),
    stack: stack.slice(0, VISUAL_PROBE_HIT_TEST_LIMIT).map(describeVisualProbeElement),
  };
}

function captureVisualProbeAncestorLayers(
  button: HTMLButtonElement,
  frame: HTMLElement | null,
) {
  const layers: VisualProbeElementSnapshot[] = [];
  let current = button.parentElement;
  while (current && layers.length < VISUAL_PROBE_ANCESTOR_LIMIT) {
    layers.push(captureVisualProbeElement(current));
    if (current === frame) break;
    current = current.parentElement;
  }
  return layers;
}

function captureVisualProbeAnimations(root: HTMLElement) {
  if (typeof root.getAnimations !== 'function') return [];
  return root.getAnimations({ subtree: true }).slice(0, 12).map((animation) => {
    const namedAnimation = animation as Animation & {
      animationName?: string;
      transitionProperty?: string;
    };
    return {
      type: animation.constructor?.name ?? 'Animation',
      id: animation.id || null,
      playState: animation.playState,
      pending: animation.pending,
      currentTimeMs: typeof animation.currentTime === 'number'
        ? roundVisualProbeNumber(animation.currentTime)
        : null,
      startTimeMs: typeof animation.startTime === 'number'
        ? roundVisualProbeNumber(animation.startTime)
        : null,
      animationName: namedAnimation.animationName ?? null,
      transitionProperty: namedAnimation.transitionProperty ?? null,
    };
  });
}

function captureVisualViewport() {
  if (typeof window === 'undefined' || !window.visualViewport) return null;
  return {
    width: roundVisualProbeNumber(window.visualViewport.width),
    height: roundVisualProbeNumber(window.visualViewport.height),
    offsetLeft: roundVisualProbeNumber(window.visualViewport.offsetLeft),
    offsetTop: roundVisualProbeNumber(window.visualViewport.offsetTop),
    scale: roundVisualProbeNumber(window.visualViewport.scale),
  };
}

function describeVisualProbeElement(element: Element) {
  let elementId = visualProbeElementIds.get(element);
  if (elementId === undefined) {
    nextVisualProbeElementId += 1;
    elementId = nextVisualProbeElementId;
    visualProbeElementIds.set(element, elementId);
  }
  const attributes = Object.fromEntries(
    VISUAL_PROBE_ELEMENT_ATTRIBUTES.flatMap((attribute) => {
      const value = element.getAttribute(attribute);
      return value === null ? [] : [[attribute, truncateVisualProbeValue(value)]];
    }),
  );
  return {
    elementId,
    tag: element.tagName.toLowerCase(),
    id: truncateVisualProbeValue(element.id || null),
    className: truncateVisualProbeValue(element.getAttribute('class')),
    attributes,
  };
}

function changedVisualSnapshotFields(
  previous: AcpReturnToLatestVisualSnapshot,
  next: AcpReturnToLatestVisualSnapshot,
) {
  return (Object.keys(next) as Array<keyof AcpReturnToLatestVisualSnapshot>)
    .filter((field) => JSON.stringify(previous[field]) !== JSON.stringify(next[field]));
}

function readComputedStyle(element: HTMLElement, property: string) {
  return getComputedStyle(element).getPropertyValue(property);
}

function roundVisualProbeNumber(value: number) {
  return Number.isFinite(value) ? Math.round(value * 100) / 100 : 0;
}

function truncateVisualProbeValue(value: string | null, limit = 600) {
  if (value === null) return null;
  return value.length <= limit ? value : `${value.slice(0, limit)}...`;
}
