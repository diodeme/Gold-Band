/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ConversationWorkspaceControl, ConversationWorkspaceInfoBar } from '@/components/conversation/ConversationComposer';
import {
  GitBranchPickerSnapshotProvider,
  GitBranchPickerSnapshotStore,
} from '@/components/git/GitBranchPickerSnapshotContext';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const workspaces = [
  { projectId: 'gold-band', workspacePath: 'D:/Projects/Gold-Band', name: 'Gold Band' },
  { projectId: 'long', workspacePath: 'D:/Projects/Long', name: 'A very long workspace name that must be truncated' },
];

function dispatchPointerEvent(target: Element, type: string) {
  const event = new MouseEvent(type, { bubbles: true, button: 0 });
  Object.defineProperties(event, {
    pointerId: { value: 1 },
    pointerType: { value: 'mouse' },
  });
  target.dispatchEvent(event);
}

describe('quick conversation workspace control', () => {
  let host: HTMLDivElement;
  let root: Root;
  let addedHTMLElementMethods: string[];
  let branchSnapshotStore: GitBranchPickerSnapshotStore;

  beforeEach(() => {
    addedHTMLElementMethods = [];
    for (const method of ['hasPointerCapture', 'setPointerCapture', 'releasePointerCapture', 'scrollIntoView']) {
      if (method in HTMLElement.prototype) continue;
      Object.defineProperty(HTMLElement.prototype, method, {
        configurable: true,
        value: method === 'hasPointerCapture' ? () => false : () => {},
      });
      addedHTMLElementMethods.push(method);
    }
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    branchSnapshotStore = new GitBranchPickerSnapshotStore();
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    for (const method of addedHTMLElementMethods) {
      delete (HTMLElement.prototype as unknown as Record<string, unknown>)[method];
    }
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  async function renderControl(projectId: string) {
    await act(async () => {
      root.render(
        <ConversationWorkspaceControl
          projectId={projectId}
          workspaceName="Fallback workspace"
          workspaces={workspaces}
          onWorkspaceChange={() => {}}
        />,
      );
    });

    const trigger = host.querySelector<HTMLButtonElement>('[data-slot="select-trigger"]');
    const value = host.querySelector<HTMLElement>('[data-conversation-workspace-value="true"]');
    expect(trigger).not.toBeNull();
    expect(value).not.toBeNull();
    return { trigger: trigger!, value: value! };
  }

  it('sizes to its content while preserving the available-width ceiling', async () => {
    const { trigger } = await renderControl('gold-band');

    expect(trigger.className).toContain('w-fit');
    expect(trigger.className).toContain('max-w-full');
    expect(trigger.className).toContain('flex-initial');
    expect(trigger.className).not.toContain('flex-1');
    expect(trigger.textContent).toContain('Gold Band');
  });

  it('shows the complete workspace name when the selected value is truncated', async () => {
    const { trigger, value } = await renderControl('long');
    Object.defineProperties(value, {
      clientWidth: { configurable: true, value: 120 },
      scrollWidth: { configurable: true, value: 340 },
    });

    await act(async () => {
      trigger.dispatchEvent(new MouseEvent('pointerover', { bubbles: true }));
    });

    expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent)
      .toBe('A very long workspace name that must be truncated');
  });

  it('does not show a redundant tooltip when the workspace name fits', async () => {
    const { trigger, value } = await renderControl('gold-band');
    Object.defineProperties(value, {
      clientWidth: { configurable: true, value: 120 },
      scrollWidth: { configurable: true, value: 120 },
    });

    await act(async () => trigger.focus());

    expect(document.body.querySelector('[data-slot="tooltip-content"]')).toBeNull();
  });

  it('keeps a truncated single-workspace label available from the keyboard', async () => {
    await act(async () => {
      root.render(
        <ConversationWorkspaceControl
          projectId="long"
          workspaceName="Fallback workspace"
          workspaces={[workspaces[1]]}
          onWorkspaceChange={() => {}}
        />,
      );
    });
    const control = host.querySelector<HTMLElement>('[tabindex="0"]');
    const value = host.querySelector<HTMLElement>('[data-conversation-workspace-value="true"]');
    expect(control).not.toBeNull();
    expect(value).not.toBeNull();
    Object.defineProperties(value!, {
      clientWidth: { configurable: true, value: 120 },
      scrollWidth: { configurable: true, value: 340 },
    });

    await act(async () => control!.focus());

    expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent)
      .toBe('A very long workspace name that must be truncated');
  });

  it('does not leave pointer focus on the workspace trigger after a selection closes', async () => {
    const onWorkspaceChange = vi.fn();
    await act(async () => {
      root.render(
        <ConversationWorkspaceControl
          projectId="gold-band"
          workspaceName="Fallback workspace"
          workspaces={workspaces}
          onWorkspaceChange={onWorkspaceChange}
        />,
      );
    });
    const trigger = host.querySelector<HTMLButtonElement>('[data-slot="select-trigger"]');
    expect(trigger).not.toBeNull();

    await act(async () => {
      trigger!.focus();
      dispatchPointerEvent(trigger!, 'pointerdown');
    });
    const item = Array.from(document.body.querySelectorAll<HTMLElement>('[data-slot="select-item"]'))
      .find((candidate) => candidate.textContent?.includes('A very long workspace name'));
    expect(item).not.toBeNull();

    await act(async () => {
      dispatchPointerEvent(item!, 'pointermove');
      dispatchPointerEvent(item!, 'pointerup');
    });

    expect(onWorkspaceChange).toHaveBeenCalledWith('long');
    expect(document.activeElement).not.toBe(trigger);
  });

  it('keeps Radix focus restoration when the workspace selector closes from the keyboard', async () => {
    await act(async () => {
      root.render(
        <ConversationWorkspaceControl
          projectId="gold-band"
          workspaceName="Fallback workspace"
          workspaces={workspaces}
          onWorkspaceChange={() => {}}
        />,
      );
    });
    const trigger = host.querySelector<HTMLButtonElement>('[data-slot="select-trigger"]');
    expect(trigger).not.toBeNull();

    await act(async () => {
      trigger!.focus();
      trigger!.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'ArrowDown' }));
    });
    await act(async () => {
      document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Escape' }));
    });

    expect(document.activeElement).toBe(trigger);
  });

  it('combines workspace and persisted work-location controls in the quick composer info bar', async () => {
    await act(async () => {
      root.render(
        <GitBranchPickerSnapshotProvider store={branchSnapshotStore}>
          <ConversationWorkspaceInfoBar
            projectId="gold-band"
            workspaceName="Fallback workspace"
            workspaces={workspaces}
            workLocation="main"
            busy={false}
            onWorkspaceChange={() => {}}
            onWorkLocationChange={() => {}}
          />
        </GitBranchPickerSnapshotProvider>,
      );
    });

    const infoBar = host.querySelector<HTMLElement>('[data-conversation-workspace-info="true"]');
    const workspaceTrigger = infoBar?.querySelector<HTMLElement>('[data-slot="select-trigger"]');
    const workLocationTrigger = infoBar?.querySelector<HTMLElement>('[data-conversation-work-location-trigger="true"]');
    const branchTrigger = infoBar?.querySelector<HTMLElement>('[data-git-branch-selector="editable"]');
    const workspaceValue = infoBar?.querySelector<HTMLElement>('[data-conversation-workspace-value="true"]');
    const workLocationValue = infoBar?.querySelector<HTMLElement>('[data-conversation-work-location-value="true"]');
    const branchValue = infoBar?.querySelector<HTMLElement>('[data-git-branch-value="true"]');
    expect(infoBar).not.toBeNull();
    expect(workspaceTrigger).not.toBeNull();
    expect(workLocationTrigger).not.toBeNull();
    expect(branchTrigger).not.toBeNull();
    expect(workspaceTrigger!.dataset.contextControl).toBe('workspace');
    expect(workLocationTrigger!.dataset.contextControl).toBe('work-location');
    expect(workLocationTrigger!.getAttribute('data-theme-role')).toBeNull();
    for (const trigger of [workspaceTrigger!, workLocationTrigger!]) {
      const classes = trigger.className.split(' ');
      expect(classes).toContain('bg-transparent');
      expect(classes).not.toContain('bg-accent');
      expect(classes).toContain('transition-colors');
      expect(classes).toContain('hover:bg-accent');
      expect(classes).toContain('focus-visible:bg-accent');
      expect(classes).toContain('data-[state=open]:bg-accent');
      expect(classes).toContain('dark:bg-transparent');
      expect(classes).toContain('dark:hover:bg-accent/50');
    }
    expect(workspaceTrigger!.className.split(' ')).toContain('data-[size=default]:h-7');
    expect(workLocationTrigger!.className.split(' ')).toContain('h-7');
    expect(workspaceTrigger!.className.split(' ')).toContain('w-7');
    expect(workspaceTrigger!.className.split(' ')).toContain('@xs/conversation-context:w-fit');
    expect(workLocationTrigger!.className.split(' ')).toContain('w-7');
    expect(workLocationTrigger!.className.split(' ')).toContain('@md/conversation-context:w-auto');
    expect(branchTrigger!.className.split(' ')).toContain('w-7');
    expect(branchTrigger!.className.split(' ')).toContain('@md/conversation-context:w-auto');
    expect(workspaceValue!.className.split(' ')).toContain('hidden');
    expect(workspaceValue!.className.split(' ')).toContain('@xs/conversation-context:inline');
    expect(workLocationValue!.className.split(' ')).toContain('hidden');
    expect(workLocationValue!.className.split(' ')).toContain('@md/conversation-context:inline');
    expect(branchValue!.className.split(' ')).toContain('hidden');
    expect(branchValue!.className.split(' ')).toContain('@md/conversation-context:inline');
    expect(workspaceTrigger!.getAttribute('aria-label')).toBe('工作空间: Gold Band');
    expect(workLocationTrigger!.getAttribute('aria-label')).toBe('工作位置: 主工作区');
    expect(infoBar!.className).toContain('mx-auto');
    expect(infoBar!.className).toContain('w-[80%]');
    expect(infoBar!.className).toContain('@container/conversation-context');
    expect(infoBar!.className).not.toContain('mx-9');
    expect(infoBar!.className).toContain('h-7');
    expect(infoBar!.className).toContain('items-center');
    expect(infoBar!.className).toContain('justify-start');
    expect(infoBar!.className).toContain('gap-0');
    expect(infoBar!.className).toContain('px-8');
    expect(infoBar!.className).not.toContain('justify-center');
    expect(infoBar!.className).toContain('[--conversation-workspace-info-surface:var(--gold-surface-high)]');
    expect(infoBar!.className).not.toContain('var(--gb-conversation-background)');
    expect(infoBar!.className).not.toContain('rounded-t-2xl');
    expect(infoBar!.className).not.toContain('before:');
    expect(infoBar!.className).not.toContain('after:');
    expect(infoBar!.className).not.toContain('bg-muted/45');
    expect(infoBar!.className).not.toContain('pt-2');
    expect(infoBar!.querySelector('[data-conversation-workspace-info-body="true"]')).not.toBeNull();
    expect(infoBar!.querySelector('[data-conversation-workspace-info-controls="true"]')).not.toBeNull();
    const curves = infoBar!.querySelectorAll<SVGSVGElement>('[data-conversation-workspace-info-curve]');
    expect(curves).toHaveLength(2);
    expect(curves[0]?.getAttribute('viewBox')).toBe('0 0 48 28');
    expect(curves[0]?.classList).toContain('left-0');
    expect(curves[0]?.classList).toContain('w-12');
    expect(curves[0]?.classList).not.toContain('-left-12');
    expect(curves[0]?.classList).not.toContain('w-9');
    expect(curves[0]?.querySelector('path')?.getAttribute('d')).toBe('M0 28L20.14 4Q23.497 0 29.497 0H48V28Z');
    expect(curves[0]?.querySelector('path')?.getAttribute('transform')).toBeNull();
    expect(curves[1]?.classList).toContain('right-0');
    expect(curves[1]?.classList).toContain('w-12');
    expect(curves[1]?.classList).not.toContain('-right-12');
    expect(curves[1]?.classList).not.toContain('w-9');
    expect(curves[1]?.querySelector('path')?.getAttribute('d')).toBe('M0 28L20.14 4Q23.497 0 29.497 0H48V28Z');
    expect(curves[1]?.querySelector('path')?.getAttribute('transform')).toBe('translate(48 0) scale(-1 1)');
    expect(host.querySelector('[data-conversation-workspace-value="true"]')?.textContent).toBe('Gold Band');
  });

  it('keeps the compact context controls mounted while their values change', async () => {
    const renderInfoBar = async (workLocation: 'main' | 'worktree') => {
      await act(async () => {
        root.render(
          <GitBranchPickerSnapshotProvider store={branchSnapshotStore}>
            <ConversationWorkspaceInfoBar
              projectId="gold-band"
              workspaceName="Fallback workspace"
              workspaces={workspaces}
              workLocation={workLocation}
              busy={false}
              onWorkspaceChange={() => {}}
              onWorkLocationChange={() => {}}
            />
          </GitBranchPickerSnapshotProvider>,
        );
      });
    };

    await renderInfoBar('main');
    const workspaceTrigger = host.querySelector('[data-slot="select-trigger"]');
    const workLocationTrigger = host.querySelector('[data-conversation-work-location-trigger="true"]');
    const branchTrigger = host.querySelector('[data-git-branch-selector="editable"]');

    await renderInfoBar('worktree');

    expect(host.querySelector('[data-slot="select-trigger"]')).toBe(workspaceTrigger);
    expect(host.querySelector('[data-conversation-work-location-trigger="true"]')).toBe(workLocationTrigger);
    expect(host.querySelector('[data-git-branch-selector="editable"]')).toBe(branchTrigger);
    expect(host.querySelector('[data-conversation-work-location-value="true"]')?.textContent).toBe('工作树');
  });

  it('reuses the same info surface without exposing worktree selection for scheduled authoring', async () => {
    await act(async () => {
      root.render(
        <GitBranchPickerSnapshotProvider store={branchSnapshotStore}>
          <ConversationWorkspaceInfoBar
            projectId="gold-band"
            workspaceName="Fallback workspace"
            workspaces={workspaces}
            workLocation="worktree"
            busy={false}
            onWorkspaceChange={() => {}}
            onWorkLocationChange={() => {}}
            showWorkLocation={false}
          />
        </GitBranchPickerSnapshotProvider>,
      );
    });

    const infoBar = host.querySelector<HTMLElement>('[data-conversation-workspace-info="true"]');
    expect(infoBar).not.toBeNull();
    expect(infoBar!.className).toContain('w-[80%]');
    expect(infoBar!.querySelector('[data-slot="select-trigger"]')).not.toBeNull();
    expect(infoBar!.querySelector('[data-conversation-work-location-trigger="true"]')).toBeNull();
    expect(infoBar!.querySelectorAll('[data-conversation-workspace-info-curve]')).toHaveLength(2);
  });

  it('does not restore pointer focus to the work-location trigger after the menu closes', async () => {
    await act(async () => {
      root.render(
        <GitBranchPickerSnapshotProvider store={branchSnapshotStore}>
          <ConversationWorkspaceInfoBar
            projectId="gold-band"
            workspaceName="Fallback workspace"
            workspaces={workspaces}
            workLocation="worktree"
            busy={false}
            onWorkspaceChange={() => {}}
            onWorkLocationChange={() => {}}
          />
        </GitBranchPickerSnapshotProvider>,
      );
    });

    const trigger = host.querySelector<HTMLButtonElement>('[data-conversation-work-location-trigger="true"]');
    expect(trigger).not.toBeNull();
    await act(async () => {
      trigger!.focus();
      dispatchPointerEvent(trigger!, 'pointerdown');
    });
    expect(document.body.querySelector('[data-slot="dropdown-menu-content"]')).not.toBeNull();

    await act(async () => {
      dispatchPointerEvent(document.body, 'pointerdown');
      dispatchPointerEvent(document.body, 'pointerup');
    });

    expect(document.body.querySelector('[data-slot="dropdown-menu-content"]')).toBeNull();
    expect(document.activeElement).not.toBe(trigger);
  });
});
