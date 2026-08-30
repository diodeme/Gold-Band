import { missingCoreWebviewCapabilities } from './webview-feature-policy';
import type { WebviewEnvironmentSnapshot } from './webview-environment';

export type WebviewStartupErrorCode =
  | 'webview.capability.unsupported'
  | 'webview.app_chunk.load_failed';

export interface WebviewStartupError {
  readonly code: WebviewStartupErrorCode;
  readonly msg: string;
  readonly details: Readonly<Record<string, unknown>>;
}

interface StartupCopy {
  title: string;
  unsupported: string;
  loadFailed: string;
  guidance: string;
  copy: string;
  copied: string;
}

const STARTUP_COPY: Record<'zh-CN' | 'en', StartupCopy> = {
  'zh-CN': {
    title: 'Gold Band 无法在当前 WebView 中启动',
    unsupported: '当前系统 WebKit 缺少应用运行所需的基础能力。',
    loadFailed: '应用资源加载失败。请复制诊断信息并反馈给 Gold Band 支持人员。',
    guidance: 'macOS 的 WKWebView 随系统更新，不能通过单独更新 Safari 替换。请先安装这台 Mac 可用的最新 macOS 更新。',
    copy: '复制诊断信息',
    copied: '已复制',
  },
  en: {
    title: 'Gold Band cannot start in this WebView',
    unsupported: 'The system WebKit is missing capabilities required to run the application.',
    loadFailed: 'The application bundle failed to load. Copy the diagnostics and contact Gold Band support.',
    guidance: 'WKWebView is updated with macOS and cannot be replaced by updating Safari alone. Install the latest macOS update available for this Mac.',
    copy: 'Copy diagnostics',
    copied: 'Copied',
  },
};

function startupLocale(language: string) {
  return language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
}

function startupError(
  code: WebviewStartupErrorCode,
  msg: string,
  details: Readonly<Record<string, unknown>>,
): WebviewStartupError {
  return Object.freeze({ code, msg, details: Object.freeze({ ...details }) });
}

export function unsupportedWebviewError(snapshot: WebviewEnvironmentSnapshot) {
  return startupError(
    'webview.capability.unsupported',
    'Required WebView capabilities are unavailable.',
    {
      tier: snapshot.policy.tier,
      missingCapabilities: missingCoreWebviewCapabilities(snapshot.capabilities),
      capabilities: snapshot.capabilities,
    },
  );
}

export function appChunkLoadError(error: unknown, snapshot: WebviewEnvironmentSnapshot) {
  return startupError(
    'webview.app_chunk.load_failed',
    error instanceof Error ? error.message : String(error),
    { tier: snapshot.policy.tier, capabilities: snapshot.capabilities },
  );
}

async function copyDiagnosticText(text: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', 'true');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand('copy');
  textarea.remove();
}

export function renderWebviewStartupError(
  error: WebviewStartupError,
  root: HTMLElement = document.getElementById('root') as HTMLElement,
  language = navigator.language,
) {
  const copy = STARTUP_COPY[startupLocale(language)];
  const shell = document.createElement('main');
  shell.className = 'webview-startup-shell';
  shell.dataset.webviewStartupError = error.code;

  const panel = document.createElement('section');
  panel.className = 'webview-startup-panel';

  const mark = document.createElement('div');
  mark.className = 'webview-startup-mark';
  mark.textContent = 'GB';
  mark.setAttribute('aria-hidden', 'true');

  const title = document.createElement('h1');
  title.textContent = copy.title;

  const summary = document.createElement('p');
  summary.textContent = error.code === 'webview.capability.unsupported' ? copy.unsupported : copy.loadFailed;

  const guidance = document.createElement('p');
  guidance.className = 'webview-startup-guidance';
  guidance.textContent = copy.guidance;

  const diagnostic = document.createElement('pre');
  diagnostic.textContent = JSON.stringify({
    code: error.code,
    msg: error.msg,
    details: error.details,
    userAgent: navigator.userAgent,
  }, null, 2);

  const copyButton = document.createElement('button');
  copyButton.type = 'button';
  copyButton.textContent = copy.copy;
  copyButton.addEventListener('click', () => {
    void copyDiagnosticText(diagnostic.textContent ?? '').then(() => {
      copyButton.textContent = copy.copied;
    }).catch(() => {});
  });

  panel.append(mark, title, summary, guidance, diagnostic, copyButton);
  shell.append(panel);
  root.replaceChildren(shell);
}

export async function startWebviewBootstrap(options: {
  snapshot: WebviewEnvironmentSnapshot;
  loadApp: () => Promise<unknown>;
  renderError?: (error: WebviewStartupError) => void;
}) {
  const renderError = options.renderError ?? renderWebviewStartupError;
  if (options.snapshot.policy.tier === 'unsupported') {
    const error = unsupportedWebviewError(options.snapshot);
    renderError(error);
    return { loaded: false as const, error };
  }
  try {
    await options.loadApp();
    return { loaded: true as const, error: null };
  } catch (cause) {
    const error = appChunkLoadError(cause, options.snapshot);
    renderError(error);
    return { loaded: false as const, error };
  }
}
