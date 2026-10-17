(async () => {
  await document.fonts.ready;
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const overflow = [];
  const smallText = [];
  let node;
  while ((node = walker.nextNode())) {
    const text = node.textContent.trim();
    const element = node.parentElement;
    if (!text || !element || element.closest('script, style, [aria-hidden="true"]')) continue;
    const style = getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden') continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    for (const box of range.getClientRects()) {
      if (box.width && box.height && (box.left < -1 || box.top < -1 || box.right > 1921 || box.bottom > 1081)) {
        overflow.push({ text: text.slice(0, 65), bounds: [box.left, box.top, box.right, box.bottom] });
      }
    }
    if (parseFloat(style.fontSize) < 18) smallText.push({ text: text.slice(0, 50), size: style.fontSize });
  }
  return {
    title: document.title,
    dimensions: [document.body.scrollWidth, document.body.scrollHeight],
    fontsLoaded: document.fonts.check('28px "GB MiSans"') && document.fonts.check('28px "GB Inter"'),
    brokenImages: [...document.images].filter(image => !image.complete || !image.naturalWidth).map(image => image.src),
    textOutsideCanvas: overflow,
    textBelow18px: smallText,
  };
})()
