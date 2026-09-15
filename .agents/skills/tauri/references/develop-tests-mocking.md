---
name: tauri-develop-tests-mocking
description: Mocking Tauri in frontend tests — mockIPC, mockWindows, clearMocks. Use when unit testing frontend code that calls invoke or window APIs.
---

# Mocking Tauri APIs (Frontend Tests)

When testing the **frontend** (e.g. with Vitest or Jest), you often need to fake the Tauri environment: windows, IPC, etc. The **@tauri-apps/api/mocks** module provides helpers for this.

## mockIPC

Intercept **invoke** calls so tests don’t hit the real backend:

```javascript
import { mockIPC } from '@tauri-apps/api/mocks';
import { invoke } from '@tauri-apps/api/core';

mockIPC((cmd, args) => {
  if (cmd === 'add') return (args.a ?? 0) + (args.b ?? 0);
  return null;
});

// Now invoke('add', { a: 1, b: 2 }) resolves to 3 in tests.
```

Use with a test runner’s spy (e.g. `vi.fn()`) to assert how many times a command was invoked and with which arguments.

## mockWindows

Provide fake window labels and optional window objects so code that uses `getCurrentWindow()`, `getWindow(label)`, or similar gets a predictable environment. See the [mocks](/reference/javascript/api/namespacemocks/) API for the exact shape.

## clearMocks

**Always** clear mocks between tests so state doesn’t leak (e.g. `afterEach(() => clearMocks())`). Use **clearMocks()** from `@tauri-apps/api/mocks`.

## WebCrypto

Some Tauri APIs (e.g. isolation, crypto) expect **window.crypto.getRandomValues**. In jsdom, you may need to polyfill:

```javascript
import { randomFillSync } from 'crypto';
beforeAll(() => {
  Object.defineProperty(window, 'crypto', {
    value: { getRandomValues: (buffer) => randomFillSync(buffer) },
  });
});
```

## Key Points

- **mockIPC** is the most common: implement a (cmd, args) → return value handler so **invoke** returns synchronously in tests. Clear mocks after each test.
- For **Rust**-side tests (command logic, no WebView), use the **mock runtime** (see [Tests](/develop/tests/)); no frontend mocks needed.

<!--
Source references:
- https://v2.tauri.app/develop/tests/mocking/
- https://github.com/tauri-apps/tauri-docs
-->
