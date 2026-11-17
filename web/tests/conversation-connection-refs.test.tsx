/** @vitest-environment jsdom */
//
// Contract tests for the conversation "connection reference" card:
//   - only bound connections render; nothing renders when none are bound
//   - the chip reveals the full non-secret spec on demand
//   - removing a connection calls back with its stable id and drops the chip
//   - a failed removal surfaces an inline error and keeps the chip (no silent
//     loss of the binding)

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-i18next')>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/api', () => ({
  listServerConnections: vi.fn().mockResolvedValue([]),
}));

import { ConversationConnectionRefs, ConversationConnectionRefsRow, ConversationHomeAttach } from '../src/components/conversation/ConversationConnectionRefs';
import { TooltipProvider } from '../src/components/ui/tooltip';
import type { ConnectionRefVm, ServerConnectionVm } from '../src/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const connection: ConnectionRefVm = {
  id: 'conn-1',
  name: 'wb-prod-db',
  host: '10.0.0.5',
  port: 22,
  username: 'ops',
  authMethod: 'publicKey',
  env: 'prod',
  jumpRef: null,
};

const available: ServerConnectionVm = {
  id: 'conn-2',
  name: 'edge',
  host: '10.0.0.7',
  port: 22,
  username: 'ops',
  authMethod: 'password',
  keyPath: null,
  jumpRef: null,
  env: 'test',
  createdAt: '2026-09-09T00:00:00Z',
  updatedAt: '2026-09-09T00:00:00Z',
  credentialRef: true,
  jumpLabel: null,
  jumpMissing: false,
};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => {
    root.unmount();
  });
  container.remove();
});

function dispatchPointerEvent(target: Element, type: string) {
  const event = new MouseEvent(type, { bubbles: true, button: 0 });
  Object.defineProperties(event, {
    pointerId: { value: 1 },
    pointerType: { value: 'mouse' },
  });
  target.dispatchEvent(event);
}

function render(
  connections: ConnectionRefVm[],
  onRemove: (id: string) => Promise<void>,
  availableConnections?: ServerConnectionVm[],
  onAttach?: (id: string) => Promise<void>,
) {
  return act(async () => {
    root.render(
      <TooltipProvider>
        <ConversationConnectionRefs
          connections={connections}
          onRemove={onRemove}
          onAttach={onAttach}
          availableConnections={availableConnections}
        />
      </TooltipProvider>,
    );
  });
}

