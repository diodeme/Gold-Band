import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AcpUsagePanel } from "../../src/components/acp/AcpUsagePanel";
import { formatTokenCount } from "../../src/lib/format-token";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("AcpUsagePanel", () => {
  it("keeps the compact runtime processing spinner visible without an active ACP session", () => {
    const html = renderToStaticMarkup(
      createElement(AcpUsagePanel, {
        usage: {
          used: 32000,
          size: 1_000_000,
          inputTokens: 30600,
          outputTokens: 1400,
          cachedReadTokens: 0,
          totalTokens: 32000,
        },
        isRunning: true,
        compact: true,
        processingLabel: "拉起下一节点中",
        stepSeconds: 40,
        sessionSeconds: 141,
      }),
    );

    expect(html).toContain("animate-spin");
    expect(html).toContain("拉起下一节点中");
    expect(html).toContain("acp.timingStep");
    expect(html).toContain("40s");
    expect(html).toContain("acp.timingSession");
    expect(html).toContain("2m 21s");
    expect(html).toContain("32.0K / 1.0M");
    expect(html).toContain("acp.usagePanel.input");
  });
});

describe("formatTokenCount", () => {
  it("formats 0 as raw number", () => {
    expect(formatTokenCount(0)).toBe("0");
  });

  it("formats numbers below 1K as raw number", () => {
    expect(formatTokenCount(842)).toBe("842");
    expect(formatTokenCount(999)).toBe("999");
  });

  it("formats 1K with .0 suffix", () => {
    expect(formatTokenCount(1000)).toBe("1.0K");
  });

  it("formats numbers in K range with one decimal", () => {
    expect(formatTokenCount(1234)).toBe("1.2K");
    expect(formatTokenCount(12000)).toBe("12.0K");
    expect(formatTokenCount(123456)).toBe("123.5K");
  });

  it("formats 1M with .0 suffix", () => {
    expect(formatTokenCount(1_000_000)).toBe("1.0M");
  });

  it("formats numbers in M range with one decimal", () => {
    expect(formatTokenCount(1_234_567)).toBe("1.2M");
    expect(formatTokenCount(12_345_678)).toBe("12.3M");
  });

  it("is a pure function (same input → same output)", () => {
    expect(formatTokenCount(42)).toBe(formatTokenCount(42));
  });
});
