import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

function source(relativePath: string) {
  return readFileSync(fileURLToPath(new URL(relativePath, import.meta.url)), 'utf8');
}

describe('conversation stop feedback surface', () => {
  it('keeps stop progress in the composer without covering the conversation canvas', () => {
    const chatSource = source('../src/components/acp/ACPChatDialog.tsx');
    const shellSource = source('../src/components/workspace/WorkspaceShell.tsx');

    expect(chatSource).not.toContain('AcpStopOverlay');
    expect(chatSource).not.toContain('stopOverlayPending');
    expect(shellSource).not.toContain('stoppingRun');
    expect(chatSource).toContain('composerStoppingPlaceholder');
  });
});
