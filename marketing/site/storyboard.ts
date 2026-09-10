import type { Language } from './content';
import i18n from '@/i18n';
import { FULL_FRAME, type Frame, type Shot } from './timeline';
import { WORKFLOW_STEPS, type WorkflowStep } from './workflow-scene';
import { demoProfiles } from '../demo/catalog';
import { previewEvents, taskTitle } from './fixture';
import { themePackageSummaries } from '@/theme';
import { EditorView } from '@codemirror/view';

interface RecordingDriver {
  start(): void;
  advance(step: WorkflowStep): void;
  shot(shot: Omit<Shot, 'at'>): void;
  save(): Promise<unknown>;
  resize(width: number, height: number): Promise<void>;
  hover(element: Element): Promise<void>;
}

export async function recordReviewStory(driver: RecordingDriver, language: Language) {
  const t = i18n.getFixedT(language === 'en' ? 'en' : 'zh-CN');
  const focus = async (id: string, element: Element, mobile = element, duration = 2200) => {
    driver.shot({ id, desktop: minimumFrame(contentFrame(mobile), 480, 260), mobile: readingFrame(mobile), speed: 0.9, transition: 450 });
    await hold(duration);
  };
  const openEntry = async (label: string) => {
    selectOpen(await until(() => named('button', t('workspace.openNewTab')), 'workspace-entry'));
    click(await until(() => [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(e => e.textContent?.startsWith(label)), label));
  };
  const attachment = await until(() => document.querySelector<HTMLElement>('[data-turn-attachments-card] [role="listitem"]'), 'attachment');
  driver.start();
  driver.shot({ id: 'opening', desktop: FULL_FRAME, mobile: readingFrame(attachment), speed: 1.5, transition: 450 });
  await hold(1000);
  click(attachment);
  const report = await until(() => document.querySelector<HTMLElement>('article[aria-label="report.md"] .cm-content'), 'report');
  await focus('report', report, report.querySelector('.cm-line')!, 1500);
  const acceptance = await until(() => [...report.querySelectorAll<HTMLElement>('.cm-line')].find(e => e.textContent?.includes(language === 'zh' ? '文件快照已启用' : 'File snapshots enabled')), 'report-result');
  await focus('report-result', report, acceptance, 2200);
  const change = await until(() => document.querySelector<HTMLElement>('[role="listitem"][aria-label*="src/config.json"]'), 'captured-change');
  await driver.hover(change);
  const hover = await until(() => document.querySelector<HTMLElement>('[data-turn-file-diff-preview="browser-modified-config"] .cm-content'), 'hover-diff');
  await focus('hover-diff', hover.closest('[data-turn-file-diff-preview]')!, hover, 2800);
  await driver.hover(document.querySelector('[data-right-workspace-tab-strip]')!);
  await openEntry(t('workspace.files'));
  click(await until(() => named('[role="tree"] span', 'docs'), 'docs-directory'));
  click(await until(() => named('[role="tree"] span', 'workspace-notes.md'), 'workspace-file'));
  const article = await until(() => document.querySelector<HTMLElement>('article[aria-label="workspace-notes.md"]'), 'latest-file');
  const source = await until(() => named('button', t('workspace.filesPanel.viewMarkdownSource')), 'source-mode');
  click(source);
  const editor = await until(() => article.querySelector<HTMLElement>('.cm-content[contenteditable="true"]'), 'editable-document');
  const view = EditorView.findFromDOM(editor)!;
  await focus('workspace-file', article, editor, 1700);
  view.focus();
  const addition = language === 'zh' ? '\n已完成本地审阅。\n' : '\nReviewed locally.\n';
  for (const character of addition) {
    view.dispatch({ changes: { from: view.state.doc.length, insert: character }, selection: { anchor: view.state.doc.length + character.length }, scrollIntoView: true });
    await hold(45);
  }
  await until(() => article.textContent?.includes(t('workspace.filesPanel.saved')), 'file-saved');
  await focus('file-saved', article, editor, 2200);
  await openEntry(t('sourceControl.title'));
  const docRow = await until(() => [...document.querySelectorAll<HTMLElement>('[role="tabpanel"] button')].find(e => e.textContent?.includes('docs/workspace-notes.md')), 'git-doc');
  await focus('source-control', docRow.closest('[role="tabpanel"]')!, docRow, 1600);
  click(docRow);
  const diff = await until(() => [...document.querySelectorAll<HTMLElement>('[data-right-workspace-dock] .cm-content')].find(e => e.textContent?.includes(addition.trim())), 'git-saved-diff');
  await focus('git-diff', diff.closest('section') ?? diff, diff, 2700);
  click(await until(() => named('[data-right-workspace-tab-strip] button', t('sourceControl.title')), 'source-control-tab'));
  for (const path of ['docs/workspace-notes.md', 'src/config.json']) {
    click(await until(() => named('button', `${t('sourceControl.stage')}: ${path}`), `stage-${path}`));
    await until(() => named('button', `${t('sourceControl.unstage')}: ${path}`), `staged-${path}`);
  }
  const staged = await until(() => document.querySelector<HTMLElement>('[data-source-control-group="staged"]'), 'staged-files');
  await focus('staged', staged, staged, 1800);
  const input = await until(() => document.querySelector<HTMLInputElement>(`input[placeholder="${t('sourceControl.commitSubject')}"]`), 'commit-subject');
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, language === 'zh' ? '完善工作区配置与说明' : 'Review workspace changes');
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await focus('commit-subject', input.closest('[role="tabpanel"]')!, input, 2200);
  const commit = await until(() => {
    const button = named('button', t('sourceControl.commit')) as HTMLButtonElement | undefined;
    return button && !button.disabled ? button : null;
  }, 'commit-enabled');
  click(commit);
  const clean = await until(() => named('[role="tabpanel"] div', t('sourceControl.clean')), 'clean-workspace');
  await focus('committed', clean.closest('[role="tabpanel"]')!, clean, 2600);
  driver.shot({ id: 'closing', desktop: FULL_FRAME, mobile: readingFrame(clean), speed: 1.2, transition: 500 });
  await hold(1200);
  return driver.save();
}

