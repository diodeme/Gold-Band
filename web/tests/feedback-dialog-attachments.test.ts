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
    expect(SOURCE).toContain('resolveAttachmentInputs');
    expect(SOURCE).toContain('acceptedMimes: FEEDBACK_IMAGE_MIMES');
    expect(SOURCE).not.toContain('resolveAttachmentPaths');
  });

  it('listens for paste globally via document, decoupled from the click target', () => {
    // Paste (Ctrl+V) works anywhere in the dialog via a document-level
    // listener, so it no longer conflicts with the click-to-pick-files
    // affordance on the drop zone. The drop zone itself is click-only.
    expect(SOURCE).toContain('document.addEventListener("paste"');
    expect(SOURCE).toContain('addFiles(files)');
    // The drop zone must NOT also bind onPaste (that was the conflict source).
    const dropZoneMatch = SOURCE.match(/cursor-pointer[\s\S]*?onClick=\{[^}]*fileInputRef\.current\?\.click\(\)[\s\S]*?\}>/);
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
    expect(SOURCE).toContain('accept={FEEDBACK_IMAGE_MIMES.join(",")}');
    expect(SOURCE).toContain('handleFilesFromInput');
    expect(SOURCE).toContain('fileInputRef.current?.click()');
  });

  it('limits screenshots to 4 images', () => {
    expect(SOURCE).toContain('maxCount: MAX_SCREENSHOTS');
    expect(SOURCE).toContain('MAX_SCREENSHOTS = 4');
  });
});

describe('useAttachmentPicker feedback serialization', () => {
  it('supports an exact MIME allowlist before the generic prefix path', () => {
    expect(SERVICE).toContain('acceptedMimes?: string[]');
    expect(SERVICE).toMatch(/if \(acceptedMimes\) \{/);
    expect(SERVICE).toContain('acceptedMimes.includes(item.mime.toLowerCase())');
    const filterBlock = SERVICE.match(
      /if \(acceptedMimes\) \{[\s\S]*?return true;\s*\}/,
    );
    expect(filterBlock, 'acceptedMimes branch must short-circuit before allowedExts').toBeTruthy();
  });

  it('serializes File bytes and rejects path-only attachment items', () => {
    expect(SERVICE).toContain('resolveAttachmentInputs');
    expect(SERVICE).toContain('attachments.some((item) => !item.file)');
    expect(SERVICE).toContain('dataBase64: await fileToBase64(item.file!)');
  });
});
