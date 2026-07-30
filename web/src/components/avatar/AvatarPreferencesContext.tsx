import { createContext, useContext, type ReactNode } from 'react';
import type { AvatarPreferencesVm } from '@/types';
import { createDefaultAvatarPreferences } from '@/lib/avatar';

const AvatarPreferencesContext = createContext<AvatarPreferencesVm>(createDefaultAvatarPreferences());

export function AvatarPreferencesProvider({ preferences, children }: { preferences: AvatarPreferencesVm; children: ReactNode }) {
  return (
    <AvatarPreferencesContext.Provider value={preferences}>
      {children}
    </AvatarPreferencesContext.Provider>
  );
}

export function useAvatarPreferences() {
  return useContext(AvatarPreferencesContext);
}
