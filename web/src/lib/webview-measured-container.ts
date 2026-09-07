export interface MeasuredContainerEnvironment {
  readonly ResizeObserver: typeof ResizeObserver;
  requestAnimationFrame(callback: FrameRequestCallback): number;
  cancelAnimationFrame(handle: number): void;
}

const CONTAINER_BREAKPOINTS = [
  ['xs', 320],
  ['sm', 384],
  ['md', 448],
  ['lg', 512],
  ['xl', 576],
  ['2xl', 672],
  ['3xl', 768],
  ['4xl', 896],
  ['5xl', 1024],
  ['6xl', 1152],
] as const;

export function measuredContainerTierTokens(width: number) {
  return CONTAINER_BREAKPOINTS
    .filter(([, minimumWidth]) => width >= minimumWidth)
    .map(([name]) => name)
    .join(' ');
}

export function observeMeasuredWebviewContainer(
  element: HTMLElement,
  name: string,
  environment: MeasuredContainerEnvironment = window,
) {
  element.dataset.webviewContainer = name;
  let frame = 0;
  let pendingWidth = element.getBoundingClientRect().width;
  let publishedTokens: string | null = null;

  const publish = () => {
    frame = 0;
    const nextTokens = measuredContainerTierTokens(pendingWidth);
    if (nextTokens === publishedTokens) return;
    publishedTokens = nextTokens;
    element.dataset.webviewContainerTiers = nextTokens;
  };
  const schedule = (width: number) => {
    pendingWidth = width;
    if (!frame) frame = environment.requestAnimationFrame(publish);
  };

  const observer = new environment.ResizeObserver((entries) => {
    const entry = entries.find(({ target }) => target === element);
    if (entry) schedule(entry.contentRect.width);
  });
  observer.observe(element);
  schedule(pendingWidth);

  return () => {
    observer.disconnect();
    if (frame) environment.cancelAnimationFrame(frame);
    delete element.dataset.webviewContainer;
    delete element.dataset.webviewContainerTiers;
  };
}
