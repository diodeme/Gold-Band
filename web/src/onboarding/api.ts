import { Channel } from '@tauri-apps/api/core';
import { invokeCommand, isTauriRuntime } from '@/api/shared';
import type { EnvironmentCheck, SetupApi, SetupEvent } from './types';

function channel(onEvent: (event: SetupEvent) => void) {
  const result = new Channel<SetupEvent>();
  result.onmessage = onEvent;
  return result;
}
export const desktopSetupApi: SetupApi = {
  status: () => invokeCommand('get_onboarding_status'),
  environment: (requestId, onChecked) => {
    const progress = new Channel<EnvironmentCheck>();
    progress.onmessage = (check) => onChecked?.(check);
    return invokeCommand('check_setup_environment', { requestId, onChecked: progress });
  },
  cancelEnvironment: (requestId) => invokeCommand('cancel_setup_environment', { requestId }),
  skipGit: () => invokeCommand('skip_setup_git'),
  deferSetup: () => invokeCommand('defer_setup'),
  start: (onEvent, after) => invokeCommand('start_setup_agent_detection', { onEvent: channel(onEvent), after: after ?? null }),
  cancel: (generation) => invokeCommand('cancel_setup_detection', { generation }),
  probeModel: (generation, agentId, modelId) => invokeCommand('probe_setup_model', { generation, agentId, modelId }),
  preview: (selection) => invokeCommand('preview_default_setup', { selection }),
  apply: (request) => invokeCommand('apply_default_setup', { request }),
  preferences: (language, appearance) => invokeCommand('save_setup_preferences', { language, appearance }),
};

export async function setupApi(): Promise<SetupApi | null> {
  if (isTauriRuntime()) return desktopSetupApi;
  // Browser fixtures are opt-in and absent from production desktop execution.
  if (import.meta.env.DEV && new URLSearchParams(location.search).has('onboardingFixture')) {
    return (await import('./browser-fixture')).browserSetupApi();
  }
  return null;
}
