(() => {
  const navigationKeys = new Set(['ArrowLeft', 'ArrowRight', 'PageUp', 'PageDown', 'Home', 'End', ' ', 'Escape', 'f', 'F', 'p', 'P']);
  document.addEventListener('keydown', event => {
    if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.target.closest('input, textarea, select, [contenteditable]')) return;
    if (event.key === 'Escape') {
      const expanded = document.querySelector('details[open]');
      if (expanded) {
        expanded.open = false;
        expanded.querySelector('summary')?.focus();
        event.preventDefault();
        return;
      }
    }
    if (event.key === ' ' && event.target.closest('button, a, summary')) return;
    if (window.parent === window || !navigationKeys.has(event.key)) return;
    event.preventDefault();
    window.parent.postMessage({ type: 'gold-band-deck:navigate', key: event.key }, '*');
  });
})();
