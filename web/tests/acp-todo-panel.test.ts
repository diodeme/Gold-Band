import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { AcpTodoPanel } from '@/components/acp/ACPChatDialog';

describe('ACP todo panel', () => {
  it('renders canonical plan states as compact task rows', () => {
    const html = renderToStaticMarkup(createElement(AcpTodoPanel, {
      variant: 'nested',
      entries: [
        { content: 'Inspect repository', status: 'completed' },
        { content: 'Update conversation UI', status: 'in_progress' },
        { content: 'Verify behavior', priority: 'high' },
      ],
    }));

    expect(html).toContain('data-acp-todo-panel="true"');
    expect(html.match(/data-acp-todo-row="true"/g)).toHaveLength(3);
    expect(html).toContain('Inspect repository');
    expect(html).toContain('Update conversation UI');
    expect(html).toContain('Verify behavior');
    expect(html).toContain('data-acp-processing-spinner="true"');
    expect(html).toContain('text-emerald-700');
    expect(html).toContain('data-acp-todo-pending-mark="true"');
    expect(html).toContain('size-2');
    expect(html).not.toContain('>3<');
    expect(html).toContain('high');
  });
});
