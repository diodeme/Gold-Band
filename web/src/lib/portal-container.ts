import { createContext, useContext, useMemo } from 'react';

export const overlayPortalHostId = 'gold-band-overlay-portal-host';
export const overlayOwnerAttribute = 'data-gold-band-overlay-owner';
export const rightWorkspaceOverlayOwner = 'right-workspace';
export const overlayCollisionBoundaryAttribute = 'data-overlay-collision-boundary';
export const conversationOverlayCollisionBoundary = 'conversation';
export const overlayCollisionPadding = 8;
export const overlayCollisionMaxWidthClassName =
  'max-w-[min(100%,var(--radix-popper-available-width))]';

export type OverlayCollisionPadding =
  | number
  | Partial<Record<'top' | 'right' | 'bottom' | 'left', number>>;

export function getOverlayPortalHost(): HTMLElement | undefined {
  if (typeof document === 'undefined') return undefined;
  return document.getElementById(overlayPortalHostId) ?? undefined;
}

export const PortalContainerContext = createContext<HTMLElement | null>(null);

export function usePortalContainer(): HTMLElement | null {
  return useContext(PortalContainerContext);
}

export const CollisionBoundaryContext = createContext<HTMLElement | null>(null);

export function useCollisionBoundary(): HTMLElement | null {
  return useContext(CollisionBoundaryContext);
}

export function resolveOverlayCollisionBoundary(
  explicit: HTMLElement | null | undefined,
  context: HTMLElement | null,
): HTMLElement | undefined {
  if (explicit === undefined) return context ?? undefined;
  return explicit ?? undefined;
}

const EMPTY_COLLISION_PROPS: Record<string, never> = {};

export function useOverlayPositioning(
  explicitBoundary?: HTMLElement | null,
  explicitPadding?: OverlayCollisionPadding,
) {
  const contextBoundary = useCollisionBoundary();
  const collisionBoundary = resolveOverlayCollisionBoundary(explicitBoundary, contextBoundary);
  const collisionPadding = explicitPadding === undefined && collisionBoundary
    ? overlayCollisionPadding
    : explicitPadding;
  return useMemo(() => ({
    collisionBoundary,
    collisionPadding,
    constrainWidth: Boolean(collisionBoundary),
    constraintBoundary: collisionBoundary ? conversationOverlayCollisionBoundary : undefined,
    collisionProps: collisionBoundary
      ? { collisionBoundary, collisionPadding }
      : EMPTY_COLLISION_PROPS,
  }), [collisionBoundary, collisionPadding]);
}
