let channelAppNameValue = 'Gold Band';

export function setChannelAppName(appName: string | null | undefined) {
  const next = appName?.trim();
  channelAppNameValue = next && next.length > 0 ? next : 'Gold Band';
}

export function channelAppName() {
  return channelAppNameValue;
}
