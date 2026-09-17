import { createContext, useContext } from 'react';

export const overlayPortalHostId = 'gold-band-overlay-portal-host';
export const overlayOwnerAttribute = 'data-gold-band-overlay-owner';
export const rightWorkspaceOverlayOwner = 'right-workspace';

export function getOverlayPortalHost(): HTMLElement | undefined {
  if (typeof document === 'undefined') return undefined;
  return document.getElementById(overlayPortalHostId) ?? undefined;
}

export const PortalContainerContext = createContext<HTMLElement | null>(null);

export function usePortalContainer(): HTMLElement | null {
  return useContext(PortalContainerContext);
}