export async function recordPersonalizationStory(driver: RecordingDriver, language: Language) {
  const t = i18n.getFixedT(language === 'en' ? 'en' : 'zh-CN');
  const initialTheme = document.documentElement.dataset.theme;
  const original = themePackageSummaries.find(theme => theme.id === initialTheme)!;
  const alternate = themePackageSummaries.find(theme => theme.id !== initialTheme)!;
  const locale = language === 'en' ? 'en' : 'zh-CN';
  const focus = async (id: string, element: Element, mobile = element, duration = 2000, text = true) => {
    element.scrollIntoView({ block: 'center', behavior: 'instant' });
    await hold(300);
    await settleSourceAnimations(element);
    driver.shot({ id, desktop: minimumFrame(frameFor(element), 480, 260), mobile: text ? readingFrame(mobile) : minimumFrame(frameFor(mobile, 8), 288, 144), speed: 0.9, transition: 450 });
    await hold(duration);
  };
  const settings = async () => {
    click(await until(() => named('button', t('conversation.sidebar.settings'))));
    click(await until(() => named('[role="tab"]', t('settings.tabs.appearance'))));
  };
  const selectTheme = async (id: string, theme: typeof original) => {
    click(await until(() => named('button', t('settings.chooseTheme'))));
    const label = await until(() => named('[role="dialog"] span', theme.name[locale]));
    await focus(id, label.closest('button')!, label);
    click(label.closest('button')!);
    await until(() => !document.querySelector('[role="dialog"]') && document.documentElement.dataset.theme === theme.id);
  };
  const conversation = async () => {
    click(await until(() => named('#workspace-navigation span', taskTitle(language))));
    await until(() => document.querySelector('textarea'));
  };
  const replyText = language === 'en' ? 'Configuration and docs updated.' : '配置与文档已更新。';
  driver.start();
  driver.shot({ id: 'opening', desktop: FULL_FRAME, mobile: FULL_FRAME, speed: 1.7, transition: 450 });
  await hold(1000);
  await settings();
  await selectTheme('theme-options', alternate);
  const fontPicker = await until(() => named('button', t('settings.chooseLocalFont')));
  fontPicker.scrollIntoView({ block: 'center', behavior: 'instant' });
  click(fontPicker);
  const font = await until(() => named('[role="option"]', 'Gold Band MiSans'));
  await focus('font-option', font);
  click(font);
  font.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  await until(() => !document.querySelector('[role="listbox"]'));
  await conversation();
  const fontReply = await until(() => named('p', replyText));
  await until(() => getComputedStyle(fontReply).fontFamily.includes('Gold Band MiSans'));
  await focus('conversation-appearance', document.getElementById('workspace-center')!, fontReply);
  await settings();
  const avatarEditor = document.querySelector<HTMLElement>('[data-testid="avatar-editor-agent"]')!;
  avatarEditor.scrollIntoView({ block: 'center', behavior: 'instant' });
  const avatarButton = avatarEditor.querySelector<HTMLElement>('button')!;
  selectOpen(avatarButton);
  const avatar = await until(() => named('[role="menuitem"]', t('settings.avatar.useRecent')));
  const avatarMenu = avatar.closest('[role="menu"]')!;
  await focus('avatar-option', avatarMenu, avatarMenu, 2000, false);
  click(avatar);
  await until(() => !document.querySelector('[role="menu"]'));
  await conversation();
  const reply = await until(() => named('p', replyText));
  const renderedAvatar = await until(() => [...document.querySelectorAll<HTMLImageElement>('#workspace-center [data-slot="avatar"] img')]
    .filter(image => image.src.startsWith('data:image/png') && image.complete && image.naturalWidth > 0).at(-1));
  reply.scrollIntoView({ block: 'center', behavior: 'instant' });
  await hold(300);
  const avatarBounds = renderedAvatar.getBoundingClientRect();
  const replyRange = document.createRange();
  replyRange.selectNodeContents(reply);
  const replyBounds = replyRange.getBoundingClientRect();
  const avatarContext = frameForBounds({
    left: Math.min(avatarBounds.left, replyBounds.left),
    top: Math.min(avatarBounds.top, replyBounds.top),
    right: Math.max(avatarBounds.right, replyBounds.right),
    bottom: Math.max(avatarBounds.bottom, replyBounds.bottom),
  }, 16);
  driver.shot({ id: 'conversation-avatar', desktop: minimumFrame(avatarContext, 480, 260),
    mobile: minimumFrame(avatarContext, 288, 144), speed: 0.9, transition: 450 });
  await hold(2000);
  const file = await until(() => [...document.querySelectorAll<HTMLElement>('button[aria-label]')]
    .find(element => element.getAttribute('aria-label')?.includes('docs/workspace-notes.md')));
  click(file);
  await until(() => document.querySelector('[data-right-workspace-dock]'));
  for (const [index, width] of [1440, 900, 600, 900, 1440].entries()) {
    await driver.resize(width, 880);
    await hold(350);
    const visible = ['workspace-navigation', 'workspace-center', 'workspace-right']
      .filter(id => (document.getElementById(id)?.getBoundingClientRect().width ?? 0) > 10).length;
    if (visible !== [3, 2, 1, 2, 3][index]) throw { code: 'site.storyboard-layout-invalid', params: { width, visible } };
    driver.shot({ id: `layout-${index + 1}`, desktop: FULL_FRAME, mobile: FULL_FRAME, speed: 1, transition: 450 });
    await hold(1600);
  }
  await settings();
  await selectTheme('restore-theme', original);
  const defaultFont = await until(() => [...document.querySelectorAll<HTMLElement>('button[aria-pressed]')]
    .find(element => element.textContent?.startsWith(t('settings.fontDefault'))));
  click(defaultFont);
  await until(() => defaultFont.getAttribute('aria-pressed') === 'true', 'restore-font-selected');
  const restoreAvatarLabel = t('settings.avatar.remove', { type: t('settings.avatar.agent.title') });
  click(await until(() => {
    const button = named('button', restoreAvatarLabel);
    return button && !button.matches(':disabled') && button;
  }, 'restore-avatar-enabled'));
  await until(() => !named('button', restoreAvatarLabel), 'restore-avatar-completed');
  await conversation();
  driver.shot({ id: 'closing', desktop: FULL_FRAME, mobile: FULL_FRAME, speed: 1, transition: 500 });
  await hold(1800);
  return driver.save();
}
const hold = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
async function until<T>(get: () => T | undefined | null | false, target = 'unspecified'): Promise<T> {
  const deadline = performance.now() + 8000;
  while (performance.now() < deadline) {
    const value = get();
    if (value) return value;
    await new Promise(requestAnimationFrame);
  }
  throw { code: 'site.storyboard-target-missing', params: { target } };
}
function named(selector: string, text: string) {
  return [...document.querySelectorAll<HTMLElement>(selector)].find(e => (e.getAttribute('aria-label') || e.textContent || '').trim() === text);
}
function frameFor(element: Element, padding = 20): Frame {
  const r = element.getBoundingClientRect();
  return frameForBounds(r, padding);
}
function frameForBounds(r: { left: number; top: number; right: number; bottom: number }, padding = 20): Frame {
  const left = Math.max(0, r.left - padding);
  const top = Math.max(0, r.top - padding);
  return { x: left / innerWidth, y: top / innerHeight, width: (Math.min(innerWidth, r.right + padding) - left) / innerWidth, height: (Math.min(innerHeight, r.bottom + padding) - top) / innerHeight };
}
function graphFrame(nodeId?: 'repair' | 'deliver'): Frame {
  const selector = nodeId ? `[data-id="${nodeId}"]` : '[data-id="review"], [data-id="repair"], [data-id="deliver"]';
  const rectangles = [...document.querySelectorAll(selector)]
    .map(element => element.getBoundingClientRect()).filter(rect => rect.width > 0 && rect.height > 0);
  if (!rectangles.length) throw { code: 'site.storyboard-graph-missing' };
  return frameForBounds({ left: Math.min(...rectangles.map(r => r.left)), top: Math.min(...rectangles.map(r => r.top)),
    right: Math.max(...rectangles.map(r => r.right)), bottom: Math.max(...rectangles.map(r => r.bottom)) }, nodeId ? 12 : 35);
}
function contentFrame(element: Element): Frame {
  if (element instanceof HTMLInputElement) {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    const context = document.createElement('canvas').getContext('2d')!;
    context.font = style.font;
    const left = rect.left + parseFloat(style.paddingLeft);
    return frameForBounds({ left, top: rect.top, right: Math.min(rect.right, left + context.measureText(element.value || element.placeholder).width), bottom: rect.bottom }, 24);
  }
  if (element instanceof HTMLTextAreaElement) return frameFor(element, 24);
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  const rectangles: DOMRect[] = [];
  while (walker.nextNode()) {
    if (!walker.currentNode.textContent?.trim()) continue;
    const range = document.createRange();
    range.selectNodeContents(walker.currentNode);
    const rect = range.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) rectangles.push(rect);
  }
  if (!rectangles.length) throw { code: 'site.storyboard-content-missing' };
  return frameForBounds({ left: Math.min(...rectangles.map(r => r.left)), top: Math.min(...rectangles.map(r => r.top)),
    right: Math.max(...rectangles.map(r => r.right)), bottom: Math.max(...rectangles.map(r => r.bottom)) }, 24);
}
function readingFrame(element: Element): Frame {
  return minimumFrame(contentFrame(element), 288, 144);
}
async function settleSourceAnimations(element: Element) {
  const animations = element.getAnimations({ subtree: true }).filter(animation => Number.isFinite(animation.effect?.getComputedTiming().endTime));
  await Promise.all(animations.map(animation => animation.finished.catch(() => {})));
}
function minimumFrame(bounds: Frame, minimumWidth: number, minimumHeight: number): Frame {
  const width = Math.min(1, Math.max(bounds.width, minimumWidth / innerWidth));
  const height = Math.min(1, Math.max(bounds.height, minimumHeight / innerHeight));
  return { x: Math.max(0, Math.min(1 - width, bounds.x + (bounds.width - width) / 2)),
    y: Math.max(0, Math.min(1 - height, bounds.y + (bounds.height - height) / 2)), width, height };
}
function click(element: HTMLElement) {
  if (element.getAttribute('role') === 'tab') element.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
  else element.click();
}
function selectOpen(element: HTMLElement) {
  element.focus();
  element.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
}

