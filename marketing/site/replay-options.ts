// rrweb's virtual DOM seek loses conversation nodes across recorded layout remounts.
export const REPLAY_OPTIONS = { skipInactive: false, mouseTail: false, showWarning: false, useVirtualDom: false, triggerFocus: false } as const;

export function settleReplayAnimations(document: Document | null | undefined) {
  for (const animation of document?.getAnimations() ?? []) {
    if (Number.isFinite(animation.effect?.getComputedTiming().endTime)) animation.finish();
  }
}
