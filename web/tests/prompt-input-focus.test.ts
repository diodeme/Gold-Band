import { describe, expect, it, vi } from "vitest"

import { shouldFocusPromptInputTextarea } from "@/components/prompt-kit/prompt-input"

function targetInside(selectorPart: string | null) {
  return {
    closest: vi.fn((selector: string) => (
      selectorPart && selector.includes(selectorPart) ? {} : null
    )),
  }
}

describe("PromptInput click-to-focus", () => {
  it("focuses the textarea for blank or non-interactive content", () => {
    const target = targetInside(null)

    expect(shouldFocusPromptInputTextarea(target)).toBe(true)
    expect(target.closest).toHaveBeenCalledOnce()
  })

  it.each([
    ["button", "button"],
    ["combobox trigger", '[role="combobox"]'],
    ["menu item", '[role="menuitem"]'],
    ["checkbox menu item", '[role="menuitemcheckbox"]'],
    ["radio menu item", '[role="menuitemradio"]'],
    ["textarea", "textarea"],
  ])("keeps focus on an interactive %s", (_label, selectorPart) => {
    expect(shouldFocusPromptInputTextarea(targetInside(selectorPart))).toBe(false)
  })

  it("safely allows focus for missing or non-Element targets", () => {
    expect(shouldFocusPromptInputTextarea(null)).toBe(true)
    expect(shouldFocusPromptInputTextarea({})).toBe(true)
  })
})