export async function recordSetupStory(driver: RecordingDriver, language: Language) {
  const t = i18n.getFixedT(language === 'en' ? 'en' : 'zh-CN');
  const focus = async (id: string, element: Element, mobileElement = element, duration = 2200) => {
    await settleSourceAnimations(element);
    driver.shot({ id, desktop: frameFor(element, 25), mobile: readingFrame(mobileElement), speed: 0.9, transition: 450 });
    await hold(duration);
  };
  driver.start();
  driver.shot({ id: 'opening', desktop: FULL_FRAME, mobile: FULL_FRAME, speed: 1.7, transition: 450 });
  click(await until(() => named('button', t('conversation.sidebar.agentManagement'))));
  const claude = await until(() => named('h3', 'Claude'));
  const agentCard = claude.closest('[data-slot="card"]')!;
  await focus('agent-claude', agentCard, claude);
  const codex = await until(() => named('h3', 'Codex'));
  await focus('agent-codex', codex.closest('[data-slot="card"]')!, codex);
  click(await until(() => named('button', t('conversation.sidebar.contextManagement'))));
  click(await until(() => named('[role="tab"]', t('contextManagement.builtInSectionTitle'))));
  const profile = demoProfiles(language === 'en' ? 'en' : 'zh-cn')[0];
  const role = await until(() => named('[data-slot="card-title"]', profile.name));
  await focus('roles', role.closest('[data-slot="card"]')!, role);
  click(await until(() => named('[role="tab"]', t('contextManagement.tabs.skills'))));
  const skill = await until(() => named('span', 'project-review'));
  const skillCard = skill.closest('[data-slot="card"]')!;
  await focus('skill', skillCard, skill.parentElement!);
  const sync = await until(() => skillCard.querySelector('[data-testid="skill-agent-overflow"]'), 'skill-sync');
  driver.shot({ id: 'skill-sync', desktop: frameFor(skillCard, 25),
    mobile: minimumFrame(frameFor(sync, 16), 288, 144), speed: 0.9, transition: 450 });
  await hold(2200);
  click(await until(() => named('button', t('conversation.sidebar.newChat'))));
  const input = await until(() => document.querySelector<HTMLTextAreaElement>(`textarea[placeholder="${t('conversation.home.inputPlaceholder')}"]`));
  const requirement = previewEvents(language)[0].content!;
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  for (let i = 1; i <= requirement.length; i += 3) {
    setter.call(input, requirement.slice(0, i + 2));
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await hold(25);
  }
  for (const mode of ['direct', 'workflow', 'auto']) {
    const tab = await until(() => named('[role="tab"]', t(`conversation.home.${mode}`)));
    click(tab);
    await hold(350);
    const modeList = tab.closest('[role="tablist"]')!;
    await focus(`mode-${mode}`, document.getElementById('workspace-center')!, modeList, 1700);
    if (mode !== 'direct') {
      const selector = await until(() => document.querySelector<HTMLElement>('#workspace-center [role="combobox"]'));
      selectOpen(selector);
      const menu = await until(() => document.querySelector('[role="listbox"]'));
      await focus(`${mode}-options`, menu, mode === 'auto' ? menu.querySelector('[role="option"]')! : menu);
      const option = mode === 'auto' ? await until(() => named('[role="option"]', 'Claude'))
        : [...menu.querySelectorAll<HTMLElement>('[role="option"]')].at(-1)!;
      const selectedText = option.textContent!.trim();
      option.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
      const selected = await until(() => !document.querySelector('[role="listbox"]')
        && [...document.querySelectorAll<HTMLElement>('#workspace-center [role="combobox"]')].find(element => element.textContent?.includes(selectedText)));
      await hold(350);
      await focus(`${mode}-selected`, selected, selected, 1600);
    }
  }
  driver.shot({ id: 'closing', desktop: FULL_FRAME, mobile: FULL_FRAME, speed: 1.5, transition: 500 });
  await hold(1400);
  return driver.save();
}

