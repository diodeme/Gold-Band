import type { ImportProfilesResult } from '../types';

export interface ProfileImportState {
  settingsOpen: boolean;
  dynamicTemplate: boolean;
  importing: boolean;
  result: ImportProfilesResult | null;
  error: string | null;
}

export type ProfileImportAction =
  | { type: 'open-settings' }
  | { type: 'close-settings' }
  | { type: 'set-dynamic-template'; enabled: boolean }
  | { type: 'begin-import' }
  | { type: 'cancel-import' }
  | { type: 'import-succeeded'; result: ImportProfilesResult }
  | { type: 'import-failed'; error: string }
  | { type: 'close-result' }
  | { type: 'begin-edit' }
  | { type: 'edit-succeeded' }
  | { type: 'edit-failed'; error: string };

export const initialProfileImportState: ProfileImportState = {
  settingsOpen: false,
  dynamicTemplate: false,
  importing: false,
  result: null,
  error: null,
};

export function profileImportReducer(
  state: ProfileImportState,
  action: ProfileImportAction,
): ProfileImportState {
  switch (action.type) {
    case 'open-settings':
      return { ...state, settingsOpen: true, result: null, error: null };
    case 'close-settings':
      return { ...state, settingsOpen: false, error: null };
    case 'set-dynamic-template':
      return { ...state, dynamicTemplate: action.enabled };
    case 'begin-import':
      return { ...state, importing: true, error: null };
    case 'cancel-import':
      return { ...state, importing: false };
    case 'import-succeeded':
      return {
        ...state,
        settingsOpen: false,
        importing: false,
        result: action.result,
        error: null,
      };
    case 'import-failed':
      return {
        ...state,
        settingsOpen: true,
        importing: false,
        error: action.error,
      };
    case 'close-result':
      return { ...state, result: null, error: null };
    case 'begin-edit':
      return { ...state, error: null };
    case 'edit-succeeded':
      return { ...state, result: null, error: null };
    case 'edit-failed':
      return { ...state, error: action.error };
  }
}
