import { describe, expect, it } from 'vitest';
import type { AppBootstrapVm } from '../src/types';
import {
  canRemoveRecentWorkspace,
  shouldAutoOpenWorkspacePicker,
  shouldRenderWorkspacePicker,
} from '../src/lib/workspace-picker-scope';

const bootstrap = (needsWorkspace: boolean): AppBootstrapVm => ({
  repoRoot: 'D:/Projects/code/ai/Gold-Band',
  recentWorkspaces: ['D:/Projects/code/ai/Gold-Band'],
  preferences: {
    theme: 'system',
    language: 'zh-cn',
    font: 'app-default',
    useLocalClaude: false,
    verboseLogging: false,
  },
  updaterSettings: {
    channel: 'default',
    builtInUrl: '',
    effectiveUrl: '',
    overrideUrl: null,
    pollIntervalMinutes: 240,
  },
  metricsSettings: {
    enabled: false,
    toggleLocked: false,
    metricsBaseUrl: null,
    heartbeatEndpoint: null,
    nodeMetricsEndpoint: null,
    apiKeySet: false,
  },
  updateStatus: {
    status: 'idle',
    checkedAt: null,
    update: null,
    error: null,
    background: false,
  },
  updateBadges: {
    settingsEntrySeenVersion: null,
    settingsAdvancedSeenVersion: null,
    announcementClosedVersion: null,
  },
  persistedAvailableUpdate: null,
  clientVersion: '0.0.0',
  platform: 'windows',
  windowChrome: { frameStyle: 'native-compositor' },
  appInfo: {
    channel: 'default',
    appName: 'Gold Band',
    appKey: 'gold-band',
    configDirName: '.gold-band',
  },
  appConfig: {
    acpSessionTitleRefreshEnabled: false,
    acpChatEventPageSize: 360,
  },
  needsWorkspace,
});

describe('workspace picker scope', () => {
  it('auto-opens only for workbench when a desktop workspace is required', () => {
    expect(shouldAutoOpenWorkspacePicker(bootstrap(true), 'workbench')).toBe(true);
    expect(shouldAutoOpenWorkspacePicker(bootstrap(true), 'conversation')).toBe(false);
    expect(shouldAutoOpenWorkspacePicker(bootstrap(false), 'workbench')).toBe(false);
  });

  it('renders the workspace picker only in workbench mode', () => {
    expect(shouldRenderWorkspacePicker('workbench', true)).toBe(true);
    expect(shouldRenderWorkspacePicker('conversation', true)).toBe(false);
    expect(shouldRenderWorkspacePicker('workbench', false)).toBe(false);
  });

  it('allows removing only non-current recent workspaces when more than one exists', () => {
    expect(canRemoveRecentWorkspace(1, 'D:/Projects/code/ai/Gold-Band', 'D:/Projects/code/ai/Gold-Band')).toBe(false);
    expect(canRemoveRecentWorkspace(2, 'D:/Projects/code/ai/Gold-Band', 'D:/Projects/code/ai/Gold-Band')).toBe(false);
    expect(canRemoveRecentWorkspace(2, 'D:/Projects/Other', 'D:/Projects/code/ai/Gold-Band')).toBe(true);
  });
});
