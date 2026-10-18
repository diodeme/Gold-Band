const svgMarkup = new Map<string, Promise<string>>();

function svgAddress(img: HTMLImageElement) {
  // rrweb keeps the SVG path here and puts a 16px PNG into src.
  const src = img.getAttribute('rrweb-original-src') || img.getAttribute('src') || '';
  return src.includes('.svg') ? src : '';
}

export async function sharpenIconsIn(doc: Document, load: (src: string) => Promise<string>) {
  const images = [...doc.querySelectorAll('img')].filter((img) => svgAddress(img) && img.dataset.sharpIcon !== '1');
  await Promise.all(images.map(async (img) => {
    const src = svgAddress(img);
    img.dataset.sharpIcon = '1';
    const markup = await load(src).catch(() => '');
    if (!markup || !img.isConnected) {
      if (!markup) delete img.dataset.sharpIcon;
      return;
    }
    const parsed = new DOMParser().parseFromString(markup, 'image/svg+xml');
    const svg = parsed.documentElement;
    if (svg.tagName.toLowerCase() !== 'svg') {
      delete img.dataset.sharpIcon;
      return;
    }
    const box = img.ownerDocument.defaultView?.getComputedStyle(img);
    svg.removeAttribute('width');
    svg.removeAttribute('height');
    svg.setAttribute('preserveAspectRatio', 'xMidYMid meet');
    svg.setAttribute('class', img.getAttribute('class') ?? '');
    // Keep the img's used box. Percentage size lets a 1254px logo intrinsic
    // expand the slot and the camera then crops inside that shape.
    if (box?.width && box.width !== 'auto') svg.style.setProperty('width', box.width);
    if (box?.height && box.height !== 'auto') svg.style.setProperty('height', box.height);
    svg.style.setProperty('color', 'inherit');
    svg.style.setProperty('transform', 'none', 'important');
    svg.style.setProperty('filter', 'none', 'important');
    img.replaceWith(svg);
  }));
}

export function watchReplayIcons(iframe: HTMLIFrameElement) {
  const doc = iframe.contentDocument;
  if (!doc || doc.documentElement.dataset.sharpIcons === '1') return;
  doc.documentElement.dataset.sharpIcons = '1';
  const load = (src: string) => {
    const pending = svgMarkup.get(src) ?? fetch(src).then((response) => {
      if (!response.ok) throw new Error(`Icon ${response.status}`);
      return response.text();
    });
    svgMarkup.set(src, pending);
    return pending;
  };
  const scan = () => { void sharpenIconsIn(doc, load); };
  scan();
  const observer = new MutationObserver(scan);
  observer.observe(doc.documentElement, { childList: true, subtree: true });
}