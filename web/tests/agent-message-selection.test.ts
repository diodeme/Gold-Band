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
