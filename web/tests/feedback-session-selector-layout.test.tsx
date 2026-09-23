/** @vitest-environment jsdom */

import { act } from "react";
import { createRoot } from "react-dom/client";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const stableMocks = vi.hoisted(() => ({
  t: (key: string) => key,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: stableMocks.t }),
}));

vi.mock("lucide-react", () => ({
  CheckIcon: () => null,
  ChevronDownIcon: () => null,
  ChevronUpIcon: () => null,
  CircleCheck: () => null,
  UploadCloud: () => null,
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: { open: boolean; children: ReactNode }) =>
    open ? <>{children}</> : null,
  DialogContent: ({ className, children }: { className?: string; children: ReactNode }) => (
    <div data-slot="dialog-content" className={className}>{children}</div>
  ),
  DialogDescription: ({ children }: { children: ReactNode }) => <p>{children}</p>,
  DialogFooter: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DialogHeader: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DialogTitle: ({ children }: { children: ReactNode }) => <h2>{children}</h2>,
}));

vi.mock("@/components/shared/AttachmentComponents", () => ({
  AttachmentChipsList: () => null,
  AttachmentPreviewDialogs: () => null,
}));

vi.mock("@/lib/attachment-service", () => {
  const clearAttachments = vi.fn();
  return {
    useAttachmentPicker: () => ({
      attachments: [],
      fileError: null,
      fileInputRef: { current: null },
      addFiles: vi.fn(),
      handleFilesFromInput: vi.fn(),
      removeAttachment: vi.fn(),
      clearAttachments,
      resolveAttachmentInputs: vi.fn().mockResolvedValue([]),
      dropZoneHandlers: {},
      previewImage: null,
      setPreviewImage: vi.fn(),
      textPreview: null,
      setTextPreview: vi.fn(),
      handlePreviewAttachment: vi.fn(),
    }),
    useWindowDragGuard: () => {},
  };
});

const runtimeApiMocks = vi.hoisted(() => ({
  getConversationSidebarBootstrap: vi.fn(),
  getConversationTaskPage: vi.fn(),
  previewFeedbackSessionArchive: vi.fn(),
  submitFeedback: vi.fn(),
}));

vi.mock("@/api/client", () => ({
  getRuntimeApi: () => runtimeApiMocks,
}));

import { FeedbackDialog } from "@/components/feedback/FeedbackDialog";

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const longSessionTitle =
  "A very long feedback-related session title that must stay inside the dialog card even when it contains an unbreakable identifier such as task-123456789012345678901234567890";

describe("FeedbackDialog related session selector layout", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = () => {};
    runtimeApiMocks.getConversationSidebarBootstrap.mockResolvedValue({
      workspaces: [{ projectId: "workspace-1", name: "Workspace" }],
    });
    runtimeApiMocks.getConversationTaskPage.mockImplementation(async () => {
      const page = {
        projectId: "workspace-1",
        tasks: [{ taskId: "task-1", title: longSessionTitle }],
      };
      return page;
    });
    runtimeApiMocks.previewFeedbackSessionArchive.mockResolvedValue(null);
  });

  afterEach(() => {
    vi.clearAllMocks();
    document.body.replaceChildren();
  });

  it("lets the selected session value shrink instead of expanding the dialog card", async () => {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<FeedbackDialog open onOpenChange={() => {}} />);
      });
      let trigger = container.querySelector<HTMLElement>('[data-slot="select-trigger"]');
      for (let attempt = 0; attempt < 10 && !trigger; attempt += 1) {
        await act(async () => {
          await new Promise((resolve) => setTimeout(resolve, 0));
        });
        trigger = container.querySelector<HTMLElement>('[data-slot="select-trigger"]');
      }
      expect(trigger).not.toBeNull();
      expect(trigger?.classList.contains("min-w-0")).toBe(true);
      expect(container.querySelector<HTMLElement>('[data-attachment-dropzone="true"]')?.classList.contains("min-w-0"))
        .toBe(true);

      const relatedSessionLabel = [...container.querySelectorAll("label")]
        .find((label) => label.textContent?.includes("common.feedbackRelatedSession"));
      expect(relatedSessionLabel?.parentElement?.classList.contains("min-w-0")).toBe(true);

      const selectedValue = trigger?.querySelector<HTMLElement>('[data-slot="select-value"]');
      expect(selectedValue).not.toBeNull();
      expect(trigger?.classList.contains("*:data-[slot=select-value]:min-w-0")).toBe(true);
      expect(trigger?.classList.contains("*:data-[slot=select-value]:flex-1")).toBe(true);

      await act(async () => {
        trigger?.focus();
        trigger?.dispatchEvent(new KeyboardEvent("keydown", {
          key: "ArrowDown",
          bubbles: true,
          cancelable: true,
        }));
      });
      const longOption = [...document.querySelectorAll<HTMLElement>('[role="option"]')]
        .find((option) => option.textContent === longSessionTitle);
      expect(longOption).toBeDefined();
      expect(longOption?.classList.contains("min-w-0")).toBe(true);
      expect(longOption?.classList.contains("whitespace-normal")).toBe(true);
      expect(longOption?.classList.contains("[overflow-wrap:anywhere]")).toBe(true);
      const selectContent = longOption?.closest<HTMLElement>('[data-slot="select-content"]');
      expect(selectContent?.classList.contains("w-[var(--radix-select-trigger-width)]")).toBe(true);
      expect(selectContent?.classList.contains("max-w-[calc(100vw-2rem)]")).toBe(true);

      await act(async () => longOption?.click());
      expect(trigger?.textContent).toContain(longSessionTitle);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
