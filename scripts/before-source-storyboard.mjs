import assert from 'node:assert/strict';

export const BEFORE_CAMERA_TRANSITION_MS = 450;
export const BEFORE_STEPS = ['establish', 'agent', 'roles-skills', 'direct', 'workflow', 'auto', 'workflow-ready'];

export function beforeNavigationVisible(name) {
  return Array.from(document.querySelectorAll('button')).some(e => {
    const rect = e.getBoundingClientRect();
    return e.textContent === name && rect.width > 0 && rect.x >= 0 && rect.right <= innerWidth && rect.y >= 0 && rect.bottom <= innerHeight;
  });
}

export function beforeSourceStoryboard({ browser, evaluate, screenshot, camera = () => {}, language = 'zh', capture = true }) {
  const en = language === 'en';
  const text = (zh, english) => en ? english : zh;
  const settle = () => browser('wait', '--fn', `document.getAnimations().every(a => a.playState !== 'running' || a.effect?.getComputedTiming().iterations === Infinity)`);
  const click = (role, name) => { settle(); browser('find', 'role', role, 'click', '--name', name, '--exact'); };
  const transition = () => { camera('overview', { overview: true }); browser('wait', String(BEFORE_CAMERA_TRANSITION_MS)); };
  const mark = (stepId, { secondary = false } = {}) => {
    settle(); camera(stepId);
    browser('wait', String(BEFORE_CAMERA_TRANSITION_MS));
    if (capture && !secondary) evaluate(`window.goldBandPreview.marker(${JSON.stringify(stepId)})`);
    screenshot(stepId);
    browser('wait', '1600');
  };
  const navigate = (name, route) => {
    transition();
    if (!evaluate(`(${beforeNavigationVisible.toString()})(${JSON.stringify(name)})`)) {
      browser('pushstate', route);
    } else click('button', name);
    settle();
  };
  click('tab', 'Direct');
  if (capture) evaluate('window.goldBandPreview.start()');
  mark('establish');
  browser('click', '[role="tab"][id$="trigger-codex-acp"]');
  browser('wait', '--text', 'Codex');
  mark('agent');
  navigate(text('上下文', 'Context'), '/chat/contexts');
  click('tab', text('自定义角色', 'Custom Roles'));
  click('button', text('详情', 'Detail'));
  browser('wait', '--text', text('检查相关实现', 'Inspect the relevant implementation'));
  browser('scrollintoview', '[data-slot="sheet-content"] [data-slot="card"]:last-child');
  mark('roles-skills');
  transition(); browser('press', 'Escape');
  click('tab', text('SKILL 管理', 'SKILLs'));
  browser('wait', '--text', 'project-review');
  mark('skills', { secondary: true });
  browser('hover', `button[aria-label=${JSON.stringify(text('取消同步 Claude', 'Stop syncing to Claude'))}]`);
  mark('skill-sync', { secondary: true });
  navigate(text('快速对话', 'New Chat'), '/chat');
  click('tab', 'Direct');
  browser('click', '[role="tab"][id$="trigger-claude-acp"]');
  browser('find', 'role', 'button', 'click', '--name', text('模型 不指定', 'Model Unspecified'), '--exact');
  mark('direct');
  transition(); browser('press', 'Escape');
  click('tab', text('工作流', 'Workflow'));
  browser('click', '[data-conversation-workflow-selector] [role="combobox"]');
  mark('workflow');
  click('option', text('为工作区补充配置与说明', 'Workspace configuration and docs'));
  transition(); click('tab', 'AUTO');
  browser('click', '[data-conversation-composer="quick"] [role="combobox"]');
  click('option', 'Claude');
  browser('find', 'placeholder', text('输入统一目标，会追加到每个内部节点', 'Enter a shared goal appended to every internal node'), 'fill',
    text('完善工作区配置与说明，保留每轮文件快照。', 'Update workspace configuration and docs; keep per-turn file snapshots.'));
  mark('auto');
  transition(); click('tab', text('工作流', 'Workflow'));
  browser('fill', '[data-conversation-composer="quick"] textarea', text('请完善工作区配置，并补充使用说明。', 'Update the workspace configuration and add a usage guide.'));
  mark('workflow-ready');
  assert(evaluate(`document.querySelector('[data-conversation-workflow-selector]').textContent.includes(${JSON.stringify(text('为工作区补充配置与说明', 'Workspace configuration and docs'))})`));
  transition(); browser('wait', '1200'); screenshot('overview');
  if (!capture) return null;
  return evaluate('window.goldBandPreview.stop()');
}

// Measured only at finite storyboard boundaries; no selectors enter the published manifest.
export function measureBeforeCamera(stepId) {
  const full = { x: 0, y: 0, width: 1, height: 1 };
  const size = { width: innerWidth, height: innerHeight };
  if (stepId === 'overview' || stepId === 'establish') return { ...size, desktop: full, mobile: full };
  let element;
  if (stepId === 'roles-skills') element = document.querySelector('[data-slot="sheet-content"] [data-slot="card"]:last-child');
  else if (stepId.startsWith('skill')) element = document.querySelector('#workspace-center button[aria-label] img[src$="/claude.svg"]')?.closest('[data-slot="card"]');
  else if (stepId === 'direct') element = document.querySelector('[data-slot="dropdown-menu-content"]');
  else if (stepId === 'workflow') element = document.querySelector('[role="listbox"]');
  else if (stepId === 'auto') element = document.querySelector('[data-conversation-composer="quick"] textarea[placeholder*="node"], [data-conversation-composer="quick"] textarea[placeholder*="节点"]')?.parentElement;
  else if (stepId === 'agent') element = document.querySelector('[role="tab"][id$="trigger-codex-acp"]')?.closest('[role="tablist"]');
  else element = document.querySelector('[data-conversation-composer="quick"]');
  if (!element) throw new Error(`before.camera-target-missing:${stepId}`);
  const mode = ['auto','direct'].includes(stepId) ? Array.from(document.querySelectorAll('[role="tab"]')).find(node => node.textContent === (stepId === 'auto' ? 'AUTO' : 'Direct'))?.closest('[role="tablist"]')?.parentElement : null;
  const model = stepId === 'direct' ? document.querySelector('[data-conversation-composer="quick"] button[aria-haspopup="menu"]') : null;
  const bounds = [element, ...(mode ? [mode] : []), ...(model ? [model] : []), ...(stepId === 'skill-sync' ? document.querySelectorAll('[role="tooltip"]') : [])].map(node => node.getBoundingClientRect());
  const rect = { x: Math.min(...bounds.map(r=>r.x)), y: Math.min(...bounds.map(r=>r.y)), right: Math.max(...bounds.map(r=>r.right)), bottom: Math.max(...bounds.map(r=>r.bottom)) };
  const padding = 16;
  const x = Math.max(0, rect.x - padding), y = Math.max(0, rect.y - padding);
  const right = Math.min(innerWidth, rect.right + padding), bottom = Math.min(innerHeight, rect.bottom + padding);
  if (right <= x || bottom <= y) throw new Error(`before.camera-target-invisible:${stepId}`);
  const focus = { x: x / innerWidth, y: y / innerHeight, width: (right - x) / innerWidth, height: (bottom - y) / innerHeight };
  return { ...size, desktop: focus, mobile: focus };
}
