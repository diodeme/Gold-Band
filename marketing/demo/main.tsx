import { createRoot } from 'react-dom/client';
import { AppProviders } from '@/components/AppProviders';
import { ReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { applyAppearance, applyPersonalization } from '@/theme';
import i18n, { i18nLanguage } from '@/i18n';
import { initializeWebviewEnvironment, applyWebviewEnvironmentToDocument } from '@/lib/webview-environment';
import { browserApi } from './runtime';
import { DemoApp } from './DemoApp';
import { connectDemoHost } from './embed';
import '@xyflow/react/dist/style.css';
import '@/styles.css';
import '@/webview-compatibility.css';

applyWebviewEnvironmentToDocument(initializeWebviewEnvironment());
const [bootstrap, sidebar] = await Promise.all([browserApi.getAppBootstrap(), browserApi.getConversationSidebarBootstrap()]);
const requestedLanguage = new URLSearchParams(location.search).get('language');
if (requestedLanguage === 'zh' || requestedLanguage === 'en') {
  bootstrap.preferences = await browserApi.saveDesktopPreferences(bootstrap.preferences.appearance, bootstrap.preferences.personalization, requestedLanguage === 'zh' ? 'zh-cn' : 'en', bootstrap.preferences.useLocalClaude, bootstrap.preferences.verboseLogging);
}
applyAppearance(bootstrap.preferences.appearance);
applyPersonalization(bootstrap.preferences.personalization);
await i18n.changeLanguage(i18nLanguage(bootstrap.preferences.language));
const root = createRoot(document.getElementById('root')!);
root.render(<AppProviders><ReadOnlyExperience.Provider value={true}><DemoApp bootstrap={bootstrap} layoutPreferences={sidebar.preferences} /></ReadOnlyExperience.Provider></AppProviders>);
const disconnectHost = connectDemoHost();
if (import.meta.hot) import.meta.hot.dispose(() => { disconnectHost(); root.unmount(); });
