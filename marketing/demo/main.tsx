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
// The embedding site owns language and appearance; the initial values arrive in the URL so the first paint matches.
const requested = new URLSearchParams(location.search);
const requestedLanguage = requested.get('language');
const requestedTheme = requested.get('theme');
const hostLanguage = requestedLanguage === 'zh' ? 'zh-cn' : requestedLanguage === 'en' ? 'en' : null;
const hostColorScheme = requestedTheme === 'dark' || requestedTheme === 'light' ? requestedTheme : null;
if (hostLanguage || hostColorScheme) {
  bootstrap.preferences = await browserApi.saveDesktopPreferences(
    hostColorScheme ? { ...bootstrap.preferences.appearance, colorScheme: hostColorScheme } : bootstrap.preferences.appearance,
    bootstrap.preferences.personalization, hostLanguage ?? bootstrap.preferences.language,
    bootstrap.preferences.useLocalClaude, bootstrap.preferences.verboseLogging);
}
applyAppearance(bootstrap.preferences.appearance);
applyPersonalization(bootstrap.preferences.personalization);
await i18n.changeLanguage(i18nLanguage(bootstrap.preferences.language));
const root = createRoot(document.getElementById('root')!);
root.render(<AppProviders><ReadOnlyExperience.Provider value={true}><DemoApp bootstrap={bootstrap} layoutPreferences={sidebar.preferences} /></ReadOnlyExperience.Provider></AppProviders>);
const disconnectHost = connectDemoHost(appearance => window.dispatchEvent(new CustomEvent('gold-band.demo.host-appearance', { detail: appearance })));
if (import.meta.hot) import.meta.hot.dispose(() => { disconnectHost(); root.unmount(); });
