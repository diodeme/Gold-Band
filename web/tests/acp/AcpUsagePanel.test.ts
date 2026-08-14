import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import {
  AcpUsagePanel,
  contextUsagePercentage,
  contextUsageTone,
} from "../../src/components/acp/AcpUsagePanel";
import { formatTokenCount } from "../../src/lib/format-token";

vi.mock("@/components/ui/tooltip", async () => {
  const React = await import("react");
  return {
    Tooltip: ({ children }: { children: React.ReactNode }) => React.createElement(React.Fragment, null, children),
    TooltipTrigger: ({ children }: { children: React.ReactNode }) => React.createElement(React.Fragment, null, children),
    TooltipContent: ({ children }: { children: React.ReactNode }) => React.createElement("div", { "data-tooltip-content": "true" }, children),
  };
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("AcpUsagePanel", () => {
  it("keeps only elapsed time and the context gauge in the session information bar", () => {
    const html = renderToStaticMarkup(
      createElement(AcpUsagePanel, {
        usage: {
          used: 32000,
          size: 1_000_000,
          inputTokens: 30600,
          outputTokens: 1400,
          cachedReadTokens: 0,
          cachedWriteTokens: 120,
          totalTokens: 32000,
        },
        sessionSeconds: 141,
      }),
    );

    expect(html).toContain('data-acp-session-info="true"');
    expect(html).toContain("acp.timingSession");
    expect(html).toContain("2m 21s");
    expect(html).toContain("tabular-nums");
    expect(html).not.toContain("font-mono");
    expect(html).toContain('data-context-usage-gauge="true"');
    expect(html).toContain("3%");
    expect(html).toContain("--context-usage-percent:3%");
    expect(html).not.toContain("animate-spin");
    expect(html).not.toContain("acp.usagePanel.tokenUsage");
  });

  it("renders the active session status before timing and context information", () => {
    const html = renderToStaticMarkup(
      createElement(AcpUsagePanel, {
        usage: { used: 30_000, size: 100_000 },
        processingLabel: "Agent 调起中",
        sessionSeconds: 12,
      }),
    );

    expect(html).toContain('data-acp-processing-spinner="true"');
    expect(html.indexOf("Agent 调起中")).toBeLessThan(html.indexOf("acp.timingSession"));
    expect(html.indexOf("acp.timingSession")).toBeLessThan(html.indexOf("acp.usagePanel.contextWindow"));
  });

  it("shows context occupancy followed by one row per reported token counter", () => {
    const html = renderToStaticMarkup(
      createElement(AcpUsagePanel, {
        usage: {
          used: 1000,
          size: 100000,
          inputTokens: 800,
          outputTokens: 120,
          cachedReadTokens: 60,
          cachedWriteTokens: 20,
          totalTokens: 1000,
        },
        sessionSeconds: 12,
      }),
    );

    expect(html).toContain('data-tooltip-content="true"');
    expect(html).toContain("acp.usagePanel.occupied");
    expect(html).toContain("1.0K / 100.0K");
    const tooltipHtml = html.slice(html.indexOf('<div data-tooltip-content="true">'));
    expect(tooltipHtml).not.toContain("1%");
    expect(html).toContain("size-6");
    expect(html).not.toContain("size-8");
    expect(html).toMatch(/tracking-\[-0\.02em\][^>]*>1<\/span>/);
    expect(html).not.toContain("text-[5px]");
    expect(html).toContain('data-context-usage-tone="healthy"');
    expect(html).toContain("--context-usage-color:var(--gold-success)");
    expect(html).toContain("acp.usagePanel.input");
    expect(html).toContain("acp.usagePanel.output");
    expect(html).toContain("acp.usagePanel.cacheRead");
    expect(html).toContain("acp.usagePanel.cacheWrite");
    expect(html).toContain("acp.usagePanel.total");
  });

  it("does not present a transient zero as confirmed context usage", () => {
    const html = renderToStaticMarkup(
      createElement(AcpUsagePanel, {
        usage: { used: 0, size: 1_000_000 },
      }),
    );

    expect(html).toContain("-- / 1.0M");
    expect(html).not.toContain("0 / 1.0M");
    expect(html).toContain(">--<");
    expect(html).toContain('data-context-usage-tone="unknown"');
    expect(html).toContain("--context-usage-color:var(--muted-foreground)");
  });

  it("keeps the percent semantics accessible while rendering only the clamped number", () => {
    const html = renderToStaticMarkup(
      createElement(AcpUsagePanel, {
        usage: { used: 120_000, size: 100_000 },
      }),
    );

    expect(html).toContain("acp.usagePanel.occupied 120.0K / 100.0K 100%");
    expect(html).toContain(">100</span>");
    expect(html).not.toContain(">100%</span>");
  });
});

describe("contextUsagePercentage", () => {
  it("rounds the reported ratio and clamps over-capacity samples", () => {
    expect(contextUsagePercentage(1_000, 100_000)).toBe(1);
    expect(contextUsagePercentage(25_400, 258_400)).toBe(10);
    expect(contextUsagePercentage(120_000, 100_000)).toBe(100);
  });

  it("returns unknown when either confirmed gauge value is unavailable", () => {
    expect(contextUsagePercentage(null, 100_000)).toBeNull();
    expect(contextUsagePercentage(1_000, null)).toBeNull();
    expect(contextUsagePercentage(1_000, 0)).toBeNull();
  });
});

describe("contextUsageTone", () => {
  it.each([
    [0, "healthy"],
    [59, "healthy"],
    [60, "elevated"],
    [74, "elevated"],
    [75, "warning"],
    [89, "warning"],
    [90, "critical"],
    [100, "critical"],
    [null, "unknown"],
  ] as const)("maps %s percent to %s", (percentage, expected) => {
    expect(contextUsageTone(percentage)).toBe(expected);
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
