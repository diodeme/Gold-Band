import { test } from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { measureWorkflowCamera } from './workflow-camera.mjs';
import { workflowSourceStoryboard, WORKFLOW_CAMERA_TRANSITION_MS } from './workflow-source-storyboard.mjs';

test('real graph Close transfers camera before the stable reading hold', () => {
  const operations = [];
  workflowSourceStoryboard({
    capture: false,
    browser(...args) { operations.push(['browser', ...args]); },
    evaluate(code) {
      if (code.includes('return {node:')) return { node: { x: 100, y: 100, width: 180, height: 100 }, pane: { x: 0, y: 0, width: 320, height: 780, bottom: 780 } };
      if (code.includes('workflowGraph.edges.findIndex')) return 'review-repair-0';
      if (code === 'window.goldBandPreview.snapshot()') return { selectedSession: { nodeId: 'continue' }, workflowGraph: { nodes: [{ current: true, nodeId: 'continue' }] } };
      return true;
    },
    camera(stepId, options) { operations.push(['camera', stepId, options]); },
    screenshot(stepId) { operations.push(['screenshot', stepId]); },
  });
  const closes = operations.flatMap((op, index) => op[0] === 'browser' && op.includes('Close') ? [index] : []);
  assert.equal(closes.length, 2);
  for (const [index, close] of closes.entries()) {
    const step = `branch-${index ? 'continue' : 'repair'}-follow`;
    const after = operations.slice(close + 1);
    const camera = after.findIndex(op => op[0] === 'camera' && op[1] === step);
    const hold = after.findIndex(op => op[0] === 'browser' && op[1] === 'wait' && op[2] === '1100');
    assert(camera >= 0 && camera < hold, `${step}: camera must transfer before the 1100ms reading hold after Close`);
    const bridge = operations.slice(0, close).findLast(op => op[0] === 'camera');
    assert.equal(bridge?.[2]?.overview, true, 'Close starts from a transition view containing the live UI');
    const bridgeIndex = operations.lastIndexOf(bridge);
    assert(operations.slice(bridgeIndex + 1, close).some(op => op[1] === 'wait' && op[2] === String(WORKFLOW_CAMERA_TRANSITION_MS)), 'The overview transition completes before Close');
    assert(after.slice(0, camera).some(op => op[2] === '--fn' && op[3].includes('document.getAnimations()')), 'Follow measurement waits for the native exit animation');
  }
});

test('desktop branch isolates the resolved route from the distant conversation and the later node shot', () => {
  const dom = new JSDOM('<main id="workspace-center"><div data-acp-conversation-rail="timeline">Followed conversation</div></main><div class="react-flow"><div class="react-flow__node" data-id="repair"></div><svg><g class="react-flow__edge" data-id="review-repair-0"><path class="react-flow__edge-path" /></g></svg><span class="workflow-edge-label">Failure</span></div>', { runScripts: 'outside-only' });
  const { window } = dom;
  Object.defineProperties(window, { innerWidth: { value: 1440 }, innerHeight: { value: 900 } });
  window.HTMLCanvasElement.prototype.getContext = () => ({});
  const box = (left, top, width, height) => ({ left, top, width, height, right: left + width, bottom: top + height });
  window.document.querySelector('.react-flow').getBoundingClientRect = () => box(1000, 85, 430, 805);
  window.document.querySelector('.react-flow__node').getBoundingClientRect = () => box(1290, 400, 260, 158);
  window.document.querySelector('path').getBoundingClientRect = () => box(1100, 410, 190, 80);
  window.document.querySelector('.workflow-edge-label').getBoundingClientRect = () => box(1150, 420, 60, 24);
  window.document.createRange = () => ({ selectNodeContents() {}, getClientRects: () => [box(350, 140, 260, 24)] });
  const camera = window.eval(`(${measureWorkflowCamera.toString()})('branch-repair', [{from:'review',to:'repair',label:'failure'}], [{id:'review',outcome:'failure'}])`).desktop;
  assert(camera.width * 1440 <= 648, 'Distant followed conversation must not shrink the route shot');
  assert(camera.y * 900 >= 398, 'Contain-fit must not reveal unrelated routes above');
  assert(camera.x * 1440 <= 1100 && (camera.x + camera.width) * 1440 >= 1290);
  assert(Math.abs(camera.width * 1440 / (camera.height * 900) - 4 / 3) < 0.001);
  dom.window.close();
});

