import { describe, expect, it } from 'vitest';
import { browserApi } from '../../src/api/browser';

describe('browserApi', () => {
  it('keeps the current preview workspace in the recent list', async () => {
    const bootstrap = await browserApi.getAppBootstrap();

    await expect(browserApi.removeRecentWorkspace(bootstrap.repoRoot)).rejects.toEqual({
      code: 'workspace.recent-current-locked',
      params: { workspace: bootstrap.repoRoot },
    });
  });

  it('keeps built-in profiles readonly in preview mode', async () => {
    const builtIn = (await browserApi.getProfiles()).profiles.find((profile) => profile.isBuiltIn);

    expect(builtIn).toBeDefined();
    await expect(browserApi.deleteProfile(builtIn!.id)).rejects.toEqual({
      code: 'profile.readonly-built-in',
      params: {},
    });
  });

  it('requires explicit force before deleting confirmation-gated preview profiles', async () => {
    const created = await browserApi.createProfile({
      name: 'Needs confirmation',
      summary: 'preview role [requires-confirmation]',
      content: 'temp',
      dynamicTemplate: true,
    });
    expect(created.scope).toBe('user');
    expect(created.dynamicTemplate).toBe(true);

    await expect(browserApi.deleteProfile(created.id)).rejects.toEqual({
      code: 'profile.delete-confirmation-required',
      params: {
        templateCount: 1,
        taskCount: 1,
        runCount: 0,
      },
    });

    const list = await browserApi.deleteProfile(created.id, true);
    expect(list.profiles.some((profile) => profile.id === created.id)).toBe(false);
  });

  it('enforces and rotates single-file grants for external preview files', async () => {
    const resolved = await browserApi.resolveWorkspaceFileLink('default', 'D:/outside/external.txt');
    const grant = resolved.externalAccessGrant!;
    const snapshot = await browserApi.readFileResource(
      'default',
      resolved.locator.canonicalPath,
      grant.token,
    );
    expect(snapshot.kind).toBe('text');

    await expect(browserApi.writeFileResource({
      projectId: 'default',
      canonicalPath: resolved.locator.canonicalPath,
      externalAccessToken: null,
      content: 'denied',
      encoding: 'utf-8',
      lineEnding: 'lf',
      expectedRevision: snapshot.revision,
      operationId: 'denied-write',
      force: false,
    })).rejects.toMatchObject({ code: 'workspace-file.external-access-denied' });

    const renewed = await browserApi.renewExternalFileAccess(grant.token);
    expect(renewed.token).not.toBe(grant.token);
    await expect(browserApi.readFileResource(
      'default',
      resolved.locator.canonicalPath,
      grant.token,
    )).rejects.toMatchObject({ code: 'workspace-file.external-access-denied' });
    await expect(browserApi.readFileResource(
      'default',
      resolved.locator.canonicalPath,
      renewed.token,
    )).resolves.toMatchObject({ kind: 'text' });
  });

  it('preserves the line target when resolving a conversation file link', async () => {
    const resolved = await browserApi.resolveWorkspaceFileLink('default', 'README.md:47');

    expect(resolved.locator.relativePath).toBe('README.md');
    expect(resolved.target).toEqual({ line: 47, column: null, endLine: null });
  });

  it('returns the shared comparison contract for GitHub pull request files', async () => {
    const comparison = await browserApi.getGitComparison('default', {
      kind: 'github-pr',
      workspacePath: '/preview/gold-band',
      host: 'github.com',
      repository: 'gold-band/desktop',
      prNumber: 42,
      path: 'src/source-control.ts',
    });

    expect(comparison).toMatchObject({
      path: 'src/source-control.ts',
      stats: { addedLines: 4, deletedLines: 1 },
      limitationCode: null,
    });
    expect(comparison.after?.content).toContain('gitHubPullRequests');
  });
});
