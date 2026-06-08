import { useEffect, useState } from 'react';

export type SplashPhase = 'checking' | 'downloading' | 'installing' | 'done';

interface StartupSplashProps {
  phase: SplashPhase;
  progress: { downloaded: number; total: number | null };
  version?: string | null;
}

export function StartupSplash({ phase, progress, version }: StartupSplashProps) {
  const [visible, setVisible] = useState(true);

  useEffect(() => {
    if (phase === 'checking') return;
    // Keep visible for download/install phases
  }, [phase]);

  // When phase transitions away from 'checking', ensure we're visible
  const progressPct = progress.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : 0;
  const progressLabel = progress.total
    ? `${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}`
    : '';

  return (
    <div
      className={`fixed inset-0 z-[100] flex flex-col items-center justify-center bg-background transition-opacity duration-300 ${
        visible ? 'opacity-100' : 'opacity-0 pointer-events-none'
      }`}
    >
      <div className="flex flex-col items-center gap-6 max-w-sm mx-auto px-8">
        {/* Logo area */}
        <div className="flex flex-col items-center gap-3">
          <div className="flex size-16 items-center justify-center rounded-2xl bg-primary/10">
            <svg width="32" height="32" viewBox="0 0 32 32" fill="none" className="text-primary">
              <path d="M16 2L6 8v8c0 5.5 4.3 10.7 10 12 5.7-1.3 10-6.5 10-12V8L16 2z" stroke="currentColor" strokeWidth="2" fill="none" />
              <path d="M12 16l3 3 5-5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
          <span className="text-xl font-semibold tracking-tight">Gold Band</span>
        </div>

        {/* Content area */}
        {phase === 'checking' && (
          <div className="flex flex-col items-center gap-2">
            <p className="text-sm text-muted-foreground">正在检查更新...</p>
          </div>
        )}

        {phase === 'downloading' && (
          <div className="flex w-full flex-col items-center gap-3">
            <p className="text-sm font-medium">
              {version ? `正在下载安全更新 v${version}` : '正在下载安全更新...'}
            </p>
            <div className="h-2 w-full overflow-hidden rounded-full bg-secondary">
              <div
                className="h-full rounded-full bg-primary transition-all duration-300 ease-out"
                style={{ width: `${progressPct}%` }}
              />
            </div>
            <div className="flex w-full items-center justify-between text-xs text-muted-foreground">
              <span>{progressPct}%</span>
              {progressLabel && <span>{progressLabel}</span>}
            </div>
            <p className="text-xs text-muted-foreground">请勿关闭应用...</p>
          </div>
        )}

        {phase === 'installing' && (
          <div className="flex flex-col items-center gap-2">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" className="text-green-500">
              <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="2" />
              <path d="M8 12l3 3 5-5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
            <p className="text-sm font-medium">更新已下载完成</p>
            <p className="text-xs text-muted-foreground">正在安装，应用即将重启...</p>
          </div>
        )}
      </div>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
