export function resolveConversationEventSelectedSessionKey(args: {
  currentSelectedKey?: string | null;
  incomingSessionKey: string;
  autoFollow: boolean;
}) {
  const { currentSelectedKey, incomingSessionKey, autoFollow } = args;
  if (!currentSelectedKey || autoFollow) return incomingSessionKey;
  return currentSelectedKey;
}

export function shouldEnableConversationAutoFollow(
  sessionTone: string | null | undefined,
  atBottom: boolean,
) {
  return sessionTone === 'running' && atBottom;
}
