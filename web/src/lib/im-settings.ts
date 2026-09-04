import type {
  ImChannelKind,
  ImChannelSettingsVm,
  ImChannelSnapshotVm,
  ImNotificationPreferencesVm,
  ImSettingsVm,
  SaveImNotificationPreferencesInputVm,
  SetImChannelEnabledInputVm,
} from '@/types';

export interface ImNotificationDraft {
  notifications: ImNotificationPreferencesVm;
}

export type ImChannelDisplayStatus =
  | 'notConfigured'
  | 'waitingBinding'
  | 'connecting'
  | 'reconnecting'
  | 'ready'
  | 'paused'
  | 'reauthorize'
  | 'conflict';

export type ImChannelRecoveryAction = 'none' | 'reauthorize' | 'reconnect';

export interface ImChannelDisplayModel {
  status: ImChannelDisplayStatus;
  tone: 'default' | 'muted' | 'warning' | 'destructive';
  recoveryAction: ImChannelRecoveryAction;
  showNotifications: boolean;
  canToggle: boolean;
}

const reauthorizationErrors = new Set([
  'IM_AUTHENTICATION_REQUIRED',
  'IM_CREDENTIAL_INVALID',
  'IM_CREDENTIAL_UNAVAILABLE',
]);

export function imChannelDisplayModel(channel: ImChannelSettingsVm): ImChannelDisplayModel {
  if (!channel.credentialConfigured) {
    return { status: 'notConfigured', tone: 'muted', recoveryAction: 'none', showNotifications: false, canToggle: false };
  }
  if (!channel.enabled) {
    return { status: 'paused', tone: 'muted', recoveryAction: 'none', showNotifications: true, canToggle: true };
  }
  const errorCode = channel.connection?.lastErrorCode;
  if (errorCode && reauthorizationErrors.has(errorCode)) {
    return { status: 'reauthorize', tone: 'destructive', recoveryAction: 'reauthorize', showNotifications: true, canToggle: true };
  }
  if (errorCode === 'IM_CONNECTION_CONFLICT') {
    return { status: 'conflict', tone: 'destructive', recoveryAction: 'reconnect', showNotifications: true, canToggle: true };
  }
  if (errorCode === 'IM_NETWORK_UNAVAILABLE' || errorCode === 'IM_RATE_LIMITED') {
    return { status: 'reconnecting', tone: 'warning', recoveryAction: 'none', showNotifications: true, canToggle: true };
  }
  if (channel.connection?.state === 'connecting') {
    return { status: 'connecting', tone: 'warning', recoveryAction: 'none', showNotifications: true, canToggle: true };
  }
  const durableBinding = channel.binding;
  const currentBinding = channel.connection?.binding;
  if (channel.connection?.state === 'connected' && durableBinding && currentBinding
      && durableBinding.destinationId === currentBinding.destinationId
      && durableBinding.conversationId === currentBinding.conversationId
      && durableBinding.authorizedActorId === currentBinding.actorId) {
    return { status: 'ready', tone: 'default', recoveryAction: 'none', showNotifications: true, canToggle: true };
  }
  return { status: 'waitingBinding', tone: 'warning', recoveryAction: 'none', showNotifications: true, canToggle: true };
}

export function notificationDraftFrom(channel: ImChannelSettingsVm): ImNotificationDraft {
  return { notifications: { ...channel.notifications } };
}

export function notificationPreferencesEqual(
  left: ImNotificationPreferencesVm,
  right: ImNotificationPreferencesVm,
) {
  return (Object.keys(left) as Array<keyof ImNotificationPreferencesVm>)
    .every((key) => left[key] === right[key]);
}

export function buildSetImChannelEnabledInput(
  kind: ImChannelKind,
  enabled: boolean,
): SetImChannelEnabledInputVm {
  return { kind, enabled };
}

export function buildSaveImNotificationPreferencesInput(
  kind: ImChannelKind,
  notifications: ImNotificationPreferencesVm,
): SaveImNotificationPreferencesInputVm {
  return { kind, notifications: { ...notifications } };
}

export function mergeImChannelSnapshot(settings: ImSettingsVm, snapshot: ImChannelSnapshotVm): ImSettingsVm {
  return {
    channels: settings.channels.map((channel) => {
      if (channel.kind !== snapshot.kind) return channel;
      if (channel.connection && snapshot.generation < channel.connection.generation) return channel;
      const binding = snapshot.binding ? {
        destinationId: snapshot.binding.destinationId,
        conversationId: snapshot.binding.conversationId,
        authorizedActorId: snapshot.binding.actorId,
        displayName: channel.binding?.authorizedActorId === snapshot.binding.actorId
          ? channel.binding.displayName
          : snapshot.binding.actorId,
      } : null;
      return { ...channel, enabled: snapshot.enabled, binding, connection: snapshot };
    }),
  };
}

export function mergeImSettings(current: ImSettingsVm, incoming: ImSettingsVm): ImSettingsVm {
  return {
    channels: incoming.channels.map((channel) => {
      const existing = current.channels.find((candidate) => candidate.kind === channel.kind);
      if (!existing?.connection || !channel.connection
          || channel.connection.generation >= existing.connection.generation) {
        return channel;
      }
      return { ...channel, enabled: existing.enabled, connection: existing.connection };
    }),
  };
}
