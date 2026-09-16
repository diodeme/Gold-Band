import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2, Plus, Save, Trash2, Undo2 } from 'lucide-react';
import { readProjectMemory, writeProjectMemory } from '@/api';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from '@/components/ui/sheet';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { AlertDialog, AlertDialogContent, AlertDialogHeader, AlertDialogTitle, AlertDialogDescription, AlertDialogFooter, AlertDialogAction, AlertDialogCancel } from '@/components/ui/alert-dialog';
import { memoryEntryValid, memoryError, type MemoryEntry, type MemoryRecord, type MemorySnapshot, type MemoryError } from '@/lib/memory';

type Row = { id: string; record: MemoryRecord | null };
type Load = { status: 'loading' } | { status: 'error'; error: MemoryError } | { status: 'ready'; limits: MemorySnapshot['limits']; rows: Row[] };

export function ProjectMemorySheet({ projectId, name, onClose }: { projectId: string; name: string; onClose: () => void }) {
  const { t } = useTranslation();
  const [load, setLoad] = useState<Load>({ status: 'loading' });
  const [confirmClose, setConfirmClose] = useState(false);
  const dirty = useRef(new Set<string>());
  const pending = useRef(new Set<string>());
  const generation = useRef(0);
  async function refresh() {
    const request = ++generation.current;
    setLoad({ status: 'loading' });
    try {
      const snapshot = await readProjectMemory(projectId);
      if (request === generation.current) setLoad({ status: 'ready', limits: snapshot.limits, rows: snapshot.workspace.map(record => ({ id: record.key, record })) });
    } catch (error) { if (request === generation.current) setLoad({ status: 'error', error: memoryError(error) }); }
  }
  function applyAuthoritative(previousId: string, record: MemoryRecord | null, preserveSource = false) {
    dirty.current.delete(previousId);
    setLoad(current => {
      if (current.status !== 'ready') return current;
      if (preserveSource && record) {
        const targetIndex = current.rows.findIndex(item => item.id === record.key);
        if (targetIndex < 0) return { ...current, rows: [...current.rows, { id: record.key, record }] };
        return {
          ...current,
          rows: current.rows.map((item, index) => index === targetIndex ? { id: record.key, record } : item),
        };
      }
      const index = current.rows.findIndex(item => item.id === previousId);
      const remaining = current.rows.filter(item => item.id !== previousId && item.id !== record?.key);
      if (!record) return { ...current, rows: remaining };
      const insertAt = index < 0 ? remaining.length : Math.min(index, remaining.length);
      return {
        ...current,
        rows: [...remaining.slice(0, insertAt), { id: record.key, record }, ...remaining.slice(insertAt)],
      };
    });
  }
  useEffect(() => { void refresh(); return () => { generation.current++; }; }, [projectId]);
  function close() {
    if (pending.current.size) return;
    if (dirty.current.size) setConfirmClose(true); else onClose();
  }
  return <>
    <Sheet open onOpenChange={open => { if (!open) close(); }}>
      <SheetContent className="flex min-h-0 flex-col overflow-hidden" resizeStorageKey="project-memory" minSize={320} defaultSize={560} maxSize={900} closeLabel={t('common.close')}>
        <SheetHeader className="shrink-0">
          <SheetTitle>{t('memory.title')}</SheetTitle>
          <SheetDescription className="break-all">{name}</SheetDescription>
        </SheetHeader>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
          {load.status === 'loading' && <div role="status" className="flex items-center gap-2 py-4"><Loader2 className="size-4 animate-spin" />{t('memory.loading')}</div>}
          {load.status === 'error' && <><MemoryErrorText error={load.error} /><Button variant="outline" onClick={() => void refresh()}>{t('memory.retry')}</Button></>}
          {load.status === 'ready' && <>
            {load.rows.length === 0 && <p className="py-4 text-sm text-muted-foreground">{t('memory.empty')}</p>}
            {load.rows.map(row => <MemoryRow key={row.id} row={row} projectId={projectId} limits={load.limits}
              onDirty={value => { if (value) dirty.current.add(row.id); else dirty.current.delete(row.id); }}
              onPending={value => { if (value) pending.current.add(row.id); else pending.current.delete(row.id); }}
              onSaved={(record, preserveSource) => applyAuthoritative(row.id, record, preserveSource)} />)}
          </>}
        </div>
        <div className="shrink-0 border-t px-4 py-3">
          <Button variant="outline" disabled={load.status !== 'ready' || load.rows.length >= load.limits.entries} onClick={() => {
            const id = crypto.randomUUID(); dirty.current.add(id);
            setLoad(current => current.status === 'ready' ? { ...current, rows: [...current.rows, { id, record: null }] } : current);
          }}><Plus className="size-4" />{t('memory.add')}</Button>
        </div>
      </SheetContent>
    </Sheet>
    <AlertDialog open={confirmClose} onOpenChange={setConfirmClose}>
      <AlertDialogContent><AlertDialogHeader><AlertDialogTitle>{t('memory.unsavedTitle')}</AlertDialogTitle><AlertDialogDescription>{t('memory.unsavedDescription')}</AlertDialogDescription></AlertDialogHeader>
        <AlertDialogFooter><AlertDialogCancel>{t('memory.keepEditing')}</AlertDialogCancel><AlertDialogAction onClick={onClose}>{t('memory.discard')}</AlertDialogAction></AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </>;
}

function MemoryErrorText({ error }: { error: MemoryError }) {
  const { t } = useTranslation();
  return <div role="alert" className="break-all py-2 text-sm text-destructive">{t(`memory.errors.${error.code.replace('memory.', '')}`, { defaultValue: t('memory.errors.io') })}{error.params?.path && <p>{error.params.path}</p>}</div>;
}

