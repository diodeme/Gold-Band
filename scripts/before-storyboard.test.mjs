import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { measureBeforeCamera, beforeNavigationVisible, beforeSourceStoryboard } from './before-source-storyboard.mjs';

test('before storyboard selects the actual localized Skill tab in both languages', () => {
  for (const [language, label] of [['zh', 'SKILL 管理'], ['en', 'SKILLs']]) {
    const actions = [];
    beforeSourceStoryboard({ language, capture: false, browser: (...args) => actions.push(args), evaluate: () => true, screenshot: () => {} });
    assert(actions.some(args => args[0] === 'find' && args[2] === 'tab' && args[5] === label), `Missing actual ${language} Skill tab`);
    assert.equal(actions.filter(args => args[0] === 'find' && args[2] === 'tab' && args[5] === (language === 'zh' ? '工作流' : 'Workflow')).length, 2);
    assert(actions.some(args => args[0] === 'find' && args[1] === 'placeholder' && args[2] === (language === 'zh' ? '输入统一目标，会追加到每个内部节点' : 'Enter a shared goal appended to every internal node')));
  }
});

test('Skill framing follows its card and includes the Portal sync tooltip, not the management panel', () => {
  const dom = new JSDOM('<div id="workspace-center"><div data-slot="card" id="panel"><div data-slot="card" id="skill"><button aria-label="Stop syncing to Claude"><img src="/agent-icons/claude.svg"></button></div></div></div><div role="tooltip" id="tip">Stop syncing to Claude</div>');
  const rectangle = (id, x, y, width, height) => {
    dom.window.document.getElementById(id).getBoundingClientRect = () => ({ x, y, width, height, right: x + width, bottom: y + height });
  };
  rectangle('panel', 280, 150, 1100, 730);
  rectangle('skill', 300, 276, 350, 176);
  rectangle('tip', 315, 460, 160, 30);
  Object.assign(globalThis, { document: dom.window.document, innerWidth: 1440, innerHeight: 900 });
  try {
    const view = measureBeforeCamera('skill-sync').desktop;
    assert(view.width < 0.3, 'A distant panel must not reduce the Skill card reading scale');
    assert(view.y * 900 + view.height * 900 >= 490, 'Sync tooltip must fit in the same clock/camera');
  } finally { dom.window.close(); delete globalThis.document; delete globalThis.innerWidth; delete globalThis.innerHeight; }
});

test('offscreen sidebar controls do not bypass the visible sidebar opener', () => {
  const dom = new JSDOM('<button>Context</button>');
  Object.assign(globalThis, { document: dom.window.document, innerWidth: 320, innerHeight: 780 });
  const button = document.querySelector('button');
  button.getBoundingClientRect = () => ({ x: -260, y: 100, width: 240, height: 32, right: -20, bottom: 132 });
  try { assert.equal(beforeNavigationVisible('Context'), false); }
  finally { dom.window.close(); delete globalThis.document; delete globalThis.innerWidth; delete globalThis.innerHeight; }
});

test('AUTO framing excludes the empty composer and keeps its mode/config controls', () => {
  const dom = new JSDOM('<div data-conversation-composer="quick" id="composer"><div><div role="tablist"><button role="tab" id="auto">AUTO</button></div></div><div id="config"><textarea placeholder="goal for each node"></textarea></div></div>');
  Object.assign(globalThis, { document: dom.window.document, innerWidth: 320, innerHeight: 780 });
  const box = (node, y, height) => { node.getBoundingClientRect = () => ({ x: 17, y, width: 286, height, right: 303, bottom: y + height }); };
  box(document.getElementById('composer'), 164, 517);
  box(document.querySelector('[role="tablist"]').parentElement, 338, 106);
  box(document.getElementById('config'), 450, 230);
  try { assert(measureBeforeCamera('auto').mobile.height * 780 < 390, 'Empty composer must not shrink AUTO configuration'); }
  finally { dom.window.close(); delete globalThis.document; delete globalThis.innerWidth; delete globalThis.innerHeight; }
});

test('Direct frames the visible configuration menu and mode, not an offscreen submenu', () => {
  const dom = new JSDOM('<div data-conversation-composer="quick"><button aria-haspopup="menu" id="model">Model</button><div id="mode"><div role="tablist"><button role="tab">Direct</button></div></div></div><div data-slot="dropdown-menu-content" role="menu" id="menu">Model Reasoning</div>');
  Object.assign(globalThis, { document: dom.window.document, innerWidth: 320, innerHeight: 780 });
  const box=(id,y,height)=>{document.getElementById(id).getBoundingClientRect=()=>({x:28,y,width:284,height,right:312,bottom:y+height});};
  box('model',350,32);box('menu',390,80);box('mode',478,72);
  try { const rect=measureBeforeCamera('direct').mobile;assert(rect.width*320>280);assert(rect.y*780<=350);assert((rect.y+rect.height)*780>=550); }
  finally {dom.window.close();delete globalThis.document;delete globalThis.innerWidth;delete globalThis.innerHeight;}
});
