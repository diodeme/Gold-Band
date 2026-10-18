import assert from 'node:assert/strict';

export const PERSONALIZE_CAMERA_TRANSITION_MS = 450;
export const PERSONALIZE_STEPS = ['theme', 'font', 'avatar', 'layout-wide', 'layout-medium', 'layout-narrow', 'layout-restore-medium', 'layout-restore-wide', 'restore'];

export function personalizeSourceStoryboard({ browser, evaluate, screenshot, camera = () => {}, language = 'zh', capture = true }) {
  const text = (zh, en) => language === 'en' ? en : zh;
  const initial = evaluate('({width:innerWidth,height:innerHeight})');
  const route = '/chat/projects/default/tasks/mock-task/runs/run-052';
  const settle = () => browser('wait', '--fn', 'document.getAnimations().every(a => a.playState !== "running" || a.effect?.getComputedTiming().iterations === Infinity)');
  const click = (role, name) => { settle(); browser('find', 'role', role, 'click', '--name', name, '--exact'); };
  const overview = () => { camera('overview', { overview: true }); browser('wait', String(PERSONALIZE_CAMERA_TRANSITION_MS)); };
  const mark = (id, secondary = false) => {
    settle(); camera(id); browser('wait', String(PERSONALIZE_CAMERA_TRANSITION_MS));
    if (capture && !secondary) evaluate(`window.goldBandPreview.marker(${JSON.stringify(id)})`);
    screenshot(id); browser('wait', '1500');
  };
  const settings = () => {
    browser('set', 'viewport', '1440', String(initial.height));
    click('button', text('设置', 'Settings'));
    click('tab', text('个性化', 'Personalization'));
    browser('set', 'viewport', String(initial.width), String(initial.height));
  };
  settings();
  if (capture) evaluate('window.goldBandPreview.start()');
  camera('overview', { overview: true });
  click('button', text('选择主题', 'Choose theme'));
  browser('wait', '[data-slot="sheet-content"]');
  settle();
  camera('theme-menu'); browser('wait', String(PERSONALIZE_CAMERA_TRANSITION_MS));
  browser('find', 'role', 'button', 'click', '--name', text('技术中性', 'Tech Neutral'));
  browser('wait', '--fn', 'document.documentElement.dataset.theme === "builtin.tech-neutral"');
  mark('theme');
  overview();
  click('button', text('添加或取消字体', 'Add or remove fonts'));
  click('option', 'Georgia');
  mark('font-menu', true); browser('press', 'Escape');
  mark('font');
  overview();
  browser('scrollintoview', '[data-testid="avatar-editor-user"]');
  browser('click', '[data-testid="avatar-editor-user"] button:last-child');
  browser('wait', '[data-testid="avatar-editor-user"] button:last-child[aria-pressed="true"]');
  mark('avatar');
  overview(); browser('pushstate', route);
  browser('wait', '[data-turn-file-changes-card]');
  browser('focus', '[role="log"] .overflow-y-auto');
  browser('press', 'Control+Home');
  browser('wait', '--fn', 'document.querySelector("[data-acp-message-row=user]").getBoundingClientRect().top >= 0');
  mark('avatar-effect', true);
  overview();
  browser('set', 'viewport', '1440', String(initial.height));
  browser('wait', '--fn', 'document.getElementById("workspace-center").getBoundingClientRect().width > 600');
  settle();
  browser('click', '[data-turn-file-changes-card] [role="listitem"]:first-child');
  browser('wait', '[data-right-workspace-dock]');
  for (const [id, width, count] of [['layout-wide', 1440, 3], ['layout-medium', 900, 2], ['layout-narrow', 600, 1], ['layout-restore-medium', 900, 2], ['layout-restore-wide', 1440, 3]]) {
    overview(); browser('set', 'viewport', String(width), String(initial.height)); settle();
    const panels = evaluate('["workspace-navigation","workspace-center","workspace-right"].map(id=>document.getElementById(id)?.getBoundingClientRect().width||0)');
    assert.equal(panels.filter(value => value > 10).length, count, `${id}: ${JSON.stringify(panels)}`);
    mark(id);
  }
  overview(); settings();
  click('button', text('移除字体', 'Remove font'));
  browser('scrollintoview', '[data-testid="avatar-editor-user"]');
  browser('find', 'role', 'button', 'click', '--name', text('跟随主题', 'Follow theme'), '--exact');
  click('button', text('选择主题', 'Choose theme'));
  click('button', '$ gold-band run workflow ready Gold Band');
  browser('wait', '--fn', 'document.documentElement.dataset.theme === "builtin.gold-band"');
  browser('wait', '--fn', 'document.documentElement.dataset.font === "theme"');
  browser('pushstate', route);
  browser('set', 'viewport', String(initial.width), String(initial.height));
  mark('restore');
  return capture ? evaluate('window.goldBandPreview.stop()') : null;
}

export function measurePersonalizeCamera(id) {
  const size = { width: innerWidth, height: innerHeight };
  const full = { x: 0, y: 0, width: 1, height: 1 };
  if (id === 'overview' || id.startsWith('layout-') || id === 'restore') return { ...size, desktop: full, mobile: full };
  let element;
  if (id === 'theme-menu') element = document.querySelector('[data-slot="sheet-content"]');
  if (id === 'theme') element = document.querySelector('[data-slot="sheet-trigger"]')?.parentElement;
  if (id === 'font-menu') element = document.querySelector('[data-slot="popover-content"]');
  if (id === 'font') element = document.querySelector('[aria-label="已选字体优先级"], [aria-label="Selected font priority"]')?.parentElement;
  if (id === 'avatar') element = document.querySelector('[data-testid="avatar-editor-user"]');
  if (id === 'avatar-effect') element = document.querySelector('[data-acp-message-row="user"]');
  if (!element) throw new Error(`personalize.camera-target-missing:${id}`);
  const box = element.getBoundingClientRect();
  const x = Math.max(0, box.x - 8), y = Math.max(0, box.y - 8);
  const right = Math.min(innerWidth, box.right + 8), bottom = Math.min(innerHeight, box.y + (id === 'avatar-effect' ? 340 : box.height + 8));
  if (right <= x || bottom <= y) throw new Error(`personalize.camera-target-invisible:${id}`);
  const focus = { x: x / innerWidth, y: y / innerHeight, width: (right - x) / innerWidth, height: (bottom - y) / innerHeight };
  return { ...size, desktop: focus, mobile: focus };
}
