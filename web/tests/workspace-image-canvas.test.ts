import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const source = readFileSync(
  fileURLToPath(new URL('../src/components/workspace/files/WorkspaceImageCanvas.tsx', import.meta.url)),
  'utf8',
);
const conversationAssetPanelSource = readFileSync(
  fileURLToPath(new URL('../src/components/workspace/files/ConversationAssetWorkspacePanel.tsx', import.meta.url)),
  'utf8',
);

describe('WorkspaceImageCanvas interaction contract', () => {
  it('zooms only for Ctrl-wheel/native pinch while retaining touch pinch and mouse drag', () => {
    expect(source).toContain('minScale={MIN_IMAGE_SCALE}');
    expect(source).toContain('maxScale={MAX_IMAGE_SCALE}');
    expect(source).toContain('wheel={{ disabled: true }}');
    expect(source).toContain('trackPadPanning={{ disabled: true }}');
    expect(source).toContain('allowLeftClickPan: true');
    expect(source).toContain('allowRightClickPan: false');
    expect(source).toContain('pinch={{ disabled: false }}');
    expect(source).toContain("addEventListener('wheel', handleCtrlWheelZoom, { passive: false })");
    expect(source).toContain("removeEventListener('wheel', handleCtrlWheelZoom)");
    expect(source).toContain('ref={viewportRef}');
    expect(source).toContain('data-workspace-image-viewport="true"');
    expect(source).not.toContain('wrapperProps={{');
  });

  it('uses a semantic plain background and centers the image in both axes', () => {
    expect(source).toContain('bg-background active:cursor-grabbing');
    expect(source).toContain('items-center justify-center p-5');
    expect(source).not.toContain('linear-gradient');
  });

  it('receives the full remaining height from the conversation asset panel', () => {
    expect(conversationAssetPanelSource).toContain('className="flex min-h-0 flex-1 flex-col overflow-hidden"');
  });

  it('updates the hot-path scale label through a ref instead of React state', () => {
    expect(source).toContain('scaleLabelRef.current.textContent');
    expect(source).not.toContain('useState(');
  });

  it('commits each animation-frame transform to the authoritative ref before the library callback', () => {
    const authoritativeCommit = source.indexOf('transformStateRef.current = next;');
    const libraryCommit = source.indexOf(
      'transformRef.current?.setTransform(next.positionX, next.positionY, next.scale, 0);',
    );

    expect(authoritativeCommit).toBeGreaterThan(-1);
    expect(libraryCommit).toBeGreaterThan(authoritativeCommit);
  });

  it('fits back to the CSS-constrained viewport size instead of the minimum zoom', () => {
    expect(source).toContain("centerView(1, 180, 'easeOut')");
    expect(source).not.toContain("centerView(MIN_IMAGE_SCALE, 180, 'easeOut')");
  });
});
