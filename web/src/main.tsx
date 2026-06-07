import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { resolveThemePreference } from './theme';
import './i18n';
import '@xyflow/react/dist/style.css';
import './styles.css';

// 在 React 渲染前同步恢复主题，避免 splash 画面闪烁
(function applyInitialTheme() {
  const rootEl = document.documentElement;
  const KEY = 'gold-band:preferred-themes';
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    const systemMode = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    const theme = saved[systemMode] ?? (systemMode === 'dark' ? 'dark' : 'light');
    const resolved = resolveThemePreference(theme);
    rootEl.dataset.theme = resolved;
    rootEl.classList.toggle('dark', resolved === 'dark' || resolved === 'black');
  } catch {
    rootEl.dataset.theme = 'dark';
    rootEl.classList.add('dark');
  }
})();

createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
