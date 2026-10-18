import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { measureAfterCamera } from './after-source-storyboard.mjs';

test('stage and commit focus on review content rather than the empty full-height panel', () => {
  const dom = new JSDOM('<section data-source-control-workspace id="panel"><section data-source-control-group="staged" id="staged"></section><div id="composer"><input></div></section>');
  Object.assign(globalThis, { document: dom.window.document, innerWidth: 1440, innerHeight: 900 });
  const box = (id, y, height) => { document.getElementById(id).getBoundingClientRect = () => ({ x: 1000, y, width: 440, height, right: 1440, bottom: y + height }); };
  box('panel', 77, 823); box('staged', 160, 100); box('composer', 714, 186);
  try {
    const stage = measureAfterCamera('stage').desktop, commit = measureAfterCamera('commit').desktop;
    assert(stage.height * 900 < 400, 'Stage text must not shrink with the empty panel');
    assert(commit.y * 900 >= 700, 'Commit camera must follow the actual composer');
    assert(commit.height * 900 <= 200);
  } finally { dom.window.close(); delete globalThis.document; delete globalThis.innerWidth; delete globalThis.innerHeight; }
});
