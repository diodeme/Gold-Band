import type { ImportProfilesResult } from '../types';

export interface ProfileImportState {
  surface: 'closed' | 'settings' | 'result' | 'editing';
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
  | { type: 'edit-failed'; error: string }
  | { type: 'profile-updated'; importedId: string; name: string }
  | { type: 'resume-result' };

export const initialProfileImportState: ProfileImportState = {
  surface: 'closed',
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
      return { ...state, surface: 'settings', result: null, error: null };
    case 'close-settings':
      return { ...state, surface: 'closed', error: null };
    case 'set-dynamic-template':
      return { ...state, dynamicTemplate: action.enabled };
    case 'begin-import':
      return { ...state, surface: 'settings', importing: true, error: null };
    case 'cancel-import':
      return { ...state, surface: 'settings', importing: false };
    case 'import-succeeded':
      return {
        ...state,
        surface: 'result',
        importing: false,
        result: action.result,
        error: null,
      };
    case 'import-failed':
      return {
        ...state,
        surface: 'settings',
        importing: false,
        error: action.error,
      };
    case 'close-result':
      return { ...state, surface: 'closed', result: null, error: null };
    case 'begin-edit':
      return { ...state, surface: state.result ? 'result' : 'closed', error: null };
    case 'edit-succeeded':
      return { ...state, surface: state.result ? 'editing' : 'closed', error: null };
    case 'edit-failed':
      return { ...state, surface: state.result ? 'result' : 'closed', error: action.error };
    case 'profile-updated':
      if (!state.result) return state;
      return {
        ...state,
        result: {
          ...state.result,
          imported: state.result.imported.map((record) => (
            record.importedId === action.importedId
              ? { ...record, name: action.name }
              : record
          )),
        },
      };
    case 'resume-result':
      return { ...state, surface: state.result ? 'result' : 'closed', error: null };
  }
}
