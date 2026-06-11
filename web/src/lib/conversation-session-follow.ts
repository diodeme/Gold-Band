export function resolveConversationEventSelectedSessionKey(args: {
  currentSelectedKey?: string | null;
  incomingSessionKey: string;
  autoFollow: boolean;
}) {
  const { currentSelectedKey, incomingSessionKey, autoFollow } = args;
  if (!currentSelectedKey || autoFollow) return incomingSessionKey;
  return currentSelectedKey;
}

export function resolveConversationRefreshSelectedSessionKey(args: {
  autoFollow: boolean;
  pendingEventSessionKey?: string | null;
  currentSelectedKey?: string | null;
}) {
  const { autoFollow, pendingEventSessionKey, currentSelectedKey } = args;
  if (autoFollow && pendingEventSessionKey) return pendingEventSessionKey;
  return currentSelectedKey ?? pendingEventSessionKey ?? null;
}

export function shouldEnableConversationAutoFollow(
  isActiveSession: boolean,
  atBottom: boolean,
) {
  return isActiveSession && atBottom;
}
