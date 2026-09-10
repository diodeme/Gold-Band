export function connectDemoHost() {
  if (window.parent === window) return () => {};
  const origin = document.referrer ? new URL(document.referrer).origin : location.origin;
  const publish = () => window.parent.postMessage({ type: 'gold-band.demo.location', hash: location.hash }, origin);
  const receive = (event: MessageEvent) => {
    if (event.source !== window.parent || event.origin !== origin || event.data?.type !== 'gold-band.demo.navigate') return;
    const hash = event.data.hash;
    if (typeof hash !== 'string' || (hash && !hash.startsWith('#')) || hash === location.hash) return;
    location.replace(`${location.pathname}${location.search}${hash}`);
  };
  window.addEventListener('hashchange', publish);
  window.addEventListener('message', receive);
  publish();
  return () => { window.removeEventListener('hashchange', publish); window.removeEventListener('message', receive); };
}
