import { describe, expect, it } from "vitest";
import {
  createAcpSessionConfigViewModel,
  findAcpConfigOption,
} from "@/lib/acp-session-config";
import type { AcpSessionConfigVm } from "@/types";

function baseConfig(): AcpSessionConfigVm {
  return {
    currentModelId: "gpt-5",
    currentModelName: "GPT-5",
    currentModeId: "ask",
    currentModeName: "Ask",
    models: {
      availableModels: [
        { modelId: "gpt-5", name: "GPT-5", description: "primary" },
        { modelId: "gpt-5-mini", name: "GPT-5 mini" },
      ],
    },
    modes: {
      availableModes: [
        { id: "ask", name: "Ask" },
        { id: "full_access", name: "Full access" },
      ],
    },
    configOptions: [
      {
        category: "model",
        options: [{ value: "fallback", name: "Fallback model" }],
      },
    ],
  };
}

describe("ACP session config view model", () => {
  it("keeps the same signature for stream-only session changes", () => {
    const first = createAcpSessionConfigViewModel(baseConfig());
    const second = createAcpSessionConfigViewModel({
      ...baseConfig(),
    });

    expect(second.signature).toBe(first.signature);
  });

  it("changes the signature when visible config changes", () => {
    const first = createAcpSessionConfigViewModel(baseConfig());
    const second = createAcpSessionConfigViewModel({
      ...baseConfig(),
      currentModelId: "gpt-5-mini",
      currentModelName: "GPT-5 mini",
    });

    expect(second.signature).not.toBe(first.signature);
  });

  it("normalizes grouped model and permission mode options", () => {
    const viewModel = createAcpSessionConfigViewModel(baseConfig());

    expect(viewModel.availableModels.map((option) => option.id)).toEqual([
      "gpt-5",
      "gpt-5-mini",
    ]);
    expect(viewModel.availablePermissionModes.map((option) => option.id)).toEqual([
      "ask",
      "full_access",
    ]);
  });

  it("falls back to configOptions and preserves unknown ids", () => {
    const config = baseConfig();

    expect(
      findAcpConfigOption(config.models, config.configOptions, "model", "gpt-5"),
    ).toMatchObject({ id: "gpt-5", name: "GPT-5" });
    expect(
      findAcpConfigOption(null, config.configOptions, "model", "fallback"),
    ).toMatchObject({ id: "fallback", name: "Fallback model" });
    expect(
      findAcpConfigOption(null, null, "mode", "unknown-mode"),
    ).toMatchObject({ id: "unknown-mode", name: "unknown-mode" });
  });
});
