import { supportedLocaleTag, type SupportedLocaleTag } from '../languages';
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

export type WebviewStartupPlatform = 'windows' | 'macos' | 'other';

interface StartupCopy {
  title: string;
  unsupported: string;
  loadFailed: string;
  unsupportedGuidance: Record<WebviewStartupPlatform, string>;
  loadFailedGuidance: string;
  copy: string;
  copied: string;
}

const STARTUP_COPY: Record<SupportedLocaleTag, StartupCopy> = {
  'zh-CN': {
    title: 'Gold Band 无法在当前 WebView 中启动',
    unsupported: '当前 WebView 缺少应用运行所需的基础能力。',
    loadFailed: '应用资源加载失败。请复制诊断信息并反馈给 Gold Band 支持人员。',
    unsupportedGuidance: {
      windows: 'Gold Band 使用 Microsoft Edge WebView2 运行。请安装或修复最新版 Microsoft Edge WebView2 Runtime，然后重新启动 Gold Band。',
      macos: 'macOS 的 WKWebView 随系统更新，不能通过单独更新 Safari 替换。请先安装这台 Mac 可用的最新 macOS 更新。',
      other: '请更新当前操作系统或 WebView 运行环境，然后重新启动 Gold Band。',
    },
    loadFailedGuidance: '请重新安装最新版 Gold Band 后重试；若问题仍然存在，请复制诊断信息并反馈给 Gold Band 支持人员。',
    copy: '复制诊断信息',
    copied: '已复制',
  },
  'zh-TW': {
    title: 'Gold Band 無法在目前 WebView 中啟動',
    unsupported: '目前 WebView 缺少應用程式執行所需的基礎能力。',
    loadFailed: '應用程式資源載入失敗。請複製診斷資訊並回報給 Gold Band 支援人員。',
    unsupportedGuidance: {
      windows: 'Gold Band 使用 Microsoft Edge WebView2 執行。請安裝或修復最新版 Microsoft Edge WebView2 Runtime，然後重新啟動 Gold Band。',
      macos: 'macOS 的 WKWebView 隨系統更新，不能只靠更新 Safari 替換。請先安裝這台 Mac 可用的最新 macOS 更新。',
      other: '請更新目前作業系統或 WebView 執行環境，然後重新啟動 Gold Band。',
    },
    loadFailedGuidance: '請重新安裝最新版 Gold Band 後再試；若問題仍然存在，請複製診斷資訊並回報給 Gold Band 支援人員。',
    copy: '複製診斷資訊',
    copied: '已複製',
  },
  en: {
    title: 'Gold Band cannot start in this WebView',
    unsupported: 'The current WebView is missing capabilities required to run the application.',
    loadFailed: 'The application bundle failed to load. Copy the diagnostics and contact Gold Band support.',
    unsupportedGuidance: {
      windows: 'Gold Band runs on Microsoft Edge WebView2. Install or repair the latest Microsoft Edge WebView2 Runtime, then restart Gold Band.',
      macos: 'WKWebView is updated with macOS and cannot be replaced by updating Safari alone. Install the latest macOS update available for this Mac.',
      other: 'Update the operating system or WebView runtime, then restart Gold Band.',
    },
    loadFailedGuidance: 'Reinstall the latest version of Gold Band and try again. If the problem persists, copy the diagnostics and contact Gold Band support.',
    copy: 'Copy diagnostics',
    copied: 'Copied',
  },
  'ja-JP': {
    title: 'Gold Band はこの WebView では起動できません',
    unsupported: '現在の WebView に、アプリケーションの実行に必要な基本機能がありません。',
    loadFailed: 'アプリケーションリソースの読み込みに失敗しました。診断情報をコピーして Gold Band サポートへ送ってください。',
    unsupportedGuidance: {
      windows: 'Gold Band は Microsoft Edge WebView2 で動作します。最新の Microsoft Edge WebView2 Runtime をインストールまたは修復してから、Gold Band を再起動してください。',
      macos: 'macOS の WKWebView はシステム更新で更新され、Safari だけを更新しても置き換えられません。この Mac で利用できる最新の macOS 更新を先にインストールしてください。',
      other: 'オペレーティングシステムまたは WebView ランタイムを更新してから、Gold Band を再起動してください。',
    },
    loadFailedGuidance: '最新版の Gold Band を再インストールしてから再試行してください。問題が続く場合は、診断情報をコピーして Gold Band サポートへ送ってください。',
    copy: '診断情報をコピー',
    copied: 'コピーしました',
  },
  'ko-KR': {
    title: 'Gold Band를 현재 WebView에서 시작할 수 없습니다',
    unsupported: '현재 WebView에 애플리케이션 실행에 필요한 기본 기능이 없습니다.',
    loadFailed: '애플리케이션 리소스를 불러오지 못했습니다. 진단 정보를 복사해 Gold Band 지원 담당자에게 보내 주세요.',
    unsupportedGuidance: {
      windows: 'Gold Band는 Microsoft Edge WebView2에서 실행됩니다. 최신 Microsoft Edge WebView2 Runtime을 설치하거나 복구한 다음 Gold Band를 다시 시작하세요.',
      macos: 'macOS의 WKWebView는 시스템 업데이트로 갱신되며 Safari만 업데이트해서 바꿀 수 없습니다. 이 Mac에서 사용할 수 있는 최신 macOS 업데이트를 먼저 설치하세요.',
      other: '운영 체제 또는 WebView 런타임을 업데이트한 다음 Gold Band를 다시 시작하세요.',
    },
    loadFailedGuidance: '최신 버전의 Gold Band를 다시 설치한 후 재시도하세요. 문제가 계속되면 진단 정보를 복사해 Gold Band 지원 담당자에게 보내 주세요.',
    copy: '진단 정보 복사',
    copied: '복사됨',
  },
  'pt-BR': {
    title: 'O Gold Band não pode iniciar neste WebView',
    unsupported: 'O WebView atual não tem os recursos básicos necessários para executar o aplicativo.',
    loadFailed: 'Falha ao carregar os recursos do aplicativo. Copie o diagnóstico e envie ao suporte do Gold Band.',
    unsupportedGuidance: {
      windows: 'O Gold Band é executado no Microsoft Edge WebView2. Instale ou repare o Microsoft Edge WebView2 Runtime mais recente e reinicie o Gold Band.',
      macos: 'O WKWebView do macOS é atualizado com o sistema e não pode ser substituído apenas pela atualização do Safari. Instale primeiro a atualização mais recente do macOS disponível para este Mac.',
      other: 'Atualize o sistema operacional ou o runtime do WebView e reinicie o Gold Band.',
    },
    loadFailedGuidance: 'Reinstale a versão mais recente do Gold Band e tente novamente. Se o problema persistir, copie o diagnóstico e entre em contato com o suporte do Gold Band.',
    copy: 'Copiar diagnóstico',
    copied: 'Copiado',
  },
  es: {
    title: 'Gold Band no puede iniciarse en este WebView',
    unsupported: 'El WebView actual no tiene las capacidades básicas necesarias para ejecutar la aplicación.',
    loadFailed: 'No se pudieron cargar los recursos de la aplicación. Copie el diagnóstico y envíelo al soporte de Gold Band.',
    unsupportedGuidance: {
      windows: 'Gold Band se ejecuta en Microsoft Edge WebView2. Instale o repare el Microsoft Edge WebView2 Runtime más reciente y reinicie Gold Band.',
      macos: 'El WKWebView de macOS se actualiza con el sistema y no se puede sustituir solo actualizando Safari. Instale primero la última actualización de macOS disponible para este Mac.',
      other: 'Actualice el sistema operativo o el runtime de WebView y reinicie Gold Band.',
    },
    loadFailedGuidance: 'Reinstale la versión más reciente de Gold Band e inténtelo de nuevo. Si el problema persiste, copie el diagnóstico y contacte con el soporte de Gold Band.',
    copy: 'Copiar diagnóstico',
    copied: 'Copiado',
  },
};

function startupLocale(language: string): SupportedLocaleTag {
  return supportedLocaleTag(language);
}

export function webviewStartupPlatform(userAgent = navigator.userAgent): WebviewStartupPlatform {
  if (/Windows/u.test(userAgent)) return 'windows';
  if (/Macintosh|Mac OS X/u.test(userAgent)) return 'macos';
  return 'other';
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
  platform = webviewStartupPlatform(),
) {
  const locale = startupLocale(language);
  document.documentElement.lang = locale;
  const copy = STARTUP_COPY[locale];
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
  guidance.textContent = error.code === 'webview.capability.unsupported'
    ? copy.unsupportedGuidance[platform]
    : copy.loadFailedGuidance;

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
