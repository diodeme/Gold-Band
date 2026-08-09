import { describe, expect, it } from 'vitest';

import {
  initialProfileImportState,
  profileImportReducer,
} from '../src/lib/profile-import-state';
import type { ImportProfilesResult } from '../src/types';

const result: ImportProfilesResult = {
  totalScanned: 1,
  imported: [],
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
  it('keeps the settings dialog open so import failures remain visible', () => {
    const opened = profileImportReducer(initialProfileImportState, { type: 'open-settings' });
    const importing = profileImportReducer(opened, { type: 'begin-import' });
    const failed = profileImportReducer(importing, {
      type: 'import-failed',
      error: 'Selected folder could not be read',
    });

    expect(failed).toMatchObject({
      settingsOpen: true,
      importing: false,
      result: null,
      error: 'Selected folder could not be read',
    });
  });

  it('moves successful imports to the result dialog and surfaces edit failures there', () => {
    const importing = profileImportReducer(
      profileImportReducer(initialProfileImportState, { type: 'open-settings' }),
      { type: 'begin-import' },
    );
    const succeeded = profileImportReducer(importing, { type: 'import-succeeded', result });
    const editFailed = profileImportReducer(
      profileImportReducer(succeeded, { type: 'begin-edit' }),
      { type: 'edit-failed', error: 'Profile could not be loaded' },
    );

    expect(succeeded).toMatchObject({ settingsOpen: false, result, error: null });
    expect(editFailed).toMatchObject({ result, error: 'Profile could not be loaded' });
  });

  it('closes the result before opening an imported profile editor', () => {
    const succeeded = profileImportReducer(initialProfileImportState, {
      type: 'import-succeeded',
      result,
    });

    expect(profileImportReducer(succeeded, { type: 'edit-succeeded' })).toMatchObject({
      result: null,
      error: null,
    });
  });
});
