import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const SOURCE = readFileSync(
  path.resolve(__dirname, '../src/components/feedback/FeedbackDialog.tsx'),
  'utf8',
);

const SERVICE = readFileSync(
  path.resolve(__dirname, '../src/lib/attachment-service.ts'),
  'utf8',
);

describe('FeedbackDialog screenshot pipeline', () => {
  it('reuses the shared useAttachmentPicker hook instead of a bespoke path', () => {
    expect(SOURCE).toContain('useAttachmentPicker');
    expect(SOURCE).toContain('resolveAttachmentPaths');
    expect(SOURCE).toContain('acceptMimePrefix');
  });

  it('binds paste on the textarea (DOM-native), not on the window', () => {
    expect(SOURCE).toMatch(/onPaste=\{[^}]*extractPasteFiles/);
    expect(SOURCE).not.toContain("window.addEventListener('paste'");
    expect(SOURCE).not.toContain('window.addEventListener("paste"');
    expect(SOURCE).not.toContain('pasteZoneRef');
  });

  it('picks files through the native file input, never via asset:// fetch', () => {
    expect(SOURCE).not.toContain('asset://localhost');
    expect(SOURCE).not.toContain('pickAttachmentFiles');
    expect(SOURCE).toContain('type="file"');
    expect(SOURCE).toContain('accept="image/*"');
    expect(SOURCE).toContain('handleFilesFromInput');
  });

  it('limits screenshots to 4 images', () => {
    expect(SOURCE).toContain('maxCount: MAX_SCREENSHOTS');
    expect(SOURCE).toContain('MAX_SCREENSHOTS = 4');
  });
});

describe('useAttachmentPicker acceptMimePrefix', () => {
  it('rejects items whose mime does not match the prefix', () => {
    expect(SERVICE).toContain('acceptMimePrefix?: string');
    expect(SERVICE).toMatch(/if \(acceptMimePrefix\) \{/);
    expect(SERVICE).toContain('item.mime.startsWith(acceptMimePrefix)');
    const filterBlock = SERVICE.match(
      /if \(acceptMimePrefix\) \{[\s\S]*?return true;\s*\}/,
    );
    expect(filterBlock, 'acceptMimePrefix branch must short-circuit before allowedExts').toBeTruthy();
  });
});