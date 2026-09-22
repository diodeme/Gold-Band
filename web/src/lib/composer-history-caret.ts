export type ComposerHistoryCaretDirection = 'older' | 'newer';

export interface ComposerHistoryCaretProbe {
  caretTop?: (element: HTMLTextAreaElement, position: number) => number;
}

const VISUAL_LINE_EPSILON_PX = 2;

const MIRROR_STYLES = [
  'direction',
  'boxSizing',
  'width',
  'borderTopWidth',
  'borderRightWidth',
  'borderBottomWidth',
  'borderLeftWidth',
  'borderStyle',
  'paddingTop',
  'paddingRight',
  'paddingBottom',
  'paddingLeft',
  'fontStyle',
  'fontVariant',
  'fontWeight',
  'fontStretch',
  'fontSize',
  'fontSizeAdjust',
  'lineHeight',
  'fontFamily',
  'textAlign',
  'textTransform',
  'textIndent',
  'textDecoration',
  'letterSpacing',
  'wordSpacing',
  'tabSize',
  'MozTabSize',
  'whiteSpace',
  'wordBreak',
  'overflowWrap',
] as const;

function copyMirrorStyles(element: HTMLTextAreaElement, mirror: HTMLElement) {
  const computed = getComputedStyle(element);
  const style = mirror.style as CSSStyleDeclaration & Record<string, string>;
  for (const property of MIRROR_STYLES) {
    style[property] = (computed as CSSStyleDeclaration & Record<string, string>)[property];
  }
  style.position = 'absolute';
  style.visibility = 'hidden';
  style.top = '0';
  style.left = '-9999px';
  style.height = 'auto';
  style.overflow = 'hidden';
  style.whiteSpace = computed.whiteSpace || 'pre-wrap';
  style.wordWrap = 'break-word';
}

function fillAndReadTop(mirror: HTMLElement, value: string, position: number): number {
  mirror.textContent = value.slice(0, position);
  const marker = document.createElement('span');
  marker.textContent = value.slice(position) || '|';
  mirror.append(marker);
  return marker.offsetTop;
}

function measureTextareaCaretTops(
  element: HTMLTextAreaElement,
  left: number,
  right: number,
): [number, number] {
  const mirror = document.createElement('div');
  copyMirrorStyles(element, mirror);
  document.body.append(mirror);
  try {
    return [
      fillAndReadTop(mirror, element.value, left),
      fillAndReadTop(mirror, element.value, right),
    ];
  } finally {
    mirror.remove();
  }
}

function sameVisualLine(
  element: HTMLTextAreaElement,
  left: number,
  right: number,
  probe?: ComposerHistoryCaretProbe,
): boolean {
  if (left === right) return true;
  if (probe?.caretTop) {
    return Math.abs(probe.caretTop(element, left) - probe.caretTop(element, right)) < VISUAL_LINE_EPSILON_PX;
  }
  const [topLeft, topRight] = measureTextareaCaretTops(element, left, right);
  return Math.abs(topLeft - topRight) < VISUAL_LINE_EPSILON_PX;
}

export function isComposerHistoryBoundary(
  element: HTMLTextAreaElement,
  direction: ComposerHistoryCaretDirection,
  probe?: ComposerHistoryCaretProbe,
): boolean {
  if (element.selectionStart !== element.selectionEnd) return false;
  const { value } = element;
  const position = element.selectionStart ?? 0;
  const edge = direction === 'older' ? 0 : value.length;
  if (position === edge) return true;
  if (direction === 'older') {
    if (value.slice(0, position).includes('\n')) return false;
  } else if (value.slice(position).includes('\n')) {
    return false;
  }
  return sameVisualLine(element, position, edge, probe);
}