function MemoryRow({ row, projectId, limits, onDirty, onPending, onSaved }: {
  row: Row; projectId: string; limits: MemorySnapshot['limits']; onDirty: (dirty: boolean) => void; onPending: (pending: boolean) => void; onSaved: (record: MemoryRecord | null, preserveSource?: boolean) => void;
}) {
  const { t } = useTranslation();
  const [base, setBase] = useState(row.record);
  const [draft, setDraft] = useState<MemoryEntry>(row.record ?? { key: '', value: '', desc: '' });
  const [error, setError] = useState<MemoryError | null>(null);
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);
  const alive = useRef(true);
  const [deleteOpen, setDeleteOpen] = useState(false);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const changed = !base || (['key', 'value', 'desc'] as const).some(field => draft[field] !== base[field]);
  useEffect(() => {
    if (row.record?.revision === base?.revision) return;
    setBase(row.record);
    if (!changed) setDraft(row.record ?? { key: '', value: '', desc: '' });
  }, [row.record?.revision]);
  async function save(deleting = false) {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); onPending(true); setError(null);
    try {
      const snapshot = await writeProjectMemory(projectId, { scope: 'workspace', key: base?.key ?? draft.key, expectedRevision: base?.revision ?? null, entry: deleting ? null : { key: draft.key, value: draft.value, desc: draft.desc } });
      if (!alive.current) return;
      const record = deleting ? null : snapshot.workspace.find(entry => entry.key === draft.key)!;
      setBase(record); if (record) setDraft(record); onSaved(record);
    } catch (cause) { if (alive.current) setError(memoryError(cause)); }
    finally { inFlight.current = false; onPending(false); if (alive.current) setBusy(false); }
  }
  function cancel() {
    setError(null); onDirty(false);
    if (base) setDraft(base); else onSaved(null);
  }
  return <div className="min-w-0 space-y-3 border-b py-4" data-memory-row={row.id}>
    {(['key', 'value', 'desc'] as const).map(field => {
      const id = `memory-${row.id}-${field}`;
      const props = { id, value: draft[field], disabled: busy, className: 'w-full min-w-0', onChange: (event: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
        const next = { ...draft, [field]: event.target.value }; setDraft(next);
        onDirty(!base || (['key', 'value', 'desc'] as const).some(key => next[key] !== base[key]));
      } };
      return <div key={field} className="space-y-1.5"><Label htmlFor={id}>{t(`memory.${field}`)}</Label>{field === 'key' ? <Input {...props} /> : <Textarea {...props} rows={field === 'value' ? 3 : 2} />}{[...draft[field]].length > limits[field] && <p role="alert" className="text-sm text-destructive">{t('memory.errors.field')}</p>}</div>;
    })}
    {error && <MemoryErrorText error={error} />}
    {error?.code === 'memory.conflict' && <div className="space-y-2 text-sm">
      <p className="break-all whitespace-pre-wrap">{t('memory.latest')}: {error.params?.latest ? JSON.stringify({ key: error.params.latest.key, value: error.params.latest.value, desc: error.params.latest.desc }) : t('memory.deleted')}</p>
      <Button variant="outline" size="sm" onClick={() => {
        const latest = error.params?.latest ?? null;
        const targetConflict = error.params?.reason === 'target_exists'
          || Boolean(base && latest && latest.key !== base.key);
        if (targetConflict && base && latest) {
          setDraft(base); setError(null); onDirty(false); onSaved(latest, true);
          return;
        }
        setBase(latest); setDraft(latest ?? { ...draft }); setError(null); onDirty(!latest);
        if (latest) onSaved(latest);
      }}>{t('memory.useLatest')}</Button>
    </div>}
    <div className="flex flex-wrap items-center justify-end gap-2">
      {busy && <Loader2 role="status" aria-label={t('memory.saving')} className="size-4 animate-spin" />}
      <MemoryAction label={t('memory.cancel')} disabled={busy} onClick={cancel}><Undo2 className="size-4" /></MemoryAction>
      <MemoryAction label={t('memory.delete')} disabled={busy} onClick={() => base ? setDeleteOpen(true) : cancel()}><Trash2 className="size-4" /></MemoryAction>
      <MemoryAction label={t('memory.save')} disabled={busy || !changed || !memoryEntryValid(draft, limits) || error?.code === 'memory.conflict'} onClick={() => void save()}><Save className="size-4" /></MemoryAction>
    </div>
    <AlertDialog open={deleteOpen} onOpenChange={setDeleteOpen}><AlertDialogContent>
      <AlertDialogHeader><AlertDialogTitle>{t('memory.deleteTitle')}</AlertDialogTitle><AlertDialogDescription className="break-all">{base?.key}</AlertDialogDescription></AlertDialogHeader>
      <AlertDialogFooter><AlertDialogCancel>{t('memory.cancel')}</AlertDialogCancel><AlertDialogAction onClick={() => void save(true)}>{t('memory.delete')}</AlertDialogAction></AlertDialogFooter>
    </AlertDialogContent></AlertDialog>
  </div>;
}

function MemoryAction({ label, children, ...props }: { label: string; children: React.ReactNode; disabled?: boolean; onClick: () => void }) {
  return <Tooltip><TooltipTrigger asChild><Button type="button" variant="ghost" size="icon" aria-label={label} {...props}>{children}</Button></TooltipTrigger><TooltipContent>{label}</TooltipContent></Tooltip>;
}
