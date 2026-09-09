import assert from 'node:assert/strict';

export const WORKFLOW_CAMERA_TRANSITION_MS = 450;

// UI commands own the answers and permissions. Cues only supply recorded agent facts.
export function workflowSourceStoryboard({ browser, evaluate, screenshot, camera = () => {}, language = 'zh', capture = true }) {
  const translations = {
    '编辑工作流': 'Edit workflow', '配置面板': 'Inspector', 'AI 输出验证': 'AI Output Validation', '人工 check': 'Manual check',
    '关闭标签页': 'Close tab', '已记录': 'recorded activities', '思考过程': 'Thought process', '收起': 'Collapse',
    '回答': 'Answer', '其他答案...': 'Other...', '每轮保留文件快照': 'Keep a file snapshot for every turn', '允许一次': 'Allow once',
    'GOLD BAND 工作流控制 · AI 输出判定': 'Gold Band Workflow Control · AI output decision',
    '配置尚未启用文件快照。': 'Snapshots are off.', '查看工作流': 'View workflow', '放大': 'Zoom in', '缩小': 'Zoom out',
  };
  const text = value => language === 'en' ? translations[value] ?? value : value;
  const settle = () => browser('wait', '--fn', `document.getAnimations().every(animation => animation.playState !== 'running' || animation.effect?.getComputedTiming().iterations === Infinity)`);
  const click = name => { settle(); browser('find', 'role', 'button', 'click', '--name', text(name), '--exact'); };
  const wait = value => browser('wait', '--text', text(value));
  const hold = () => browser('wait', '1100');
  const bridge = stepId => { camera(stepId, { overview: true }); browser('wait', String(WORKFLOW_CAMERA_TRANSITION_MS)); };
  const cue = name => evaluate(`window.goldBandPreview.advance(${JSON.stringify(name)})`);
  const panTo = (selector, requireFull = true) => {
    const geometry = evaluate(`(() => { const node = document.querySelector(${JSON.stringify(selector)}); return {node:node.getBoundingClientRect().toJSON(),pane:node.closest('.react-flow').getBoundingClientRect().toJSON()}; })()`);
    const { node, pane } = geometry;
    const dx = pane.x + pane.width / 2 - node.x - node.width / 2;
    const dy = pane.y + pane.height / 2 - node.y - node.height / 2;
    const count = Math.ceil(Math.max(Math.abs(dx) / (pane.width - 60), Math.abs(dy) / (pane.height - 100), 1));
    assert(count <= 4, 'Graph framing exceeds the bounded source pan');
    for (let index = 0; index < count; index++) {
      const x = pane.x + pane.width / 2 - dx / count / 2;
      const y = pane.bottom - 50 - Math.max(0, dy / count);
      browser('mouse', 'move', String(Math.round(x)), String(Math.round(y)));
      browser('mouse', 'down');
      browser('mouse', 'move', String(Math.round(x + dx / count)), String(Math.round(y + dy / count)));
      browser('mouse', 'up');
    }
    if (requireFull) browser('wait', '--fn', `(() => {const e=document.querySelector(${JSON.stringify(selector)});const n=e.getBoundingClientRect();const p=e.closest('.react-flow').getBoundingClientRect();return n.left>=p.left && n.right<=p.right && n.top>=p.top && n.bottom<=p.bottom;})()`);
  };
  const frameNode = id => {
    const selector = '.react-flow__node[data-id="' + id + '"]';
    for (let index = 0; index < 12; index++) {
      if (evaluate(`(() => {const e=document.querySelector(${JSON.stringify(selector)});return e.getBoundingClientRect().width <= e.closest('.react-flow').getBoundingClientRect().width - 40;})()`)) break;
      click('缩小');
    }
    panTo(selector);
  };
  const frameRoute = id => {
    const edgeId = evaluate(`window.goldBandPreview.snapshot().then(s => {const index=s.workflowGraph.edges.findIndex(e=>e.to===${JSON.stringify(id)}&&s.workflowGraph.nodes.find(n=>n.id===e.from)?.outcome===e.label);const e=s.workflowGraph.edges[index];return e.from+'-'+e.to+'-'+index;})`);
    const selector = '.react-flow__edge[data-id="' + edgeId + '"] .react-flow__edge-path';
    panTo(selector);
    // Each Sheet mount fits the graph anew. Restore a readable route using its measured span.
    for (let index = 0; index < 12; index++) {
      if (evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().width >= 150`)) break;
      click('放大');
      panTo(selector);
    }
    panTo(selector);
  };
  const mark = name => { settle(); camera(name); hold(); if (capture) evaluate(`window.goldBandPreview.marker(${JSON.stringify(name)})`); screenshot(name); hold(); };
  const follow = (stepId, closeName) => {
    bridge(stepId);
    click(closeName);
    settle();
    camera(stepId);
    hold(); screenshot(stepId); hold();
  };
  click('编辑工作流');
  wait('review');
  browser('wait', '--fn', `Boolean(document.querySelector('.react-flow__node[data-id="review"]'))`);
  // A mounted Sheet can still be outside the viewport during its entry animation.
  settle();
  browser('click', '.react-flow__node[data-id="review"]');
  browser('find', 'role', 'tab', 'click', '--name', text('配置面板'), '--exact');
  wait('AI 输出验证');
  if (capture) evaluate('window.goldBandPreview.start()');
  browser('find', 'text', text('AI 输出验证'), 'click', '--exact');
  wait('人工 check');
  mark('result-menu');
  browser('find', 'role', 'option', 'click', '--name', text('AI 输出验证'), '--exact');
  browser('scrollintoview', 'input[placeholder="$.result == true"]');
  mark('output-rule');
  click('关闭标签页');
  cue('streaming'); hold(); cue('streaming');
  evaluate(`Array.from(document.querySelectorAll('button')).find(e => e.textContent.includes(${JSON.stringify(text('已记录'))}) && e.getAttribute('aria-expanded') === 'false')?.click()`);
  hold();
  evaluate(`Array.from(document.querySelectorAll('button')).find(e => e.textContent.includes(${JSON.stringify(text('思考过程'))}) && e.getAttribute('aria-expanded') === 'false')?.click()`);
  mark('streaming');
  click('收起');
  cue('question-wait'); wait('回答'); mark('question-wait');
  click('回答');
  browser('find', 'placeholder', text('其他答案...'), 'fill', text('每轮保留文件快照'));
  browser('press', 'Enter');
  browser('wait', '--fn', `!document.querySelector('input[placeholder=${JSON.stringify(text('其他答案...'))}]')`);
  mark('question-resume');
  cue('permission-wait'); wait('允许一次'); mark('permission-wait');
  click('允许一次'); mark('permission-resume');
  cue('result-false'); wait('result.json');
  click('GOLD BAND 工作流控制 · AI 输出判定');
  click('result.json'); wait('配置尚未启用文件快照。'); mark('result-false');
  bridge('branch-repair');
  click('关闭标签页'); click('查看工作流');
  browser('wait', '--fn', `Boolean(document.querySelector('.react-flow__node'))`);
  settle();
  browser('scroll', 'down', '2000', '--selector', '[role="log"] .overflow-y-auto');
  hold();
  cue('branch'); wait('round-001/repair/attempt-001');
  const graphInSheet = evaluate(`Boolean(document.querySelector('[data-slot="sheet-content"] .react-flow'))`);
  frameRoute('repair');
  mark('branch-repair');
  bridge('branch-repair-node');
  frameNode('repair'); camera('branch-repair-node'); hold(); screenshot('branch-repair-node'); hold();
  follow('branch-repair-follow', graphInSheet ? 'Close' : '关闭标签页');
  cue('result-true'); wait('result.json');
  click('GOLD BAND 工作流控制 · AI 输出判定'); mark('result-true');
  bridge('branch-continue');
  click('查看工作流'); settle();
  cue('branch'); wait('round-001/continue/attempt-001');
  frameRoute('continue');
  mark('branch-continue');
  bridge('branch-continue-node');
  frameNode('continue'); camera('branch-continue-node'); hold(); screenshot('branch-continue-node'); hold();
  follow('branch-continue-follow', graphInSheet ? 'Close' : '关闭标签页');
  const snapshot = evaluate('window.goldBandPreview.snapshot()');
  assert.equal(snapshot.selectedSession.nodeId, 'continue');
  assert.equal(snapshot.workflowGraph.nodes.find(node => node.current).nodeId, 'continue');
  return capture ? evaluate('window.goldBandPreview.stop()') : snapshot;
}
