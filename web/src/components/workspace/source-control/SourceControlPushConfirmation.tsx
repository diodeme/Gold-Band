import { useEffect, useState } from 'react';
import { LoaderCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { prepareGitPush } from '@/api';
import { Button } from '@/components/ui/button';
import { DialogFooter } from '@/components/ui/dialog';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import type { GitPreparedPushVm, GitPushIntentVm } from '@/types';
import { rememberPreferredGitRemote } from './source-control-preferences';

type Props = {
  projectId: string;
  workspacePath?: string | null;
  commonDir: string;
  intent: GitPushIntentVm;
  remotes: string[];
  busy: boolean;
  onCancel: () => void;
  onConfirm: (prepared: GitPreparedPushVm) => void;
};

type Preparation =
  | { status: 'loading' }
  | { status: 'error'; code: string }
  | { status: 'ready'; prepared: GitPreparedPushVm };

// A confirmation belongs to one scope and intent; remounting discards its transient result.
export function SourceControlPushConfirmation(props: Props) {
  const { projectId, workspacePath, intent } = props;
  return <PushConfirmationContent key={JSON.stringify([projectId, workspacePath, intent.kind, intent.branch, intent.remote])} {...props} />;
}

function PushConfirmationContent({ projectId, workspacePath, commonDir, intent, remotes, busy, onCancel, onConfirm }: Props) {
  const { t } = useTranslation();
  const [remote, setRemote] = useState(intent.remote);
  const [attempt, setAttempt] = useState(0);
  const [preparation, setPreparation] = useState<Preparation>({ status: 'loading' });
  const [submitted, setSubmitted] = useState(false);
  const { kind, branch } = intent;

  useEffect(() => {
    let cancelled = false;
    const input: GitPushIntentVm = kind === 'publish-head'
      ? { kind, branch, remote: remote! }
      : { kind, branch, remote };
    void (async () => {
      try {
        const prepared = await prepareGitPush(projectId, workspacePath, input);
        if (!cancelled) setPreparation({ status: 'ready', prepared });
      } catch (reason) {
        if (!cancelled) setPreparation({
          status: 'error',
          code: typeof reason === 'object' && reason !== null && 'code' in reason && typeof reason.code === 'string'
            ? reason.code : 'git.operation-failed',
        });
      }
    })();
    return () => { cancelled = true; };
  }, [attempt, branch, kind, projectId, remote, workspacePath]);

  const prepared = preparation.status === 'ready' ? preparation.prepared : null;
  const chooseRemote = (value: string) => {
    setPreparation({ status: 'loading' });
    setRemote(value);
    rememberPreferredGitRemote(commonDir, value);
  };
  return (
    <div className="grid min-w-0 gap-4" data-source-control-push-confirmation>
      {preparation.status === 'loading' ? <div role="status" className="flex items-center gap-2 text-sm text-muted-foreground"><LoaderCircle className="size-4 shrink-0 animate-spin" />{t('sourceControl.pushPreparing')}</div> : null}
      {preparation.status === 'error' ? <div role="alert" className="grid gap-2"><p className="break-words text-sm text-destructive">{t(`errors.${preparation.code}`, { defaultValue: t('sourceControl.loadFailed') })}</p><Button variant="outline" className="justify-self-start" onClick={() => { setPreparation({ status: 'loading' }); setAttempt((value) => value + 1); }}>{t('sourceControl.checkAgain')}</Button></div> : null}
      {prepared ? <>
        <p className="min-w-0 break-all text-sm leading-relaxed" data-push-target>
          <span data-push-source-branch>{prepared.target.branch}</span>{' → '}<span data-push-target-remote>{prepared.target.remote}</span>/<span data-push-target-branch>{prepared.target.remoteBranch}</span>
        </p>
        {kind === 'upstream-sync' && prepared.target.setUpstream ? <Select value={prepared.target.remote} disabled={busy || submitted} onValueChange={chooseRemote}><SelectTrigger className="w-full" aria-label={t('sourceControl.pushTargetRemote')}><SelectValue /></SelectTrigger><SelectContent>{remotes.map((name) => <SelectItem key={name} value={name}>{name}</SelectItem>)}</SelectContent></Select> : null}
        {prepared.target.setUpstream ? <p className="text-sm text-muted-foreground">{t('sourceControl.pushSetUpstream')}</p> : null}
      </> : null}
      <DialogFooter>
        <Button variant="outline" onClick={onCancel}>{t('common.cancel')}</Button>
        <Button disabled={!prepared || busy || submitted} onClick={() => { if (prepared && !busy && !submitted) { setSubmitted(true); onConfirm(prepared); } }}>{t('common.confirm')}</Button>
      </DialogFooter>
    </div>
  );
}
