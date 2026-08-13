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

export function readAgentMessageSelection(
  selection: Selection | null,
  root: HTMLElement | null,
): AgentMessageSelection | null {
  if (!selection || selection.isCollapsed || selection.rangeCount === 0 || !root) return null;
  const start = closestQuotable(selection.anchorNode);
  const end = closestQuotable(selection.focusNode);
  if (!start || start !== end || !root.contains(start)) return null;
  const text = selection.toString().trim();
  const sourceKey = start.dataset.agentMessageKey?.trim();
  if (!text || !sourceKey) return null;
  const range = selection.getRangeAt(0);
  const rect = typeof range.getBoundingClientRect === 'function'
    ? range.getBoundingClientRect()
    : new DOMRect();
  return { sourceKey, text, rect };
}
