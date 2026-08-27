/** @vitest-environment jsdom */

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "../src/i18n";
import {
  USER_MESSAGE_COLLAPSED_MAX_HEIGHT_PX,
  UserMessageDisclosure,
} from "@/components/conversation/UserMessageDisclosure";
import * as chatContainer from "@/components/prompt-kit/chat-container";

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let observedResize: (() => void) | null = null;

beforeEach(() => {
  observedResize = null;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        observedResize = () => callback([], this as unknown as ResizeObserver);
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

async function renderDisclosure(scrollHeight: number) {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <UserMessageDisclosure>
        <div data-message-text="true">A user message</div>
      </UserMessageDisclosure>,
    );
  });
  const measuredContent = container.querySelector<HTMLElement>(
    "[data-user-message-disclosure-content] > div",
  );
  Object.defineProperty(measuredContent, "scrollHeight", {
    configurable: true,
    get: () => scrollHeight,
  });
  await act(async () => observedResize?.());
  return { container, root };
}

describe("UserMessageDisclosure", () => {
  it("leaves short user messages fully visible without a disclosure action", async () => {
    const { container, root } = await renderDisclosure(
      USER_MESSAGE_COLLAPSED_MAX_HEIGHT_PX,
    );
    try {
      expect(
        container.querySelector("[data-user-message-disclosure-trigger]"),
      ).toBeNull();
      expect(
        container.querySelector<HTMLElement>(
          "[data-user-message-disclosure-content]",
        )?.dataset.state,
      ).toBe("collapsed");
    } finally {
      await act(async () => root.unmount());
    }
  });

  it("folds overflowing content and expands it through the existing scroll controller", async () => {
    const beginContentExpansion = vi.fn(() => 42);
    const endContentExpansion = vi.fn(() => true);
    vi.spyOn(
      chatContainer,
      "useOptionalChatContainerContentExpansion",
    ).mockReturnValue({ beginContentExpansion, endContentExpansion });
    const { container, root } = await renderDisclosure(
      USER_MESSAGE_COLLAPSED_MAX_HEIGHT_PX + 120,
    );

    try {
      const content = container.querySelector<HTMLElement>(
        "[data-user-message-disclosure-content]",
      );
      const trigger = container.querySelector<HTMLButtonElement>(
        "[data-user-message-disclosure-trigger]",
      );
      expect(content?.dataset.state).toBe("collapsed");
      expect(content?.style.maxHeight).toBe(
        `${USER_MESSAGE_COLLAPSED_MAX_HEIGHT_PX}px`,
      );
      expect(trigger?.textContent).toContain("查看更多");
      expect(trigger?.getAttribute("aria-expanded")).toBe("false");

      await act(async () => trigger!.click());
      expect(beginContentExpansion).toHaveBeenCalledTimes(1);
      expect(content?.dataset.state).toBe("expanded");
      expect(content?.style.maxHeight).toBe("");
      expect(trigger?.textContent).toContain("收起");
      expect(trigger?.getAttribute("aria-expanded")).toBe("true");

      await act(async () => trigger!.click());
      expect(endContentExpansion).toHaveBeenCalledWith(42);
      expect(content?.dataset.state).toBe("collapsed");
      expect(content?.style.maxHeight).toBe(
        `${USER_MESSAGE_COLLAPSED_MAX_HEIGHT_PX}px`,
      );
    } finally {
      await act(async () => root.unmount());
    }
  });
});
