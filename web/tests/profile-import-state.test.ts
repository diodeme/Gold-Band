import { describe, expect, it } from 'vitest';

import {
  initialProfileImportState,
  profileImportReducer,
} from '../src/lib/profile-import-state';
import type { ImportProfilesResult } from '../src/types';

const result: ImportProfilesResult = {
  totalScanned: 3,
  imported: [
    {
      sourcePath: 'D:/roles/writer.md',
      status: 'imported-with-fallbacks',
      name: 'Writer',
      fallbacks: ['summary'],
      importedId: 'profile-writer',
      error: null,
    },
    {
      sourcePath: 'D:/roles/reviewer.md',
      status: 'imported',
      name: 'Reviewer',
      fallbacks: [],
      importedId: 'profile-reviewer',
      error: null,
    },
  ],
  failed: [{
    sourcePath: 'D:/roles/broken.md',
    status: 'failed',
    name: 'broken',
    fallbacks: [],
    importedId: null,
    error: { code: 'invalid-frontmatter' },
  }],
  truncated: false,
};

describe('profile import state', () => {
  it('keeps the settings sheet open so import failures remain visible', () => {
    const opened = profileImportReducer(initialProfileImportState, { type: 'open-settings' });
    const importing = profileImportReducer(opened, { type: 'begin-import' });
    const failed = profileImportReducer(importing, {
      type: 'import-failed',
      error: 'Selected folder could not be read',
    });

    expect(failed).toMatchObject({
      surface: 'settings',
      importing: false,
      result: null,
      error: 'Selected folder could not be read',
    });
  });

  it('moves successful imports to the result sheet and surfaces edit failures there', () => {
    const importing = profileImportReducer(
      profileImportReducer(initialProfileImportState, { type: 'open-settings' }),
      { type: 'begin-import' },
    );
    const succeeded = profileImportReducer(importing, { type: 'import-succeeded', result });
    const editFailed = profileImportReducer(
      profileImportReducer(succeeded, { type: 'begin-edit' }),
      { type: 'edit-failed', error: 'Profile could not be loaded' },
    );

    expect(succeeded).toMatchObject({ surface: 'result', result, error: null });
    expect(editFailed).toMatchObject({ surface: 'result', result, error: 'Profile could not be loaded' });
  });

  it('preserves the import result while editing and returns to it afterwards', () => {
    const succeeded = profileImportReducer(initialProfileImportState, {
      type: 'import-succeeded',
      result,
    });
    const editing = profileImportReducer(succeeded, { type: 'edit-succeeded' });
    const resumed = profileImportReducer(editing, { type: 'resume-result' });

    expect(editing).toMatchObject({
      surface: 'editing',
      result,
      error: null,
    });
    expect(resumed).toMatchObject({
      surface: 'result',
      result,
      error: null,
    });
  });

  it('synchronizes a saved profile name into its import record without losing diagnostics', () => {
    const editing = profileImportReducer(
      profileImportReducer(initialProfileImportState, {
        type: 'import-succeeded',
        result,
      }),
      { type: 'edit-succeeded' },
    );
    const updated = profileImportReducer(editing, {
      type: 'profile-updated',
      importedId: 'profile-writer',
      name: 'Technical Writer',
    });

    expect(updated.surface).toBe('editing');
    expect(updated.result?.imported).toEqual([
      {
        sourcePath: 'D:/roles/writer.md',
        status: 'imported-with-fallbacks',
        name: 'Technical Writer',
        fallbacks: ['summary'],
        importedId: 'profile-writer',
        error: null,
      },
      result.imported[1],
    ]);
    expect(updated.result?.failed).toBe(result.failed);

    const resumed = profileImportReducer(updated, { type: 'resume-result' });
    expect(resumed).toMatchObject({
      surface: 'result',
      result: updated.result,
      error: null,
    });
  });
});
