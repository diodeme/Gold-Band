export const SUPPORT_DEVTOOLS_FEATURE = 'support-devtools';

export function parseChannelBuildArgs(args) {
  const [channel = 'default', ...rawOptions] = args;
  const options = {
    channel,
    isCritical: false,
    devtools: false,
  };

  for (const option of rawOptions) {
    if (option === 'critical' || option === '--critical') {
      options.isCritical = true;
      continue;
    }
    if (option === '--devtools') {
      options.devtools = true;
      continue;
    }
    throw new Error(`Unsupported build option: ${option}`);
  }

  return options;
}

export function channelBuildPlan(overlayPath, { devtools = false } = {}) {
  const tauriArgs = ['tauri', 'build', '--config', overlayPath];
  if (devtools) {
    tauriArgs.push('--features', SUPPORT_DEVTOOLS_FEATURE);
  }
  return {
    tauriArgs,
    tauriConfigBuildOptions: devtools ? { createUpdaterArtifacts: false } : undefined,
    shouldCollectReleaseArtifacts: !devtools,
  };
}
