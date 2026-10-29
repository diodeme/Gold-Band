/** @vitest-environment jsdom */
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterAll, describe, expect, it } from 'vitest';
import { routeFromPath } from '../src/routes';

const originalUrl = window.location.href;
window.history.replaceState(null, '', '/chat/projects/default/scheduled-tasks/scheduled-a');
const { App } = await import('../src/App');

describe('scheduled task detail app entry', () => {
  afterAll(() => {
    window.history.replaceState(null, '', originalUrl);
  });

  it('renders a scheduled task detail deep link without crashing the app root', () => {
    expect(routeFromPath(window.location.pathname).conversationPage).toEqual({
      kind: 'scheduled-task-detail', projectId: 'default', scheduledTaskId: 'scheduled-a',
    });
    expect(() => renderToStaticMarkup(React.createElement(App))).not.toThrow();
  });
});
