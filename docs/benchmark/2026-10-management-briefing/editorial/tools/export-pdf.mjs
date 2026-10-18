// Adapted from huashu-design/scripts/export_deck_pdf.mjs.
// Local fonts are awaited explicitly; Windows paths use pathToFileURL; every slide must produce one PDF page.
import { chromium } from 'playwright';
import { PDFDocument } from 'pdf-lib';
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const args = {};
for (let index = 2; index < process.argv.length; index += 2) args[process.argv[index].replace(/^--/, '')] = process.argv[index + 1];
if (!args.slides || !args.out) throw new Error('Required: --slides <directory> --out <file.pdf>; optional: --executable <chrome.exe> --screenshots <directory>');
const slidesDir = path.resolve(args.slides);
const files = (await fs.readdir(slidesDir)).filter(file => file.endsWith('.html')).sort();
if (files.length !== 9) throw new Error(`Expected the approved nine slides, found ${files.length}.`);
const screenshotDir = args.screenshots && path.resolve(args.screenshots);
if (screenshotDir) await fs.mkdir(screenshotDir, { recursive: true });
const browser = await chromium.launch({ executablePath: args.executable || process.env.DECK_CHROMIUM_PATH || undefined });
const pages = [];
const checks = [];
try {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
  for (const file of files) {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(pathToFileURL(path.join(slidesDir, file)).href, { waitUntil: 'load' });
    await page.evaluate(async () => {
      await document.fonts.ready;
      await Promise.all([...document.images].map(image => image.decode()));
    });
    await page.emulateMedia({ media: 'screen' });
    const check = await page.evaluate(() => {
      const outside = [];
      const smallText = [];
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      let node;
      while ((node = walker.nextNode())) {
        const text = node.textContent.trim();
        const element = node.parentElement;
        if (!text || !element || element.closest('script, style, [aria-hidden="true"]')) continue;
        const collapsed = element.closest('details:not([open])');
        if (collapsed && !collapsed.querySelector('summary')?.contains(element)) continue;
        if (!element.checkVisibility()) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        const rects = [...range.getClientRects()].filter(rect => rect.width && rect.height);
        if (!rects.length) continue;
        if (rects.some(rect => rect.left < -1 || rect.top < -1 || rect.right > 1921 || rect.bottom > 1081)) outside.push(text.slice(0, 80));
        if (parseFloat(getComputedStyle(element).fontSize) < 18) smallText.push(text.slice(0, 80));
      }
      return { title: document.title, dimensions: [document.body.scrollWidth, document.body.scrollHeight], outside, smallText };
    });
    checks.push({ file, ...check, errors });
    if (check.outside.length || check.smallText.length || errors.length || check.dimensions[0] > 1920 || check.dimensions[1] > 1080) {
      throw new Error(`Layout verification failed: ${JSON.stringify(checks.at(-1))}`);
    }
    if (screenshotDir) await page.screenshot({ path: path.join(screenshotDir, file.replace(/\.html$/, '.png')), fullPage: false });
    const bytes = await page.pdf({ width: '1920px', height: '1080px', printBackground: true, margin: { top: 0, right: 0, bottom: 0, left: 0 }, preferCSSPageSize: false });
    const pdf = await PDFDocument.load(bytes);
    if (pdf.getPageCount() !== 1) throw new Error(`${file} exported ${pdf.getPageCount()} pages instead of one.`);
    pages.push(pdf);
    await page.close();
    console.log(`Verified and rendered ${file}`);
  }
} finally {
  await browser.close();
}
const merged = await PDFDocument.create();
merged.setTitle('Gold Band · Benchmark 阶段性汇报');
merged.setSubject('B 编辑式；数据截止 2026-10-13，Asia/Taipei');
for (const source of pages) {
  const [page] = await merged.copyPages(source, [0]);
  merged.addPage(page);
}
await fs.writeFile(path.resolve(args.out), await merged.save());
await fs.writeFile(path.resolve(args.out).replace(/\.pdf$/i, '.verification.json'), JSON.stringify({ pageCount: merged.getPageCount(), checks }, null, 2) + '\n');
console.log(`Wrote ${merged.getPageCount()} verified vector pages: ${path.resolve(args.out)}`);
