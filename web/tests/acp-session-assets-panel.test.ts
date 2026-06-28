import { describe, expect, it } from 'vitest';
import { createAcpSessionAssetPanelViewModel } from '../src/lib/acp-session-assets-panel';
import type { AssetItemVm } from '../src/types';

function asset(kind: string, name: string): AssetItemVm {
  return {
    kind,
    name,
    title: name,
    tone: 'muted',
    preview: '',
    roundId: 'round-001',
    nodeId: 'dev',
    attemptId: 'attempt-001',
  };
}

describe('createAcpSessionAssetPanelViewModel', () => {
  it('orders artifacts before attachments and normalizes item kinds', () => {
    const vm = createAcpSessionAssetPanelViewModel(
      [asset('custom-artifact', 'dev-result.json')],
      [asset('custom-attachment', 'dev-report.md')],
    );

    expect(vm.items.map((item) => `${item.kind}:${item.name}`)).toEqual([
      'artifact:dev-result.json',
      'attachment:dev-report.md',
    ]);
  });

  it('reports artifact, attachment, and total counts', () => {
    const vm = createAcpSessionAssetPanelViewModel(
      [asset('artifact', 'a.json'), asset('artifact', 'b.json')],
      [asset('attachment', 'notes.md')],
    );

    expect(vm.artifactCount).toBe(2);
    expect(vm.attachmentCount).toBe(1);
    expect(vm.totalCount).toBe(3);
  });

  it('omits zero-count asset kinds from the summary', () => {
    const attachmentOnly = createAcpSessionAssetPanelViewModel(
      [],
      [asset('attachment', 'notes.md')],
    );
    const artifactOnly = createAcpSessionAssetPanelViewModel(
      [asset('artifact', 'result.json')],
      [],
    );

    expect(attachmentOnly.summaryParts).toEqual([
      { kind: 'attachment', count: 1 },
    ]);
    expect(artifactOnly.summaryParts).toEqual([
      { kind: 'artifact', count: 1 },
    ]);
  });
});