export async function recordWorkflowStory(driver: RecordingDriver, language: Language) {
  const overviewViewport = { width: innerWidth, height: innerHeight };
  const interactionViewportWidth = 360;
  const en = language === 'en';
  const t = i18n.getFixedT(en ? 'en' : 'zh-CN');
  const edit = t('conversation.runtime.editWorkflow');
  const view = t('conversation.runtime.viewWorkflow');
  const editorButton = await until(() => named('button', edit));
  driver.start();
  const shot = (id: string, desktop = FULL_FRAME, mobile = desktop, speed = 1, transition = 500) => driver.shot({ id, desktop, mobile, speed, transition });
  shot('opening', FULL_FRAME, { x: 0.2, y: 0.05, width: 0.65, height: 0.8 }, 1.8);
  await hold(1200);
  click(editorButton);
  click(await until(() => document.querySelector<HTMLElement>('[data-id="review"]')));
  const inspector = named('[role="tab"]', t('workflowEditor.inspector'));
  if (inspector) click(inspector);
  const validation = await until(() => named('[role="combobox"]', t('workflowEditor.outputValidation')));
  validation.scrollIntoView({ block: 'center', behavior: 'instant' });
  await hold(400);
  selectOpen(validation);
  const menu = await until(() => document.querySelector<HTMLElement>('[role="listbox"]'));
  await settleSourceAnimations(menu);
  shot('result-menu', frameFor(menu, 55), frameFor(menu, 30), 0.9);
  await hold(2200);
  const option = await until(() => named('[role="option"]', t('workflowEditor.outputValidation')));
  option.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  const expression = await until(() => [...document.querySelectorAll<HTMLInputElement>('input')].find(e => e.value === '$.result == true'));
  expression.scrollIntoView({ block: 'end', behavior: 'instant' });
  await hold(300);
  const expressionField = expression.parentElement!;
  const controls = expressionField.parentElement!;
  const schema = await until(() => controls.querySelector('textarea'));
  const schemaField = schema.parentElement!.parentElement!;
  const panel = frameFor(controls, 16);
  shot('json-contract', panel, minimumFrame(frameFor(schemaField, 8), 288, 144), 0.85);
  await hold(2200);
  shot('success-expression', panel, minimumFrame(frameFor(expressionField, 8), 288, 144), 0.85);
  await hold(2200);
  click(await until(() => named('button', view)));
  await until(() => document.querySelector('[data-id="review"]'));
  click(await until(() => {
    const button = named('button', t('graph.fitView'));
    return button && !button.matches(':disabled') && button;
  }, 'workflow-overview-enabled'));
  await until(() => {
    const graph = document.querySelector('.react-flow')?.getBoundingClientRect();
    const nodes = [...document.querySelectorAll('[data-id="review"], [data-id="repair"], [data-id="deliver"]')];
    return graph && nodes.length === 3 && nodes.every(node => {
      const rect = node.getBoundingClientRect();
      return rect.width > 0 && rect.left >= graph.left && rect.right <= graph.right
        && rect.top >= graph.top && rect.bottom <= graph.bottom;
    });
  }, 'workflow-overview-visible');
  shot('execution', FULL_FRAME, { x: 0.2, y: 0.06, width: 0.5, height: 0.85 }, 1);
  await hold(1200);
  for (const step of WORKFLOW_STEPS.slice(1)) {
    if (step === 'question') {
      click(await until(() => named('button', t('workspace.closeWorkspace'))));
      await driver.resize(interactionViewportWidth, overviewViewport.height);
    }
    if (step === 'false') {
      await driver.resize(overviewViewport.width, overviewViewport.height);
      click(await until(() => named('button', t('workspace.openWorkspace'))));
    }
    driver.advance(step);
    await hold(200);
    const stage = document.getElementById('workspace-center')!;
    const center = frameFor(stage, 0);
    const focus = ['false', 'true', 'question', 'permission'].includes(step);
    const branchTransition = ['repair', 'deliver', 'complete'].includes(step);
    const graph = branchTransition ? graphFrame() : FULL_FRAME;
    const output = ['false', 'true'].includes(step) ? [...stage.querySelectorAll('pre')].at(-1) : null;
    let interaction: Element | null = null;
    if (step === 'question') interaction = (await until(() => named('button', en ? 'Enable file snapshots' : '启用文件变更快照'))).closest('[data-slot="card"]');
    if (step === 'permission') interaction = (await until(() => named('button', en ? 'Allow once' : '允许一次'))).parentElement?.parentElement ?? null;
    const reading = output ? frameFor(output, 35) : interaction ? frameFor(interaction, 8) : center;
    const mobileGraph = branchTransition ? graphFrame(step === 'repair' ? 'repair' : 'deliver') : center;
    shot(step, focus ? reading : graph, focus ? reading : mobileGraph, focus || branchTransition ? 0.85 : 1.5);
    await hold(focus ? 2800 : 1600);
  }
  shot('closing', FULL_FRAME, { x: 0.55, y: 0.15, width: 0.45, height: 0.7 }, 1);
  await hold(2200);
  return driver.save();
}
