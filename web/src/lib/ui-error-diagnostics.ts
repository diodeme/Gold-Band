import { reportFrontendError } from '@/api';
import type { FrontendErrorKind, FrontendErrorReportInput } from '@/api/client';

const MAXIMUM_UPDATE_DEPTH_MESSAGE = 'Maximum update depth exceeded';
const DUPLICATE_WINDOW_MS = 5_000;
const THROTTLE_WINDOW_MS = 10_000;
const MAX_REPORTS_PER_WINDOW = 5;

const FIELD_LIMITS = {
  message: 4_096,
  stack: 16_384,
  componentStack: 16_384,
  source: 2_048,
  activeElement: 1_024,
  lastPointerTarget: 1_024,
  lastPointerAt: 64,
  pathname: 2_048,
  userAgent: 2_048,
} as const;

type FrontendErrorSink = (input: FrontendErrorReportInput) => Promise<void> | void;

export interface UiErrorSource {
  componentStack?: string | null;
  source?: string | null;
  line?: number | null;
  column?: number | null;
}

export interface UiErrorDiagnostics {
  report(kind: FrontendErrorKind, error: unknown, source?: UiErrorSource): void;
  dispose(): void;
}

interface DiagnosticContext {
  activeElement: string | null;
  lastPointerTarget: string | null;
  lastPointerAt: string | null;
  pathname: string | null;
  userAgent: string | null;
}

export function installUiErrorDiagnostics(
  sink: FrontendErrorSink = reportFrontendError,
): UiErrorDiagnostics | null {
  if (typeof window === 'undefined' || typeof document === 'undefined') return null;

  let lastPointerTarget: string | null = null;
  let lastPointerAt: string | null = null;
  const report = createUiErrorReporter(sink, () => ({
    activeElement: describeEventTarget(document.activeElement),
    lastPointerTarget,
    lastPointerAt,
    pathname: window.location.pathname || null,
    userAgent: window.navigator.userAgent || null,
  }));

  const onPointerDown = (event: PointerEvent) => {
    lastPointerTarget = describeEventTarget(event.target);
    lastPointerAt = new Date().toISOString();
  };
  const onError = (event: ErrorEvent) => {
    report('window-error', event.error ?? event.message, {
      source: event.filename || null,
      line: event.lineno ?? null,
      column: event.colno ?? null,
    });
  };
  const onUnhandledRejection = (event: PromiseRejectionEvent) => {
    report('unhandled-rejection', event.reason);
  };

  window.addEventListener('pointerdown', onPointerDown, true);
  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onUnhandledRejection);

  return {
    report,
    dispose() {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('error', onError);
      window.removeEventListener('unhandledrejection', onUnhandledRejection);
    },
  };
}

export function createUiErrorReporter(
  sink: FrontendErrorSink,
  getContext: () => DiagnosticContext = emptyDiagnosticContext,
  now: () => number = Date.now,
) {
  const lastReportedByFingerprint = new Map<string, number>();
  const recentReports: number[] = [];

  return (kind: FrontendErrorKind, error: unknown, source: UiErrorSource = {}) => {
    const message = extractErrorMessage(error);
    const stack = extractErrorStack(error);
    const fingerprint = `${truncateText(message, FIELD_LIMITS.message) ?? ''}\n${truncateText(stack, FIELD_LIMITS.stack) ?? ''}`;
    const timestamp = now();
    const duplicateAt = lastReportedByFingerprint.get(fingerprint);
    if (duplicateAt !== undefined && timestamp - duplicateAt < DUPLICATE_WINDOW_MS) return;

    while (recentReports.length > 0 && timestamp - recentReports[0] >= THROTTLE_WINDOW_MS) {
      recentReports.shift();
    }
    if (recentReports.length >= MAX_REPORTS_PER_WINDOW) return;

    for (const [seenFingerprint, seenAt] of lastReportedByFingerprint) {
      if (timestamp - seenAt >= DUPLICATE_WINDOW_MS) lastReportedByFingerprint.delete(seenFingerprint);
    }
    lastReportedByFingerprint.set(fingerprint, timestamp);
    recentReports.push(timestamp);

    let context: DiagnosticContext;
    try {
      context = getContext();
    } catch {
      context = emptyDiagnosticContext();
    }
    const input = buildFrontendErrorReport(kind, message, stack, source, context);
    try {
      Promise.resolve(sink(input)).catch(() => {});
    } catch {
      // Diagnostics are best-effort and must never alter the failing UI path.
    }

    if (shouldLogUiError(error)) {
      try {
        console.error(formatUiErrorDiagnostic(input));
      } catch {
        // Console forwarding is also diagnostic-only.
      }
    }
  };
}

