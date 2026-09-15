import { createContext, useContext } from 'react';

// A presentation boundary; adapters must enforce their own write policy.
export const ReadOnlyExperience = createContext(false);
export function useReadOnlyExperience() {
  return useContext(ReadOnlyExperience);
}
