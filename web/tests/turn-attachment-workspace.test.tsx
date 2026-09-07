/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { TurnAttachmentWorkspacePanel } from '@/components/workspace/files/TurnAttachmentWorkspacePanel';
import type { TurnAttachmentWorkspaceResource } from '@/components/workspace/right-workspace-context';

const { primeExternalGrantMock, releaseExternalFileAccessMock, resolveTurnAttachmentFileMock } = vi.hoisted(() => ({
  primeExternalGrantMock: vi.fn(),
  releaseExternalFileAccessMock: vi.fn(),
  resolveTurnAttachmentFileMock: vi.fn(),
}));

vi.mock('@/api', () => ({
  releaseExternalFileAccess: releaseExternalFileAccessMock,
  resolveTurnAttachmentFile: resolveTurnAttachmentFileMock,
}));

vi.mock('@/components/workspace/files/file-content-store', () => ({
  fileContentStore: { primeExternalGrant: primeExternalGrantMock },
}));

vi.mock('@/components/workspace/files/FileWorkspacePanel', () => ({
  FileContent: ({ resource }: { resource: { kind: string; key: string; locator: { canonicalPath: string } } }) => (
    <output data-kind={resource.kind} data-key={resource.key} data-path={resource.locator.canonicalPath} />
  ),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const resource: TurnAttachmentWorkspaceResource = {
  kind: 'turn-attachment',
  key: 'turn-attachment:set-1:attachment-1',
  scopeKey: 'conversation:project-1:task-1:run-1',
  title: 'report.md',
  description: 'report.md',
  attention: false,
  locator: {
    projectId: 'project-1',
    taskId: 'task-1',
    runId: 'run-1',
    roundId: 'round-1',
    nodeId: 'node-1',
    attemptId: 'attempt-1',
    branchId: 'root',
  },
  changeSetId: 'set-1',
  attachmentId: 'attachment-1',
};

beforeEach(() => {
  primeExternalGrantMock.mockReset();
  releaseExternalFileAccessMock.mockReset();
  resolveTurnAttachmentFileMock.mockReset();
  resolveTurnAttachmentFileMock.mockResolvedValue({
    locator: {
      projectId: 'project-1',
      canonicalPath: 'C:/attempt/attachments/report.md',
      relativePath: null,
      scope: 'external',
    },
    target: null,
    externalAccessGrant: {
      token: 'grant-1',
      permissions: ['read', 'write'],
      expiresAtMs: '9999999999999',
    },
  });
});

afterEach(() => {
  document.body.replaceChildren();
});

describe('turn attachment workspace', () => {
  it('resolves the manifest identity, primes the exact grant, and reuses editable file content', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<TurnAttachmentWorkspacePanel resource={resource} />);
        await Promise.resolve();
      });

      expect(resolveTurnAttachmentFileMock).toHaveBeenCalledWith(
        resource.locator,
        resource.changeSetId,
        resource.attachmentId,
      );
      expect(primeExternalGrantMock).toHaveBeenCalledWith(
        resource.key,
        'project-1',
        'C:/attempt/attachments/report.md',
        expect.objectContaining({ token: 'grant-1' }),
      );
      const output = container.querySelector('output');
      expect(output?.dataset.kind).toBe('file');
      expect(output?.dataset.key).toBe(resource.key);
      expect(output?.dataset.path).toBe('C:/attempt/attachments/report.md');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps Markdown render/source editing on the shared file editor contract', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'web/src/components/workspace/files/FileWorkspacePanel.tsx'),
      'utf8',
    );
    expect(source).toContain('export function FileContent');
    expect(source).toContain('<WorkspaceFileEditor');
    expect(source).toContain("markdownMode(resource.key)");
    expect(source).toContain('onChange={(content) => fileContentStore.updateText(resource.key, content)}');
    expect(source).toContain('onSave={() => void fileContentStore.flush(resource.key)}');
    const storeSource = readFileSync(
      resolve(process.cwd(), 'web/src/components/workspace/files/file-content-store.ts'),
      'utf8',
    );
    expect(storeSource).toContain("markdownMode ?? 'live-preview'");
  });
});
