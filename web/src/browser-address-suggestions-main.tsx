import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import type {
  BrowserAddressSuggestionActionVm,
  BrowserAddressSuggestionOverlayInput,
} from '@/api/client';
import { BrowserAddressSuggestionList } from '@/components/workspace/browser/BrowserAddressSuggestionList';
import './styles.css';

declare global {
  interface Window {
    __GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__?: BrowserAddressSuggestionOverlayInput;
    __goldBandSetBrowserAddressSuggestions?: (state: BrowserAddressSuggestionOverlayInput) => void;
  }
}

function applyTheme(state: BrowserAddressSuggestionOverlayInput) {
  const root = document.documentElement;
  root.classList.toggle('dark', state.theme.dark);
  root.dataset.theme = state.theme.themeId;
  root.dataset.colorScheme = state.theme.colorScheme;
  root.dataset.visualQuality = state.theme.visualQuality;
  root.dataset.materialModel = state.theme.materialModel;
  root.style.colorScheme = state.theme.dark ? 'dark' : 'light';
  for (const [name, value] of Object.entries(state.theme.variables)) {
    root.style.setProperty(name, value);
  }
}

export function BrowserAddressSuggestionsSurface() {
  const [state, setState] = useState(() => window.__GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__);
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const readyRevisionRef = useRef<number | null>(null);
  useEffect(() => {
    window.__goldBandSetBrowserAddressSuggestions = (next) => {
      applyTheme(next);
      setState(next);
      setHoveredIndex(null);
    };
    return () => {
      delete window.__goldBandSetBrowserAddressSuggestions;
    };
  }, []);

  // The child webview stays hidden while it loads. Hidden WebView2 surfaces do not reliably
  // schedule requestAnimationFrame, so waiting for a painted frame creates a readiness
  // deadlock that only the host fallback can break. A layout effect runs after React commits
  // the rows and applies the theme, but before the document can paint; once this synchronous
  // DOM contract is satisfied the native host can reveal the already-loaded surface.
  useLayoutEffect(() => {
    if (!state || readyRevisionRef.current === state.revision) return;
    applyTheme(state);
    readyRevisionRef.current = state.revision;
    void invoke('browser_address_suggestions_ready', {
      input: { revision: state.revision },
    }).catch((error) => {
      console.error('[gb-browser-address-suggestions] ready report failed', error);
    });
  }, [state]);

  if (!state) return null;
  const emitAction = (action: Omit<BrowserAddressSuggestionActionVm, 'revision'>) => {
    // Routed through a validated command instead of a cross-webview emit: the overlay has no
    // diagnostics channel of its own, so the command is the only place where a suggestion
    // click can be audited.
    void invoke('browser_address_suggestion_action', {
      input: {
        ...action,
        revision: state.revision,
      } satisfies BrowserAddressSuggestionActionVm,
    }).catch((error) => {
      console.error('[gb-browser-address-suggestions] suggestion action failed', error);
    });
  };
  return (
    <main className="gold-themed-scrollbar h-dvh overflow-y-auto bg-popover p-1 text-popover-foreground">
      <div role="listbox" aria-label="Browser address suggestions">
        <BrowserAddressSuggestionList
          items={state.items}
          activeIndex={hoveredIndex ?? state.activeIndex}
          onActiveIndexChange={setHoveredIndex}
          onChoose={(key) => emitAction({ kind: 'choose', key })}
          onRemove={(key) => emitAction({ kind: 'remove', key })}
          activateOnPointerDown
        />
      </div>
    </main>
  );
}

const surfaceRoot = typeof document === 'undefined' ? null : document.getElementById('root');
if (surfaceRoot) {
  createRoot(surfaceRoot).render(
    <React.StrictMode><BrowserAddressSuggestionsSurface /></React.StrictMode>,
  );
}
