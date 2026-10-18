import assert from 'node:assert/strict';

export const AFTER_CAMERA_TRANSITION_MS = 450;
export const AFTER_STEPS = ['attachment', 'hover-diff', 'workspace-edit', 'stage', 'commit', 'clean'];

export function afterSourceStoryboard({ browser, evaluate, screenshot, camera = () => {}, language = 'zh', capture = true }) {
  const text = (zh, en) => language === 'en' ? en : zh;
  const settle = () => browser('wait', '--fn', 'document.getAnimations().every(a => a.playState !== "running" || a.effect?.getComputedTiming().iterations === Infinity)');
  const click = (role, name) => { settle(); browser('find', 'role', role, 'click', '--name', name, '--exact'); };
  const mark = (id, secondary = false) => {
    settle(); camera(id); browser('wait', String(AFTER_CAMERA_TRANSITION_MS));
    if (capture && !secondary) evaluate(`window.goldBandPreview.marker(${JSON.stringify(id)})`);
    screenshot(id); browser('wait', '1800');
  };
  const overview = () => { camera('overview', { overview: true }); browser('wait', String(AFTER_CAMERA_TRANSITION_MS)); };
  const tab = kind => {
    click('button', text('打开新标签页', 'Open new tab'));
    browser('click', `[role="menuitem"]:nth-child(${kind === 'files' ? 1 : 2})`);
  };
  browser('wait', '[data-turn-attachments-card] [role="listitem"]');
  browser('scrollintoview', '[data-turn-attachments-card]');
  settle();
  if (capture) evaluate('window.goldBandPreview.start()');
  browser('click', '[data-turn-attachments-card] [role="listitem"]');
  browser('wait', 'article[aria-label="review.md"] [role="textbox"]');
  mark('attachment');
  overview();
  if (evaluate('Boolean(document.querySelector("[role=dialog]"))')) { browser('press', 'Escape'); browser('wait', '--fn', '!document.querySelector("[role=dialog]")'); }
  else click('button', text('收起右侧工作区', 'Collapse right workspace'));
  browser('hover', '[data-turn-file-changes-card] [role="listitem"]:first-child');
  browser('wait', '[data-turn-file-diff-preview] .cm-content');
  mark('hover-diff');
  overview(); browser('hover', 'header');
  click('button', text('展开右侧工作区', 'Expand right workspace'));
  tab('files');
  browser('wait', '[role="treeitem"]');
  browser('find', 'text', 'docs', 'click', '--exact');
  browser('wait', '--text', 'workspace-notes.md');
  browser('find', 'text', 'workspace-notes.md', 'click', '--exact');
  browser('wait', 'article[aria-label="workspace-notes.md"] [role="textbox"]');
  click('button', text('切换到源码模式', 'Switch to source mode'));
  browser('click', 'article[aria-label="workspace-notes.md"] [role="textbox"]');
  browser('press', 'Control+End');
  browser('keyboard', 'inserttext', text('仅在本地审阅。', 'Keep reviews local.'));
  browser('press', 'Control+s');
  mark('workspace-edit');
  overview(); tab('git');
  browser('wait', '[data-source-control-group="untracked"]');
  browser('click', '[data-source-control-group="untracked"] button:first-child');
  browser('wait', '--text', text('仅在本地审阅。', 'Keep reviews local.'));
  mark('source-diff', true);
  overview(); click('button', text('源码管理', 'Source Control'));
  click('button', text('更多更改操作', 'More change actions'));
  click('menuitem', text('全部暂存', 'Stage all'));
  browser('wait', '[data-source-control-group="staged"]');
  mark('stage');
  browser('find', 'placeholder', text('提交主题', 'Commit subject'), 'fill', text('完善工作区配置与说明', 'Review workspace files'));
  mark('commit');
  click('button', text('提交', 'Commit'));
  browser('wait', '[data-source-control-changes-empty="true"]');
  assert(evaluate('Boolean(document.querySelector("[data-source-control-changes-empty=true]"))'));
  mark('clean');
  overview(); screenshot('overview');
  return capture ? evaluate('window.goldBandPreview.stop()') : null;
}

export function measureAfterCamera(id) {
  const full = { x: 0, y: 0, width: 1, height: 1 };
  const size = { width: innerWidth, height: innerHeight };
  if (id === 'overview') return { ...size, desktop: full, mobile: full };
  const selector = id === 'attachment' ? 'article[aria-label="review.md"]' : id === 'hover-diff' ? '[data-turn-file-diff-preview]'
    : id === 'workspace-edit' ? 'article[aria-label="workspace-notes.md"]' : id === 'source-diff' ? '[data-source-control-diff-review], [data-git-diff-workspace], [data-turn-file-workspace], [data-theme-role="diff"]'
    : '[data-source-control-workspace]';
  let element = document.querySelector(selector);
  if (id === 'stage') element = document.querySelector('[data-source-control-group="staged"]');
  if (id === 'commit') element = document.querySelector('[data-source-control-workspace] input')?.parentElement;
  if (id === 'clean') element = document.querySelector('[data-source-control-changes-empty]');
  if (!element) throw new Error(`after.camera-target-missing:${id}`);
  const rect = element.getBoundingClientRect();
  const x = Math.max(0, rect.x - 8);
  const y = Math.max(0, id === 'clean' ? rect.y + rect.height / 2 - 80 : rect.y - (id === 'stage' ? 80 : 8));
  const right = Math.min(innerWidth, rect.right + 8);
  const bottom = Math.min(innerHeight, id === 'clean' ? rect.y + rect.height / 2 + 80 : ['attachment', 'workspace-edit', 'source-diff'].includes(id) ? rect.y + 360 : rect.bottom + 8);
  if (right <= x || bottom <= y) throw new Error(`after.camera-target-invisible:${id}`);
  const focus = { x: x / innerWidth, y: y / innerHeight, width: (right-x)/innerWidth, height: (bottom-y)/innerHeight };
  return { ...size, desktop: focus, mobile: focus };
}
