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

  it('listens for paste globally via document, decoupled from the click target', () => {
    // Paste (Ctrl+V) works anywhere in the dialog via a document-level
    // listener, so it no longer conflicts with the click-to-pick-files
    // affordance on the drop zone. The drop zone itself is click-only.
    expect(SOURCE).toContain('document.addEventListener("paste"');
    expect(SOURCE).toContain('addFiles(files)');
    // The drop zone must NOT also bind onPaste (that was the conflict source).
    const dropZoneMatch = SOURCE.match(/cursor-pointer[\s\S]*?onClick=\{[^}]*pickFiles[\s\S]*?\}>/);
    expect(dropZoneMatch, 'drop zone block should be present').toBeTruthy();
    expect(dropZoneMatch![0]).not.toContain('onPaste');
    // No legacy window-level or ref-gated listeners.
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