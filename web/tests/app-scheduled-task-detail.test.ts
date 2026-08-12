import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const originalWindow = globalThis.window;
const originalLocalStorage = globalThis.localStorage;

describe('scheduled task detail app entry', () => {
  beforeAll(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: {
        innerWidth: 1440,
        location: {
          pathname: '/chat/scheduled-tasks/scheduled-a',
          search: '',
          hash: '',
        },
      },
    });
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: {
        getItem: () => null,
        setItem: () => undefined,
      },
    });
  });

  afterAll(() => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
    Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: originalLocalStorage });
  });

  it('renders a scheduled task detail deep link without crashing the app root', async () => {
    const { App } = await import('../src/App');

    expect(() => renderToStaticMarkup(React.createElement(App))).not.toThrow();
  }, 60_000);
});