test('phone branch reads the resolved route and its label before the separate target-node shot', () => {
  const dom = new JSDOM('<div data-slot="sheet-content"><div class="react-flow"><div class="react-flow__node" data-id="continue"></div><svg><g class="react-flow__edge" data-id="review-continue-0"><path class="react-flow__edge-path" /></g><g class="react-flow__edge" data-id="repair-continue-1"><path class="react-flow__edge-path" /></g></svg><span class="workflow-edge-label">Success</span><span class="workflow-edge-label">Success</span></div></div>', { runScripts: 'outside-only' });
  const { window } = dom;
  Object.defineProperties(window, { innerWidth: { value: 320 }, innerHeight: { value: 780 } });
  window.HTMLCanvasElement.prototype.getContext = () => ({});
  const box = (left, top, width, height) => ({ left, top, width, height, right: left + width, bottom: top + height });
  window.document.querySelector('.react-flow').getBoundingClientRect = () => box(35, 50, 280, 720);
  window.document.querySelector('.react-flow__node').getBoundingClientRect = () => box(220, 340, 226, 138);
  const paths = window.document.querySelectorAll('path');
  paths[0].getBoundingClientRect = () => box(35, 100, 220, 220);
  paths[1].getBoundingClientRect = () => box(80, 360, 140, 30);
  const labels = window.document.querySelectorAll('.workflow-edge-label');
  labels[0].getBoundingClientRect = () => box(100, 100, 60, 22);
  labels[1].getBoundingClientRect = () => box(90, 335, 60, 22);
  const camera = window.eval(`(${measureWorkflowCamera.toString()})('branch-continue', [{from:'review',to:'continue',label:'success'},{from:'repair',to:'continue',label:'success'}], [{id:'review',outcome:'failure'},{id:'repair',outcome:'success'}])`);
  assert(camera.mobile.y * 780 >= 315, 'Unchosen success route must not expand the phone camera');
  assert(camera.mobile.y * 780 <= 335, 'The selected route label must remain completely in frame');
  assert(camera.mobile.width * 320 <= 200, 'The target node has its own shot; do not shrink the route label to fit it');
  window.document.querySelector('.react-flow__node').getBoundingClientRect = () => box(60, 340, 226, 138);
  const target = window.eval(`(${measureWorkflowCamera.toString()})('branch-continue-node')`).mobile;
  assert.equal(target.y * 780, 328, 'The target shot starts above the complete node, below unrelated route labels');
  assert(target.x * 320 <= 60 && (target.x + target.width) * 320 >= 286);
  assert(Math.abs(target.width * 320 / (target.height * 780) - 4 / 5) < 0.001);
  dom.window.close();
});

test('permission framing includes the complete card, not just its text', () => {
  const dom = new JSDOM('<div data-theme-role="permission-card">Permission</div>', { runScripts: 'outside-only' });
  const { window } = dom;
  Object.defineProperties(window, { innerWidth: { value: 320 }, innerHeight: { value: 780 } });
  window.HTMLCanvasElement.prototype.getContext = () => ({});
  window.document.querySelector('div').getBoundingClientRect = () => ({ left: 8, right: 312, top: 100, bottom: 350, width: 304, height: 250 });
  window.document.createRange = () => ({ selectNodeContents() {}, getClientRects: () => [{ left: 30, right: 200, top: 140, bottom: 160, width: 170, height: 20 }] });
  const camera = window.eval(`(${measureWorkflowCamera.toString()})('permission-wait')`);
  assert(camera.mobile.x * 320 <= 8 && (camera.mobile.x + camera.mobile.width) * 320 >= 312);
  assert(camera.mobile.y * 780 <= 100 && (camera.mobile.y + camera.mobile.height) * 780 >= 350);
  dom.window.close();
});

test('modal branch framing excludes the occluded conversation and includes the incoming path', () => {
  const dom = new JSDOM('<main id="workspace-center"><div data-acp-conversation-rail="timeline">Occluded conversation</div></main><div data-slot="sheet-content"><div class="react-flow"><div class="react-flow__node" data-id="repair"></div><svg><g class="react-flow__edge" data-id="review-repair-0"><path class="react-flow__edge-path" /></g></svg></div></div>', { runScripts: 'outside-only' });
  const { window } = dom;
  Object.defineProperties(window, { innerWidth: { value: 320 }, innerHeight: { value: 780 } });
  window.HTMLCanvasElement.prototype.getContext = () => ({});
  const box = (left, top, width, height) => ({ left, top, width, height, right: left + width, bottom: top + height });
  window.document.querySelector('.react-flow__node').getBoundingClientRect = () => box(90, 360, 170, 100);
  window.document.querySelector('.react-flow').getBoundingClientRect = () => box(35, 50, 280, 720);
  window.document.querySelector('path').getBoundingClientRect = () => box(-100, 400, 190, 10);
  window.document.createRange = () => ({ selectNodeContents() {}, getClientRects: () => [box(90, 170, 190, 40)] });
  const camera = window.eval(`(${measureWorkflowCamera.toString()})('branch-repair', [{from:'review',to:'repair'}])`);
  assert(camera.mobile.y * 780 >= 330, 'The modal occludes the conversation behind it');
  assert(camera.mobile.x * 320 <= 35, 'The visible incoming route belongs to branch context');
  dom.window.close();
});

for (const step of ['streaming', 'question-resume', 'permission-resume']) {
  test(`${step} frames timeline content without composer or disclosure chrome`, () => {
    const dom = new JSDOM(`<main id="workspace-center"><div role="log"><div data-acp-conversation-rail="timeline"><p data-box="480,180,300,24">Active message</p><button data-box="1120,250,40,20" class="acp-activity-collapse-button">Collapse</button><button aria-expanded="false" data-box="480,220,240,20">read_file src/config.json</button></div><div data-acp-conversation-rail="composer" data-box="400,780,880,110">Composer</div></div></main>`, { runScripts: 'outside-only' });
    const { window } = dom;
    Object.defineProperties(window, { innerWidth: { value: 1440 }, innerHeight: { value: 900 } });
    window.HTMLCanvasElement.prototype.getContext = () => ({});
    const original = window.document.createRange.bind(window.document);
    window.document.createRange = () => {
      const range = original(); let node;
      range.selectNodeContents = value => { node = value; };
      range.getClientRects = () => {
        const [left, top, width, height] = node.parentElement.closest('[data-box]').dataset.box.split(',').map(Number);
        return [{ left, top, width, height, right: left + width, bottom: top + height }];
      };
      return range;
    };
    const camera = window.eval(`(${measureWorkflowCamera.toString()})(${JSON.stringify(step)})`);
    assert(camera.mobile.width * 1440 <= 340, 'Unrelated horizontal controls must not shrink the reading target');
    assert(camera.mobile.height * 900 <= 130, 'Composer must not expand the semantic reading target');
    dom.window.close();
  });
}
