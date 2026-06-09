import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './i18n';
import '@xyflow/react/dist/style.css';
import './styles.css';

createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
