/** @vitest-environment jsdom */

import { describe, expect, it } from 'vitest';
import { readAgentMessageSelection } from '@/lib/agent-message-selection';

function select(start: Node, end: Node) {
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  const range = document.createRange();
  range.setStart(start, 0);
  range.setEnd(end, end.textContent?.length ?? 0);
  selection.addRange(range);
  return selection;
}

function selectElementFromParent(element: Element) {
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  const range = document.createRange();
  const parent = element.parentNode!;
  const index = Array.prototype.indexOf.call(parent.childNodes, element);
  range.setStart(parent, index);
  range.setEnd(parent, index + 1);
  selection.addRange(range);
  return selection;
}

describe('agent message quote selection boundary', () => {
  it('accepts text contained by one completed agent message', () => {
    const root = document.createElement('div');
    root.innerHTML = '<div data-agent-quotable-text="true" data-agent-message-key="answer-1"><p>可引用文本</p></div>';
    document.body.appendChild(root);
    const text = root.querySelector('p')!.firstChild!;
    const result = readAgentMessageSelection(select(text, text), root);
    expect(result).toMatchObject({ sourceKey: 'answer-1', text: '可引用文本' });
    root.remove();
  });

  it('accepts a whole agent message when browser selection endpoints are outside its element', () => {
    const root = document.createElement('div');
    root.innerHTML = [
      '<span>17:00</span>',
      '<div data-agent-quotable-text="true" data-agent-message-key="answer-1"><p>整条 Agent 正文</p></div>',
    ].join('');
    document.body.appendChild(root);
    const message = root.querySelector('[data-agent-quotable-text]')!;
    const result = readAgentMessageSelection(selectElementFromParent(message), root);
    expect(result).toMatchObject({ sourceKey: 'answer-1', text: '整条 Agent 正文' });
    root.remove();
  });

  it('skips empty boundary elements when resolving the selected message text', () => {
    const root = document.createElement('div');
    root.innerHTML = '<div data-agent-quotable-text="true" data-agent-message-key="answer-1"><span></span><p>实际正文</p><span></span></div>';
    document.body.appendChild(root);
    const message = root.querySelector('[data-agent-quotable-text]')!;
    const result = readAgentMessageSelection(selectElementFromParent(message), root);
    expect(result).toMatchObject({ sourceKey: 'answer-1', text: '实际正文' });
    root.remove();
  });

  it('rejects a whole row selection that also contains non-agent text', () => {
    const root = document.createElement('div');
    root.innerHTML = '<div><span>17:00</span><div data-agent-quotable-text="true" data-agent-message-key="answer-1">Agent 正文</div></div>';
    document.body.appendChild(root);
    expect(readAgentMessageSelection(selectElementFromParent(root.firstElementChild!), root)).toBeNull();
    root.remove();
  });

  it('rejects selections crossing an activity fold into another agent message', () => {
    const root = document.createElement('div');
    root.innerHTML = [
      '<div data-agent-quotable-text="true" data-agent-message-key="answer-1">第一条</div>',
      '<div>工具活动折叠区</div>',
      '<div data-agent-quotable-text="true" data-agent-message-key="answer-2">第二条</div>',
    ].join('');
    document.body.appendChild(root);
    const messages = root.querySelectorAll('[data-agent-quotable-text]');
    expect(readAgentMessageSelection(select(messages[0]!.firstChild!, messages[1]!.firstChild!), root)).toBeNull();
    root.remove();
  });

  it('rejects user, thought, tool, permission, and elicitation text without the trusted marker', () => {
    const root = document.createElement('div');
    root.innerHTML = '<div data-kind="permission">不可引用</div>';
    document.body.appendChild(root);
    const text = root.firstChild!.firstChild!;
    expect(readAgentMessageSelection(select(text, text), root)).toBeNull();
    root.remove();
  });
});
