import type { ReactNode } from 'react';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ThemeAssetsProvider } from '@/components/theme/ThemeAssetsContext';

export function AppProviders({ children }: { children: ReactNode }) {
  return <ThemeAssetsProvider><TooltipProvider>{children}</TooltipProvider></ThemeAssetsProvider>;
}