describe('conversation connection reference card', () => {
  it('renders nothing when no connection is bound', async () => {
    await render([], vi.fn());
    expect(container.querySelector('[data-conversation-connection-refs="true"]')).toBeNull();
  });

  it('renders one chip per bound connection showing name and host:port', async () => {
    await render([connection], vi.fn());
    const card = container.querySelector('[data-conversation-connection-refs="true"]');
    expect(card).not.toBeNull();
    expect(container.textContent).toContain('wb-prod-db');
    expect(container.textContent).toContain('10.0.0.5:22');
  });

  it('reveals the full non-secret spec on demand and never the key path', async () => {
    await render([connection], vi.fn());
    const trigger = container.querySelector<HTMLButtonElement>('button[aria-expanded]');
    expect(trigger).not.toBeNull();
    await act(async () => {
      trigger!.click();
    });
    // The popover portals to the body; assert against the document.
    const text = document.body.textContent ?? '';
    expect(text).toContain('ops');
    expect(text).toContain('connections.dialog.authPublicKey');
    expect(text).not.toContain('id_ed25519');
  });

  it('removes a connection by its stable id', async () => {
    const onRemove = vi.fn().mockResolvedValue(undefined);
    await render([connection], onRemove);
    const trigger = container.querySelector<HTMLButtonElement>('button[aria-expanded]');
    await act(async () => {
      trigger!.click();
    });
    const removeButton = Array.from(document.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('connections.reference.remove'),
    );
    expect(removeButton).toBeDefined();
    await act(async () => {
      removeButton!.click();
    });
    expect(onRemove).toHaveBeenCalledWith('conn-1');
  });

  it('keeps the chip and surfaces an error when removal fails', async () => {
    const onRemove = vi.fn().mockRejectedValue(new Error('boom'));
    await render([connection], onRemove);
    const trigger = container.querySelector<HTMLButtonElement>('button[aria-expanded]');
    await act(async () => {
      trigger!.click();
    });
    const removeButton = Array.from(document.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('connections.reference.remove'),
    );
    await act(async () => {
      removeButton!.click();
    });
    expect(document.body.textContent).toContain('connections.reference.removeFailed');
    expect(container.textContent).toContain('wb-prod-db');
  });

  it('renders the attach entry even when nothing is bound', async () => {
    await render([], vi.fn(), [available], vi.fn().mockResolvedValue(undefined));
    const card = container.querySelector('[data-conversation-connection-refs="true"]');
    expect(card).not.toBeNull();
    expect(container.textContent).toContain('connections.reference.attach');
  });

  it('attaches an available connection by its stable id and hides it from the menu once bound', async () => {
    const onAttach = vi.fn().mockResolvedValue(undefined);
    // Bindings arrive via the run snapshot; after attaching, the parent passes
    // the same connection inside `connections` — the menu must filter it out.
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ConversationConnectionRefs
            connections={[]}
            onRemove={vi.fn()}
            onAttach={onAttach}
            availableConnections={[available]}
          />
        </TooltipProvider>,
      );
    });
    const attachButton = Array.from(container.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('connections.reference.attach'),
    );
    expect(attachButton).toBeDefined();
    await act(async () => {
      attachButton!.focus();
      dispatchPointerEvent(attachButton!, 'pointerdown');
    });
    const item = Array.from(document.querySelectorAll('[role="menuitem"]')).find((item) =>
      item.textContent?.includes('edge'),
    );
    expect(item).toBeDefined();
    await act(async () => {
      dispatchPointerEvent(item!, 'pointermove');
      dispatchPointerEvent(item!, 'pointerup');
    });
    expect(onAttach).toHaveBeenCalledWith('conn-2');

    // After the run refreshes, the bound connection leaves the attachable list.
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ConversationConnectionRefs
            connections={[{ ...connection, id: 'conn-2', name: 'edge' }]}
            onRemove={vi.fn()}
            onAttach={onAttach}
            availableConnections={[available]}
          />
        </TooltipProvider>,
      );
    });
    const attachAfter = Array.from(container.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('connections.reference.attach'),
    );
    expect(attachAfter).toBeUndefined();
  });

  it('shows an attach failure without dropping the entry', async () => {
    const onAttach = vi.fn().mockRejectedValue(new Error('boom'));
    await render([], vi.fn(), [available], onAttach);
    const attachButton = Array.from(container.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('connections.reference.attach'),
    );
    await act(async () => {
      attachButton!.focus();
      dispatchPointerEvent(attachButton!, 'pointerdown');
    });
    const item = Array.from(document.querySelectorAll('[role="menuitem"]')).find((item) =>
      item.textContent?.includes('edge'),
    );
    await act(async () => {
      dispatchPointerEvent(item!, 'pointermove');
      dispatchPointerEvent(item!, 'pointerup');
    });
    expect(document.body.textContent).toContain('connections.reference.attachFailed');
  });

  // ── ConversationConnectionRefsRow (page-level wrapper) ──

  it('row renders nothing without any handler (feature hidden)', async () => {
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ConversationConnectionRefsRow connections={[connection]} />
        </TooltipProvider>,
      );
    });
    expect(container.querySelector('[data-conversation-connection-refs="true"]')).toBeNull();
  });

  it('row renders the attach entry in Direct conversations too (no bound connections)', async () => {
    // Direct sessions swap the run header for their own title; the page-level
    // row is what keeps the attach entry visible there.
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ConversationConnectionRefsRow
            connections={[]}
            onAttach={vi.fn().mockResolvedValue(undefined)}
          />
        </TooltipProvider>,
      );
    });
    await act(async () => {});
    // No connections exist in the (unmocked) list → the row itself renders but
    // the card hides when there is nothing attachable; the wrapper must still
    // be present so the entry appears once a connection exists.
    expect(container.querySelector('.bg-content-header')).not.toBeNull();
  });

  // ── ConversationHomeAttach (home-composer pre-first-turn picker) ──

  it('home attach picker renders nothing while the list has no connections', async () => {
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ConversationHomeAttach selectedId={null} onSelect={vi.fn()} />
        </TooltipProvider>,
      );
    });
    await act(async () => {});
    expect(container.querySelector('button')).toBeNull();
  });

  it('home attach picker selects and toggles a connection', async () => {
    // The picker loads its own list via the api — provide the mock data.
    const { listServerConnections } = await import('../src/api');
    vi.mocked(listServerConnections).mockResolvedValue([available]);
    const onSelect = vi.fn();
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ConversationHomeAttach selectedId={null} onSelect={onSelect} />
        </TooltipProvider>,
      );
    });
    await act(async () => {});
    const trigger = Array.from(container.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('connections.reference.attach'),
    );
    expect(trigger).toBeDefined();
    await act(async () => {
      trigger!.focus();
      dispatchPointerEvent(trigger!, 'pointerdown');
    });
    const item = Array.from(document.querySelectorAll('[role="menuitem"]')).find((item) =>
      item.textContent?.includes('edge'),
    );
    await act(async () => {
      dispatchPointerEvent(item!, 'pointermove');
      dispatchPointerEvent(item!, 'pointerup');
    });
    expect(onSelect).toHaveBeenCalledWith('conn-2');

    // Selecting the already-selected connection toggles it off.
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ConversationHomeAttach selectedId="conn-2" onSelect={onSelect} />
        </TooltipProvider>,
      );
    });
    const trigger2 = Array.from(container.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('edge'),
    );
    await act(async () => {
      trigger2!.focus();
      dispatchPointerEvent(trigger2!, 'pointerdown');
    });
    const item2 = Array.from(document.querySelectorAll('[role="menuitem"]')).find((item) =>
      item.textContent?.includes('edge'),
    );
    await act(async () => {
      dispatchPointerEvent(item2!, 'pointermove');
      dispatchPointerEvent(item2!, 'pointerup');
    });
    expect(onSelect).toHaveBeenLastCalledWith(null);
  });
});
