'use strict';

const origin = process.env.GOLD_BAND_MEMLAB_ORIGIN || 'http://127.0.0.1:1420';
const conversationUrl = `${origin}/chat/projects/default/tasks/mock-task/runs/run-052`;

module.exports = {
  url: () => conversationUrl,
  action: async (page) => {
    const versionSelector = '[aria-label="查看 docs/workspace-notes.md 的本轮版本"]';
    const diffSelector = '[aria-label="查看 src/config.json 的差异"]';
    await page.waitForSelector(versionSelector);
    for (let index = 0; index < 15; index += 1) {
      await page.click(index % 2 === 0 ? versionSelector : diffSelector);
      await page.waitForSelector('[data-right-workspace-dock="true"]');
    }
  },
  back: async (page) => {
    const closeSelector = '[aria-label="关闭标签页"]';
    await page.click(closeSelector);
    await page.click(closeSelector);
    await page.waitForSelector('[data-right-workspace-empty="true"]');
    await page.click('[aria-label="收起右侧工作区"]');
    await page.waitForSelector('[aria-label="展开右侧工作区"]');
    await new Promise((resolve) => setTimeout(resolve, 250));
  },
};
