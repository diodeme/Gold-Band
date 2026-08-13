export const AGENT_QUOTABLE_SELECTOR = '[data-agent-quotable-text="true"]';

export interface AgentMessageSelection {
  sourceKey: string;
  text: string;
  rect: DOMRect;
}

function closestQuotable(node: Node | null) {
  const element = node instanceof Element ? node : node?.parentElement;
  return element?.closest<HTMLElement>(AGENT_QUOTABLE_SELECTOR) ?? null;
}

function firstTextDescendant(node: Node): Text | null {
  if (node.nodeType === Node.TEXT_NODE) return node as Text;
  for (let child = node.firstChild; child; child = child.nextSibling) {
    const text = firstTextDescendant(child);
    if (text) return text;
  }
  return null;
}

function lastTextDescendant(node: Node): Text | null {
  if (node.nodeType === Node.TEXT_NODE) return node as Text;
  for (let child = node.lastChild; child; child = child.previousSibling) {
    const text = lastTextDescendant(child);
    if (text) return text;
  }
  return null;
}

function nextTextAfter(node: Node, root: HTMLElement): Text | null {
  let current: Node | null = node;
  while (current && current !== root) {
    for (let sibling = current.nextSibling; sibling; sibling = sibling.nextSibling) {
      const text = firstTextDescendant(sibling);
      if (text) return text;
    }
    current = current.parentNode;
  }
  return null;
}

function previousTextBefore(node: Node, root: HTMLElement): Text | null {
  let current: Node | null = node;
  while (current && current !== root) {
    for (let sibling = current.previousSibling; sibling; sibling = sibling.previousSibling) {
      const text = lastTextDescendant(sibling);
      if (text) return text;
    }
    current = current.parentNode;
  }
  return null;
}

function firstTextAtBoundary(container: Node, offset: number, root: HTMLElement): Text | null {
  if (container.nodeType === Node.TEXT_NODE) {
    const text = container as Text;
    return offset < text.length ? text : nextTextAfter(text, root);
  }
  const child = container.childNodes[offset];
  return child ? firstTextDescendant(child) ?? nextTextAfter(child, root) : nextTextAfter(container, root);
}

function lastTextAtBoundary(container: Node, offset: number, root: HTMLElement): Text | null {
  if (container.nodeType === Node.TEXT_NODE) {
    const text = container as Text;
    return offset > 0 ? text : previousTextBefore(text, root);
  }
  const child = offset > 0 ? container.childNodes[offset - 1] : null;
  return child ? lastTextDescendant(child) ?? previousTextBefore(child, root) : previousTextBefore(container, root);
}

function selectedTextInNode(range: Range, node: Text) {
  if (!range.intersectsNode(node)) return '';
  const start = range.startContainer === node ? range.startOffset : 0;
  const end = range.endContainer === node ? range.endOffset : node.length;
  return node.data.slice(start, end);
}

function selectedBoundaryText(
  range: Range,
  root: HTMLElement,
  direction: 'forward' | 'backward',
) {
  let node = direction === 'forward'
    ? firstTextAtBoundary(range.startContainer, range.startOffset, root)
    : lastTextAtBoundary(range.endContainer, range.endOffset, root);
  while (node && root.contains(node)) {
    if (selectedTextInNode(range, node).trim()) return node;
    node = direction === 'forward'
      ? nextTextAfter(node, root)
      : previousTextBefore(node, root);
  }
  return null;
}

export function readAgentMessageSelection(
  selection: Selection | null,
  root: HTMLElement | null,
): AgentMessageSelection | null {
  if (!selection || selection.isCollapsed || selection.rangeCount === 0 || !root) return null;
  const range = selection.getRangeAt(0);
  const start = closestQuotable(selectedBoundaryText(range, root, 'forward'));
  const end = closestQuotable(selectedBoundaryText(range, root, 'backward'));
  if (!start || start !== end || !root.contains(start)) return null;
  const text = range.toString().trim();
  const sourceKey = start.dataset.agentMessageKey?.trim();
  if (!text || !sourceKey) return null;
  const rect = typeof range.getBoundingClientRect === 'function'
    ? range.getBoundingClientRect()
    : new DOMRect();
  return { sourceKey, text, rect };
}
