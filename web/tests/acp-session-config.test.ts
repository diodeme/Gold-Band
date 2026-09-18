import { describe, expect, it } from "vitest";
import {
  acpProviderConfigCatalog,
  createAcpSessionConfigViewModel,
  findAcpConfigOption,
  mergeLiveAcpSessionConfig,
} from "@/lib/acp-session-config";
import type { AcpSessionConfigVm, AgentRegistryVm } from "@/types";

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
  const doctorCatalog = {
    observedAt: "200Z",
    models: [{ id: "gpt-new", name: "GPT New" }],
    modes: [{ id: "full", name: "Full access" }],
    configOptions: [{
      id: "effort",
      category: "thought_level",
      name: "Thought level",
      currentValue: "doctor-default",
      options: [
        { value: "low", name: "Low" },
        { value: "high", name: "High" },
      ],
    }],
  };

  it("uses a newer successful Doctor catalog without replacing Session current values", () => {
    const viewModel = createAcpSessionConfigViewModel({
      catalogObservedAt: "199Z",
      modelOverrideId: "gpt-new",
      currentModelId: "gpt-old",
      currentModelName: "GPT Old",
      configOptionOverrides: { effort: "high" },
      configOptions: [{
        id: "effort",
        category: "thought_level",
        type: "select",
        currentValue: "low",
        options: [{ value: "low", name: "Low" }],
      }],
    }, doctorCatalog);

    expect(viewModel.availableModels.map((option) => option.id)).toEqual(["gpt-new"]);
    expect(viewModel.currentModelId).toBe("gpt-old");
    expect(viewModel.thoughtLevel?.currentValue).toBe("low");
    expect(viewModel.thoughtLevel?.overrideValue).toBe("high");
  });

  it("keeps a same-time or newer Session catalog authoritative", () => {
    const session = {
      ...baseConfig(),
      catalogObservedAt: "200Z",
    };
    expect(
      createAcpSessionConfigViewModel(session, doctorCatalog).availableModels.map((option) => option.id),
    ).toEqual(["gpt-5", "fallback"]);
    expect(
      createAcpSessionConfigViewModel({ ...session, catalogObservedAt: "201Z" }, doctorCatalog)
        .availableModels.map((option) => option.id),
    ).toEqual(["gpt-5", "fallback"]);
  });

  it("keeps an unavailable override visible but disabled beside the latest choices", () => {
    const viewModel = createAcpSessionConfigViewModel({
      catalogObservedAt: "199Z",
      modelOverrideId: "gpt-old",
      currentModelId: "gpt-old",
      currentModelName: "GPT Old",
    }, doctorCatalog);

    expect(viewModel.availableModels).toEqual([
      { id: "gpt-old", name: "GPT Old", description: null, available: false },
      { id: "gpt-new", name: "GPT New", description: null },
    ]);
  });

  it("keeps an unavailable generic override visible but disabled", () => {
    const viewModel = createAcpSessionConfigViewModel({
      catalogObservedAt: "199Z",
      configOptionOverrides: { effort: "max" },
      configOptions: [{
        id: "effort",
        category: "thought_level",
        type: "select",
        currentValue: "low",
        options: [{ value: "low", name: "Low" }, { value: "max", name: "Max" }],
      }],
    }, doctorCatalog);

    expect(viewModel.thoughtLevel?.options).toEqual([
      { id: "max", name: "Max", description: null, available: false },
      { id: "low", name: "Low", description: null },
      { id: "high", name: "High", description: null },
    ]);
  });

  it("does not expose a failed Doctor observation as a catalog", () => {
    const registry = {
      agents: [{
        agentType: "current",
        diagnostic: { status: "error", available: false, checkedAt: "300Z" },
        supportedModels: [{ id: "gpt-new", name: "GPT New" }],
      }],
      catalog: [],
    } as AgentRegistryVm;

    expect(acpProviderConfigCatalog(registry, "current")).toBeNull();
  });

  it("does not change the current provider signature for an unrelated Doctor update", () => {
    const registry = {
      agents: [{
        agentType: "current",
        diagnostic: { status: "healthy", available: true, checkedAt: "200Z" },
        supportedModels: [{ id: "gpt-new", name: "GPT New" }],
      }, {
        agentType: "other",
        diagnostic: { status: "healthy", available: true, checkedAt: "300Z" },
        supportedModels: [{ id: "other-new", name: "Other New" }],
      }],
      catalog: [],
    } as AgentRegistryVm;
    const session = { catalogObservedAt: "199Z" };
    const first = createAcpSessionConfigViewModel(
      session,
      acpProviderConfigCatalog(registry, "current"),
    );
    const updatedRegistry = {
      ...registry,
      agents: registry.agents.map((agent) => agent.agentType === "other"
        ? { ...agent, diagnostic: { ...agent.diagnostic!, checkedAt: "301Z" } }
        : agent),
    };
    const second = createAcpSessionConfigViewModel(
      session,
      acpProviderConfigCatalog(updatedRegistry, "current"),
    );

    expect(second.signature).toBe(first.signature);
  });

  it("defaults Auto Accept off and keeps it out of native permission modes", () => {
    const viewModel = createAcpSessionConfigViewModel(baseConfig());

    expect(viewModel.autoAccept).toBe(false);
    expect(viewModel.availablePermissionModes.map((option) => option.id)).toEqual([
      "ask",
      "full_access",
    ]);
  });

  it("projects session Auto Accept without treating it as a permission override", () => {
    const viewModel = createAcpSessionConfigViewModel({
      ...baseConfig(),
      autoAccept: true,
    });

    expect(viewModel.autoAccept).toBe(true);
    expect(viewModel.permissionModeOverrideId).toBe("ask");
    expect(viewModel.availablePermissionModes.map((option) => option.id)).not.toContain("autoAccept");
  });

  it("changes the signature when Auto Accept changes", () => {
    const first = createAcpSessionConfigViewModel(baseConfig());
    const second = createAcpSessionConfigViewModel({
      ...baseConfig(),
      autoAccept: true,
    });

    expect(second.signature).not.toBe(first.signature);
  });

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
    expect(viewModel.canSelectUnspecifiedModel).toBe(false);
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
    expect(viewModel.canSelectUnspecifiedPermissionMode).toBe(false);
  });

  it("prefers generic config options over conflicting legacy grouped options", () => {
    const viewModel = createAcpSessionConfigViewModel(baseConfig());

    expect(viewModel.availableModels).toEqual([
      { id: "gpt-5", name: "GPT-5", description: null, available: false },
      { id: "fallback", name: "Fallback model", description: null },
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

  it("uses the pure model catalog when legacy models expand reasoning variants", () => {
    const viewModel = createAcpSessionConfigViewModel({
      currentModelId: "gpt-5.6-sol",
      currentModelName: "GPT-5.6-Sol",
      models: {
        currentModelId: "gpt-5.6-sol[max]",
        availableModels: [
          { modelId: "gpt-5.6-sol[low]", name: "GPT-5.6-Sol (low)" },
          { modelId: "gpt-5.6-sol[max]", name: "GPT-5.6-Sol (max)" },
        ],
      },
      configOptions: [{
        id: "model",
        category: "model",
        type: "select",
        currentValue: "gpt-5.6-sol",
        options: [
          { value: "gpt-5.6-sol", name: "GPT-5.6-Sol" },
          { value: "gpt-5.6-terra", name: "GPT-5.6-Terra" },
        ],
      }],
    });

    expect(viewModel.availableModels).toEqual([
      { id: "gpt-5.6-sol", name: "GPT-5.6-Sol", description: null },
      { id: "gpt-5.6-terra", name: "GPT-5.6-Terra", description: null },
    ]);
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
      canSelectUnspecified: false,
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
    expect(viewModel.thoughtLevel?.canSelectUnspecified).toBe(true);
  });

  it("exposes Fast as a model_config composite option without treating it as thought level", () => {
    const viewModel = createAcpSessionConfigViewModel({
      configOptionOverrides: { fast: "true" },
      configOptions: [
        {
          id: "model",
          category: "model",
          type: "select",
          currentValue: "composer-2.5",
          options: [{ value: "composer-2.5", name: "Composer 2.5" }],
        },
        {
          id: "fast",
          category: "model_config",
          type: "select",
          name: "Fast",
          currentValue: "false",
          options: [
            { value: "false", name: "Off" },
            { value: "true", name: "On" },
          ],
        },
        {
          id: "effort",
          category: "thought_level",
          type: "select",
          options: [{ value: "low", name: "Low" }, { value: "high", name: "High" }],
        },
      ],
    });

    expect(viewModel.thoughtLevel?.id).toBe("effort");
    expect(viewModel.modelBoundOptions.map((group) => group.id)).toEqual(["effort", "fast"]);
    expect(viewModel.modelBoundOptions[1]).toMatchObject({
      id: "fast",
      category: "model_config",
      overrideValue: "true",
    });
  });

  it("keeps thought and Fast after the user selects a model that is not the catalog current model", () => {
    const viewModel = createAcpSessionConfigViewModel({
      modelOverrideId: "gpt-5.6-terra",
      configOptionOverrides: { effort: "high", fast: "true" },
      configOptions: [
        {
          id: "model",
          category: "model",
          type: "select",
          currentValue: "gpt-5.6-sol",
          options: [
            { value: "gpt-5.6-sol", name: "5.6 Sol" },
            { value: "gpt-5.6-terra", name: "5.6 Terra" },
          ],
        },
        {
          id: "fast",
          category: "model_config",
          type: "select",
          name: "Fast",
          options: [{ value: "false", name: "Off" }, { value: "true", name: "On" }],
        },
        {
          id: "effort",
          category: "thought_level",
          type: "select",
          name: "Effort",
          options: [{ value: "low", name: "Low" }, { value: "high", name: "High" }],
        },
      ],
    });

    expect(viewModel.modelBoundOptions.map((group) => group.id)).toEqual(["effort", "fast"]);
    expect(viewModel.thoughtLevel?.id).toBe("effort");
  });

  it("keeps session thought options when a newer Doctor catalog belongs to a different model", () => {
    const viewModel = createAcpSessionConfigViewModel({
      catalogObservedAt: "199Z",
      modelOverrideId: "grok-4.6",
      currentModelId: "grok-4.6",
      currentModelName: "Cursor Grok 4.6",
      configOptionOverrides: { effort: "extra-high" },
      configOptions: [
        {
          id: "model",
          category: "model",
          type: "select",
          currentValue: "grok-4.6",
          options: [
            { value: "composer-2.5", name: "Composer 2.5" },
            { value: "grok-4.6", name: "Cursor Grok 4.6" },
          ],
        },
        {
          id: "effort",
          category: "thought_level",
          type: "select",
          currentValue: "high",
          options: [
            { value: "low", name: "Low" },
            { value: "high", name: "High" },
            { value: "extra-high", name: "Extra High" },
          ],
        },
      ],
    }, {
      observedAt: "200Z",
      models: [
        { id: "composer-2.5", name: "Composer 2.5" },
        { id: "grok-4.6", name: "Cursor Grok 4.6" },
      ],
      modes: [],
      configOptions: [
        {
          id: "model",
          category: "model",
          currentValue: "composer-2.5",
          options: [
            { value: "composer-2.5", name: "Composer 2.5" },
            { value: "grok-4.6", name: "Cursor Grok 4.6" },
          ],
        },
        {
          id: "fast",
          category: "model_config",
          name: "Fast",
          currentValue: "true",
          options: [{ value: "false", name: "Off" }, { value: "true", name: "On" }],
        },
      ],
    });

    expect(viewModel.thoughtLevel?.options.map((option) => option.id)).toEqual(["low", "high", "extra-high"]);
    expect(viewModel.modelBoundOptions.map((group) => group.id)).toEqual(["effort"]);
    expect(viewModel.modelBoundOptions.some((group) => group.id === "fast")).toBe(false);
  });

  it("keeps session thought when a newer Doctor catalog has Fast but no catalog model currentValue", () => {
    const viewModel = createAcpSessionConfigViewModel({
      catalogObservedAt: "199Z",
      modelOverrideId: "grok-4.6",
      configOptionOverrides: { effort: "high" },
      configOptions: [
        {
          id: "model",
          category: "model",
          type: "select",
          currentValue: "grok-4.6",
          options: [{ value: "grok-4.6", name: "Cursor Grok 4.6" }],
        },
        {
          id: "effort",
          category: "thought_level",
          type: "select",
          currentValue: "high",
          options: [{ value: "low", name: "Low" }, { value: "high", name: "High" }],
        },
      ],
    }, {
      observedAt: "200Z",
      models: [{ id: "grok-4.6", name: "Cursor Grok 4.6" }],
      modes: [],
      configOptions: [{
        id: "fast",
        category: "model_config",
        name: "Fast",
        currentValue: "true",
        options: [{ value: "false", name: "Off" }, { value: "true", name: "On" }],
      }],
    });

    expect(viewModel.thoughtLevel?.id).toBe("effort");
    expect(viewModel.modelBoundOptions.map((group) => group.id)).toEqual(["effort"]);
  });

  it("projects High onto a renamed thought_level option and keeps Context after thought", () => {
    const viewModel = createAcpSessionConfigViewModel({
      modelOverrideId: "gpt-5.6-luna",
      configOptionOverrides: { effort: "high", fast: "false" },
      configOptions: [
        {
          id: "model",
          category: "model",
          type: "select",
          currentValue: "gpt-5.6-luna",
          options: [
            { value: "grok-4.6", name: "Cursor Grok 4.6" },
            { value: "gpt-5.6-luna", name: "GPT-5.6 Luna" },
          ],
        },
        {
          id: "context",
          category: "model_config",
          type: "select",
          name: "Context",
          options: [{ value: "272k", name: "272K" }, { value: "1m", name: "1M" }],
        },
        {
          id: "reasoning",
          category: "thought_level",
          type: "select",
          name: "Reasoning",
          options: [
            { value: "medium", name: "Medium" },
            { value: "high", name: "High" },
          ],
        },
        {
          id: "fast",
          category: "model_config",
          type: "select",
          name: "Fast",
          options: [{ value: "false", name: "Off" }, { value: "true", name: "Fast" }],
        },
      ],
    });

    expect(viewModel.thoughtLevel).toMatchObject({
      id: "reasoning",
      overrideValue: "high",
    });
    expect(viewModel.modelBoundOptions.map((group) => group.id)).toEqual([
      "reasoning",
      "context",
      "fast",
    ]);
  });

  it("applies a live catalog without rebuilding timeline state", () => {
    const current = {
      ...baseConfig(),
      catalogObservedAt: "100Z",
      configOptionOverrides: { effort: "high" },
      configOptions: [{
        id: "effort",
        category: "thought_level",
        type: "select",
        options: [{ value: "high", name: "High" }],
      }],
    };
    const incoming = {
      catalogObservedAt: "200Z",
      configOptions: [{
        id: "reasoning",
        category: "thought_level",
        type: "select",
        options: [{ value: "high", name: "High" }],
      }],
    };
    const merged = mergeLiveAcpSessionConfig(current, incoming, true);
    expect(merged?.catalogObservedAt).toBe("200Z");
    expect(merged?.configOptionOverrides).toEqual({ effort: "high" });
    expect(merged?.configOptions).toEqual(incoming.configOptions);
    expect(mergeLiveAcpSessionConfig(current, incoming, false)?.configOptionOverrides).toBeUndefined();
  });
});
