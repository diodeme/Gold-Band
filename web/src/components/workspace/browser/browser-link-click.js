(() => {
  const isHttpUrl = (href) => {
    try {
      const url = new URL(href, location.href);
      return url.protocol === 'http:' || url.protocol === 'https:';
    } catch {
      return false;
    }
  };

  const clickElement = (event) => {
    const node = event.target;
    if (node instanceof Element) return node;
    if (node instanceof Node) return node.parentElement;
    return null;
  };

  document.addEventListener('click', (event) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const origin = clickElement(event);
    const link = origin && origin.closest('a[href]');
    if (!link || link.hasAttribute('download') || !isHttpUrl(link.href)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    location.assign(link.href);
  }, true);
})();