export function buildFrontendErrorReport(
  kind: FrontendErrorKind,
  message: string,
  stack: string | null,
  source: UiErrorSource = {},
  context: DiagnosticContext = emptyDiagnosticContext(),
): FrontendErrorReportInput {
  return {
    kind,
    message: truncateText(message, FIELD_LIMITS.message) ?? '',
    stack: truncateText(stack, FIELD_LIMITS.stack),
    componentStack: truncateText(source.componentStack, FIELD_LIMITS.componentStack),
    source: truncateText(stripUrlQuery(source.source), FIELD_LIMITS.source),
    line: finiteNumberOrNull(source.line),
    column: finiteNumberOrNull(source.column),
    activeElement: truncateText(context.activeElement, FIELD_LIMITS.activeElement),
    lastPointerTarget: truncateText(context.lastPointerTarget, FIELD_LIMITS.lastPointerTarget),
    lastPointerAt: truncateText(context.lastPointerAt, FIELD_LIMITS.lastPointerAt),
    pathname: truncateText(context.pathname, FIELD_LIMITS.pathname),
    userAgent: truncateText(context.userAgent, FIELD_LIMITS.userAgent),
  };
}

export function shouldLogUiError(error: unknown) {
  return `${extractErrorMessage(error)}\n${extractErrorStack(error) ?? ''}`.includes(MAXIMUM_UPDATE_DEPTH_MESSAGE);
}

export function extractErrorMessage(error: unknown): string {
  try {
    if (error instanceof Error) return error.message;
    if (typeof error === 'string') return error;
    if (error && typeof error === 'object' && 'message' in error) {
      const message = (error as { message?: unknown }).message;
      if (typeof message === 'string') return message;
    }
    return String(error);
  } catch {
    return 'Unknown frontend error';
  }
}

export function extractErrorStack(error: unknown): string | null {
  try {
    if (error instanceof Error) return error.stack ?? null;
    if (error && typeof error === 'object' && 'stack' in error) {
      const stack = (error as { stack?: unknown }).stack;
      if (typeof stack === 'string') return stack;
    }
  } catch {
    return null;
  }
  return null;
}

export function formatUiErrorDiagnostic(diagnostic: FrontendErrorReportInput) {
  const lines = Object.entries(diagnostic).map(([key, value]) => (
    `${key}=${value === null || value === undefined ? 'null' : String(value)}`
  ));
  return `[gb-ui-error] frontend fatal diagnostic\n${lines.join('\n')}`;
}

function emptyDiagnosticContext(): DiagnosticContext {
  return {
    activeElement: null,
    lastPointerTarget: null,
    lastPointerAt: null,
    pathname: null,
    userAgent: null,
  };
}

function truncateText(value: string | null | undefined, maxCharacters: number): string | null {
  if (value === null || value === undefined) return null;
  const characters = Array.from(value);
  return characters.length <= maxCharacters ? value : characters.slice(0, maxCharacters).join('');
}

function stripUrlQuery(value: string | null | undefined): string | null {
  if (value === null || value === undefined) return null;
  return value.split(/[?#]/, 1)[0];
}

function finiteNumberOrNull(value: number | null | undefined) {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function describeEventTarget(target: EventTarget | null): string | null {
  if (typeof Element === 'undefined' || !(target instanceof Element)) return null;
  const parts: string[] = [];
  let current: Element | null = target;
  for (let depth = 0; current && depth < 5; depth += 1) {
    parts.push(describeElement(current));
    current = current.parentElement;
  }
  return parts.join(' < ');
}

function describeElement(element: Element) {
  const tag = element.tagName.toLowerCase();
  const id = element.id ? `#${element.id}` : '';
  const role = element.getAttribute('role');
  const dataSlot = element.getAttribute('data-slot');
  return [
    `${tag}${id}`,
    dataSlot ? `[data-slot="${dataSlot}"]` : null,
    role ? `[role="${role}"]` : null,
  ].filter(Boolean).join('');
}
