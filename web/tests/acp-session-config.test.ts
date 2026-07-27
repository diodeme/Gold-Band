import { describe, expect, it } from "vitest";
import {
  createAcpSessionConfigViewModel,
  findAcpConfigOption,
} from "@/lib/acp-session-config";
import type { AcpSessionConfigVm } from "@/types";

function baseConfig(): AcpSessionConfigVm {
  return {
    modelOverrideId: "gpt-5",
    permissionModeOverrideId: "ask",
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

  it("keeps Gold Band unspecified separate from the Agent current model", () => {
    const viewModel = createAcpSessionConfigViewModel({
      modelOverrideId: null,
      currentModelId: "default",
      currentModelName: "Default (recommended)",
      models: {
        availableModels: [
          { modelId: "default", name: "Default (recommended)" },
          { modelId: "glm-5.2-hs", name: "GLM 5.2" },
        ],
      },
    });

    expect(viewModel.modelOverrideId).toBeNull();
    expect(viewModel.modelOverrideName).toBeNull();
    expect(viewModel.canSelectUnspecifiedModel).toBe(true);
    expect(viewModel.currentModelId).toBe("default");
    expect(viewModel.availableModels.map((option) => option.id)).toEqual([
      "default",
      "glm-5.2-hs",
    ]);
  });

  it("treats an Agent default option as explicit after the user selects it", () => {
    const viewModel = createAcpSessionConfigViewModel({
      modelOverrideId: "default",
      currentModelId: "default",
      currentModelName: "Default (recommended)",
      models: {
        availableModels: [
          { modelId: "default", name: "Default (recommended)" },
        ],
      },
    });

    expect(viewModel.modelOverrideId).toBe("default");
    expect(viewModel.modelOverrideName).toBe("Default (recommended)");
    expect(viewModel.canSelectUnspecifiedModel).toBe(true);
  });

  it("keeps Gold Band unspecified separate from the Agent current permission mode", () => {
    const viewModel = createAcpSessionConfigViewModel({
      permissionModeOverrideId: null,
      currentModeId: "default",
      currentModeName: "Default",
      modes: {
        availableModes: [
          { id: "default", name: "Default" },
          { id: "bypassPermissions", name: "Bypass Permissions" },
        ],
      },
    });

    expect(viewModel.permissionModeOverrideId).toBeNull();
    expect(viewModel.permissionModeOverrideName).toBeNull();
    expect(viewModel.canSelectUnspecifiedPermissionMode).toBe(true);
    expect(viewModel.currentModeId).toBe("default");
    expect(viewModel.availablePermissionModes.map((option) => option.id)).toEqual([
      "default",
      "bypassPermissions",
    ]);
  });

  it("treats an Agent default permission mode as explicit after the user selects it", () => {
    const viewModel = createAcpSessionConfigViewModel({
      permissionModeOverrideId: "default",
      currentModeId: "default",
      currentModeName: "Default",
      modes: {
        availableModes: [{ id: "default", name: "Default" }],
      },
    });

    expect(viewModel.permissionModeOverrideId).toBe("default");
    expect(viewModel.permissionModeOverrideName).toBe("Default");
    expect(viewModel.canSelectUnspecifiedPermissionMode).toBe(true);
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

  it("normalizes options even when current ids are absent", () => {
    const viewModel = createAcpSessionConfigViewModel({
      configOptions: [
        {
          category: "model",
          options: [{ value: "opus", name: "Opus" }],
        },
        {
          category: "mode",
          options: [{ value: "default", name: "Default" }],
        },
      ],
    });

    expect(viewModel.currentModelId).toBeNull();
    expect(viewModel.currentModeId).toBeNull();
    expect(viewModel.availableModels.map((option) => option.id)).toEqual(["opus"]);
    expect(viewModel.availablePermissionModes.map((option) => option.id)).toEqual(["default"]);
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

  it("keeps model-only Agents free of a thought-level control", () => {
    const viewModel = createAcpSessionConfigViewModel({
      models: {
        availableModels: [{ modelId: "gpt-5.6-sol", name: "GPT-5.6-Sol" }],
      },
    });

    expect(viewModel.availableModels).toHaveLength(1);
    expect(viewModel.thoughtLevel).toBeNull();
  });

  it("discovers thought level by category while preserving the Agent option id", () => {
    const viewModel = createAcpSessionConfigViewModel({
      configOptionOverrides: { reasoning_effort: "high" },
      configOptions: [
        {
          id: "reasoning_effort",
          category: "thought_level",
          type: "select",
          currentValue: "medium",
          options: [
            { value: "low", name: "Low" },
            { value: "high", name: "High" },
          ],
        },
      ],
    });

    expect(viewModel.thoughtLevel).toMatchObject({
      id: "reasoning_effort",
      category: "thought_level",
      currentValue: "medium",
      overrideValue: "high",
    });
    expect(viewModel.thoughtLevel?.options.map((option) => option.id)).toEqual(["low", "high"]);
  });

  it("keeps thought level unspecified even when the Agent has a current value", () => {
    const viewModel = createAcpSessionConfigViewModel({
      configOptions: [{
        id: "effort",
        category: "thought_level",
        type: "select",
        currentValue: "max",
        options: [{ value: "max", name: "Max" }],
      }],
    });

    expect(viewModel.thoughtLevel?.id).toBe("effort");
    expect(viewModel.thoughtLevel?.currentValue).toBe("max");
    expect(viewModel.thoughtLevel?.overrideValue).toBeNull();
  });
});
