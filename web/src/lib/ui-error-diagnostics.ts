const MAXIMUM_UPDATE_DEPTH_MESSAGE = 'Maximum update depth exceeded';
const UI_ERROR_LOG_THROTTLE_MS = 1000;

let installed = false;
let lastPointerTarget: string | null = null;
let lastPointerAt: string | null = null;
let lastLoggedAt = 0;

export function installUiErrorDiagnostics() {
  if (installed || typeof window === 'undefined' || typeof document === 'undefined') return;
  installed = true;

  window.addEventListener('pointerdown', (event) => {
    lastPointerTarget = describeEventTarget(event.target);
    lastPointerAt = new Date().toISOString();
  }, true);

  window.addEventListener('error', (event) => {
    logUiErrorDiagnostic(event.error ?? event.message, {
      source: event.filename || null,
      line: event.lineno || null,
      column: event.colno || null,
    });
  });

  window.addEventListener('unhandledrejection', (event) => {
    logUiErrorDiagnostic(event.reason);
  });
}

export function shouldLogUiError(error: unknown) {
  const message = extractErrorMessage(error);
  const stack = extractErrorStack(error);
  return `${message}\n${stack}`.includes(MAXIMUM_UPDATE_DEPTH_MESSAGE);
}

export function extractErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string') return message;
  }
  return String(error);
}

export function extractErrorStack(error: unknown): string | null {
  if (error instanceof Error) return error.stack ?? null;
  if (error && typeof error === 'object' && 'stack' in error) {
    const stack = (error as { stack?: unknown }).stack;
    if (typeof stack === 'string') return stack;
  }
  return null;
}

export function logUiErrorDiagnostic(error: unknown, source?: Record<string, unknown>) {
  if (!shouldLogUiError(error)) return;
  const now = Date.now();
  if (now - lastLoggedAt < UI_ERROR_LOG_THROTTLE_MS) return;
  lastLoggedAt = now;
  const activeElement = typeof document === 'undefined'
    ? null
    : describeEventTarget(document.activeElement);
  const diagnostic = {
    message: extractErrorMessage(error),
    stack: firstStackLines(extractErrorStack(error)),
    activeElement,
    lastPointerTarget,
    lastPointerAt,
    ...source,
  };
  console.error(formatUiErrorDiagnostic(diagnostic));
}

export function formatUiErrorDiagnostic(diagnostic: Record<string, unknown>) {
  const lines = Object.entries(diagnostic).map(([key, value]) => (
    `${key}=${formatDiagnosticValue(value)}`
  ));
  return `[gb-ui-error] maximum update depth diagnostic\n${lines.join('\n')}`;
}

function formatDiagnosticValue(value: unknown) {
  if (value === null || value === undefined) return 'null';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function firstStackLines(stack: string | null) {
  if (!stack) return null;
  return stack.split('\n').slice(0, 12).join('\n');
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
  const ariaLabel = element.getAttribute('aria-label');
  const title = element.getAttribute('title');
  const label = ariaLabel || title || textSnippet(element);
  return [
    `${tag}${id}`,
    dataSlot ? `[data-slot="${dataSlot}"]` : null,
    role ? `[role="${role}"]` : null,
    label ? `[label="${label}"]` : null,
  ].filter(Boolean).join('');
}

function textSnippet(element: Element) {
  const text = element.textContent?.replace(/\s+/g, ' ').trim();
  if (!text) return null;
  return text.length > 48 ? `${text.slice(0, 45)}...` : text;
}
