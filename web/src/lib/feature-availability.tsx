import { createContext, useContext, type ReactNode } from 'react';

export interface FeatureAvailability {
  /** Remote server-connections + remote SSH are intranet-only. The local terminal
   *  is available in every channel; the external/public channel disables remote
   *  connections, so those surfaces must be hidden and the backend rejects them. */
  remoteConnectionsEnabled: boolean;
}

const FeatureAvailabilityContext = createContext<FeatureAvailability>({
  remoteConnectionsEnabled: false,
});

export function FeatureAvailabilityProvider({
  remoteConnectionsEnabled,
  children,
}: {
  remoteConnectionsEnabled: boolean;
  children: ReactNode;
}) {
  return (
    <FeatureAvailabilityContext.Provider value={{ remoteConnectionsEnabled }}>
      {children}
    </FeatureAvailabilityContext.Provider>
  );
}

export function useFeatureAvailability(): FeatureAvailability {
  return useContext(FeatureAvailabilityContext);
}
