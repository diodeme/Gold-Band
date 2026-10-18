// Runs in the recording page. Persist geometry, never rrweb mirror IDs or selectors.
export function measureWorkflowCamera(stepId, edges = [], nodes = []) {
  const nodeShot = stepId.endsWith('-node');
  const nodeId = stepId.startsWith('branch-') ? stepId.slice(7).replace(/-node$/, '') : null;
  const graphNode = nodeId ? document.querySelector('.react-flow__node[data-id="' + nodeId + '"]') : null;
  let roots;
  if (stepId === 'result-menu') roots = [...document.querySelectorAll('[role="listbox"]')];
  else if (stepId === 'output-rule') roots = [...document.querySelectorAll('input[placeholder="$.result == true"],textarea')].filter(element => element.value.includes('result'));
  else if (stepId === 'result-false') roots = [...document.querySelectorAll('[data-right-workspace-dock] .cm-content')];
  else if (stepId === 'result-true') roots = [...document.querySelectorAll('#workspace-center pre')];
  else if (stepId === 'question-wait' || stepId === 'permission-wait') roots = [...document.querySelectorAll('[data-theme-role="permission-card"]')].slice(-1);
  else roots = [...document.querySelectorAll('#workspace-center [data-acp-conversation-rail="timeline"]')];
  const modalGraph = Boolean(graphNode?.closest('[data-slot="sheet-content"]'));
  if (graphNode) roots = [];
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d');
  const boxes = [];
  const visible = rect => rect.width > 0 && rect.height > 0 && rect.top >= 0 && rect.bottom <= innerHeight && rect.left >= 0 && rect.right <= innerWidth;
  for (const root of roots) {
    if (root.matches('[data-theme-role="permission-card"]')) boxes.push(root.getBoundingClientRect());
    if (root instanceof HTMLInputElement || root instanceof HTMLTextAreaElement) {
      const style = getComputedStyle(root); const box = root.getBoundingClientRect();
      context.font = style.font;
      const width = Math.max(...root.value.split('\n').map(line => context.measureText(line).width));
      boxes.push({ left: box.left, right: box.left + parseFloat(style.paddingLeft) + width + 12, top: box.top - 28, bottom: box.bottom });
    } else {
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      while (walker.nextNode()) {
        if (!walker.currentNode.textContent.trim()) continue;
        // Disclosure controls and message actions are not the content being narrated.
        if (walker.currentNode.parentElement.closest('.acp-activity-collapse-button, [data-agent-message-actions]')) continue;
        const range = document.createRange(); range.selectNodeContents(walker.currentNode);
        boxes.push(...[...range.getClientRects()].filter(visible));
      }
    }
  }
  const graph = graphNode?.getBoundingClientRect();
  if (graph) boxes.push(graph);
  const routeBoxes = [];
  if (graphNode) {
    const pane = graphNode.closest('.react-flow').getBoundingClientRect();
    const labels = graphNode.closest('.react-flow').querySelectorAll('.workflow-edge-label');
    for (const [index, edge] of edges.entries()) {
      if (edge.to !== nodeId) continue;
      if (nodes.length && nodes.find(node => node.id === edge.from)?.outcome !== edge.label) continue;
      const path = document.querySelector('.react-flow__edge[data-id="' + edge.from + '-' + edge.to + '-' + index + '"] .react-flow__edge-path');
      if (!path) continue;
      const box = path.getBoundingClientRect();
      const clipped = { left: Math.max(box.left, pane.left), right: Math.min(box.right, pane.right), top: Math.max(box.top, pane.top), bottom: Math.min(box.bottom, pane.bottom) };
      if (clipped.right >= clipped.left && clipped.bottom >= clipped.top) {
        boxes.push(clipped); routeBoxes.push(clipped);
        const label = labels[index]?.getBoundingClientRect();
        if (label) { boxes.push(label); routeBoxes.push(label); }
      }
    }
  }
  if (!boxes.length) throw new Error('Missing semantic camera target: ' + stepId);
  function rect(values, minimumWidth, minimumHeight) {
    const left = Math.min(...values.map(box => box.left)); const right = Math.max(...values.map(box => box.right));
    const top = Math.min(...values.map(box => box.top)); const bottom = Math.max(...values.map(box => box.bottom));
    const width = Math.min(innerWidth, Math.max(minimumWidth, right - left + 40));
    const height = Math.min(innerHeight, Math.max(minimumHeight, bottom - top + 40));
    const x = Math.max(0, Math.min(innerWidth - width, (left + right - width) / 2));
    const y = Math.max(0, Math.min(innerHeight - height, (top + bottom - height) / 2));
    return { x: x / innerWidth, y: y / innerHeight, width: width / innerWidth, height: height / innerHeight };
  }
  function branchRect(targets, aspect, minimumWidth = 0) {
    // Match the stage aspect so contain-fit cannot reveal unrelated routes above.
    const padding = 12;
    const left = Math.min(...targets.map(box => box.left)) - padding;
    const top = Math.min(...targets.map(box => box.top)) - padding;
    const right = Math.max(...targets.map(box => box.right)) + padding;
    const bottom = Math.max(...targets.map(box => box.bottom)) + padding;
    const width = Math.min(innerWidth, Math.max(minimumWidth, right - left, (bottom - top) * aspect));
    const height = width / aspect;
    return { x: Math.max(0, Math.min(innerWidth - width, left)) / innerWidth,
      y: Math.max(0, Math.min(innerHeight - height, top)) / innerHeight, width: width / innerWidth, height: height / innerHeight };
  }
  const targets = nodeShot ? [graph] : routeBoxes;
  const mobile = modalGraph && targets.length ? branchRect(targets, 4 / 5) : rect(boxes, 200, 130);
  const desktop = graphNode && targets.length ? branchRect(targets, 4 / 3, innerWidth * 0.45) : rect(boxes, innerWidth * 0.45, innerHeight * 0.4);
  return { width: innerWidth, height: innerHeight, scrollWidth: document.documentElement.scrollWidth,
    desktop, mobile };
}
