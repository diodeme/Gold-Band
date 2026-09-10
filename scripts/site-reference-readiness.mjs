export function referenceAnimationsSettled(document, viewportHeight) {
  return document.getAnimations().every(animation => {
    const target = animation.effect?.target;
    if (!target?.getBoundingClientRect) return true;
    const rect = target.getBoundingClientRect();
    const visible = rect.width > 0 && rect.height > 0 && rect.top < viewportHeight && rect.bottom > 0;
    return !visible || !Number.isFinite(animation.effect.getComputedTiming().endTime)
      || animation.playState === 'finished' || animation.playState === 'idle';
  });
}
