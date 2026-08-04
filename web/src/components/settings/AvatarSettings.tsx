import { lazy, Suspense, useRef, useState } from 'react';
import type { Area, Point } from 'react-easy-crop';
import 'react-easy-crop/react-easy-crop.css';
import { useTranslation } from 'react-i18next';
import { ImagePlus, Loader2, Maximize, Minus, UserRound } from 'lucide-react';
import type { AvatarKind, AvatarPreferencesVm, AvatarProfileVm, AvatarShape, SaveDesktopAvatarInput } from '@/types';
import { AvatarDisplay } from '@/components/avatar/AvatarDisplay';
import { Avatar, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Slider } from '@/components/ui/slider';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { avatarShapeClass } from '@/lib/avatar';
import { cropAvatarImage, readAvatarFile } from '@/lib/avatar-image';
import { cn } from '@/lib/utils';

const Cropper = lazy(() => import('react-easy-crop'));

interface AvatarSettingsProps {
  preferences: AvatarPreferencesVm;
  busy: boolean;
  onSaveAvatar: (input: SaveDesktopAvatarInput) => Promise<AvatarPreferencesVm | undefined>;
  onSelectRecentAvatar: (kind: AvatarKind, avatarId: string) => Promise<AvatarPreferencesVm | undefined>;
  onSaveAvatarShape: (kind: AvatarKind, shape: AvatarShape) => Promise<AvatarPreferencesVm | undefined>;
  onClearAvatar: (kind: AvatarKind) => Promise<AvatarPreferencesVm | undefined>;
}

export function AvatarSettings({ preferences, busy, onSaveAvatar, onSelectRecentAvatar, onSaveAvatarShape, onClearAvatar }: AvatarSettingsProps) {
  return (
    <div className="grid gap-3 @5xl/settings-content:grid-cols-2">
      <AvatarEditor
        kind="agent"
        profile={preferences.agent}
        busy={busy}
        onSaveAvatar={onSaveAvatar}
        onSelectRecentAvatar={onSelectRecentAvatar}
        onSaveAvatarShape={onSaveAvatarShape}
        onClearAvatar={onClearAvatar}
      />
      <AvatarEditor
        kind="user"
        profile={preferences.user}
        busy={busy}
        onSaveAvatar={onSaveAvatar}
        onSelectRecentAvatar={onSelectRecentAvatar}
        onSaveAvatarShape={onSaveAvatarShape}
        onClearAvatar={onClearAvatar}
      />
    </div>
  );
}

interface AvatarEditorProps {
  kind: AvatarKind;
  profile: AvatarProfileVm;
  busy: boolean;
  onSaveAvatar: AvatarSettingsProps['onSaveAvatar'];
  onSelectRecentAvatar: AvatarSettingsProps['onSelectRecentAvatar'];
  onSaveAvatarShape: AvatarSettingsProps['onSaveAvatarShape'];
  onClearAvatar: AvatarSettingsProps['onClearAvatar'];
}

function AvatarEditor({ kind, profile, busy, onSaveAvatar, onSelectRecentAvatar, onSaveAvatarShape, onClearAvatar }: AvatarEditorProps) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const [source, setSource] = useState<string | null>(null);
  const [crop, setCrop] = useState<Point>({ x: 0, y: 0 });
  const [zoom, setZoom] = useState(1);
  const [croppedArea, setCroppedArea] = useState<Area | null>(null);
  const [saving, setSaving] = useState(false);
  const updatingRef = useRef(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const title = t(`settings.avatar.${kind}.title`);
  const disabled = busy || saving;

  const openUpload = () => inputRef.current?.click();

  const handleFileChange = async (file?: File) => {
    if (!file) return;
    setLocalError(null);
    try {
      setSource(await readAvatarFile(file));
      setCrop({ x: 0, y: 0 });
      setZoom(1);
      setCroppedArea(null);
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : 'avatar.invalid-image-data');
    } finally {
      if (inputRef.current) inputRef.current.value = '';
    }
  };

  const saveCrop = async () => {
    if (!source || !croppedArea) return;
    setSaving(true);
    setLocalError(null);
    try {
      const image = await cropAvatarImage(source, croppedArea);
      const saved = await onSaveAvatar({ kind, shape: profile.shape, ...image });
      if (saved) setSource(null);
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : 'avatar.crop-failed');
    } finally {
      setSaving(false);
    }
  };

  const updateProfile = async (action: () => Promise<AvatarPreferencesVm | undefined>) => {
    if (updatingRef.current) return;
    updatingRef.current = true;
    try {
      await action();
    } finally {
      updatingRef.current = false;
    }
  };

  return (
    <div data-testid={`avatar-editor-${kind}`} className="@container/avatar-editor grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-center gap-x-3 gap-y-2 rounded-lg border border-border/40 px-3 py-3 @xl/avatar-editor:grid-cols-[auto_minmax(0,1fr)_auto]">
      <div className="group/avatar relative row-span-2 shrink-0 @xl/avatar-editor:row-span-1">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className="group relative shrink-0 rounded-lg outline-none ring-offset-background focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              aria-label={t('settings.avatar.openMenu', { type: title })}
              disabled={disabled}
            >
              <AvatarDisplay kind={kind} profile={profile} className="size-12 transition group-hover:brightness-90" fallbackClassName="bg-muted/55" />
              <span className="absolute -bottom-0.5 -right-0.5 flex size-5 items-center justify-center rounded-full border border-background bg-primary text-primary-foreground shadow-sm">
                <ImagePlus className="size-3" />
              </span>
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" sideOffset={6} className="w-72 p-2">
            <DropdownMenuLabel className="px-1 pb-2 text-xs text-muted-foreground">
              {t('settings.avatar.recent')}
            </DropdownMenuLabel>
            {profile.recentAvatars.length > 0 ? (
              <div className="grid grid-cols-5 gap-1.5 px-1 pb-1">
                {profile.recentAvatars.map((avatar) => (
                  <DropdownMenuItem
                    key={avatar.id}
                    className={cn(
                      'h-11 justify-center p-1',
                      avatar.id === profile.selectedAvatarId && 'bg-accent ring-1 ring-primary/60',
                    )}
                    aria-label={t('settings.avatar.useRecent')}
                    onSelect={() => void updateProfile(() => onSelectRecentAvatar(kind, avatar.id))}
                  >
                    <Avatar className={cn('size-9', avatarShapeClass(profile.shape))}>
                      <AvatarImage src={avatar.dataUrl} alt="" className="object-cover" />
                    </Avatar>
                  </DropdownMenuItem>
                ))}
              </div>
            ) : (
              <div className="px-2 pb-2 text-xs text-muted-foreground">{t('settings.avatar.noRecent')}</div>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={openUpload}>
              <ImagePlus />
              {t('settings.avatar.upload')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        {profile.selectedAvatarId ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="secondary"
                size="icon-xs"
                className="pointer-events-none absolute -right-1 -top-1 z-10 size-[18px] rounded-full border border-background bg-muted-foreground text-background opacity-0 shadow-sm transition-[color,background-color,opacity] hover:bg-destructive hover:text-white group-hover/avatar:pointer-events-auto group-hover/avatar:opacity-100 focus-visible:pointer-events-auto focus-visible:opacity-100"
                aria-label={t('settings.avatar.remove', { type: title })}
                disabled={disabled}
                onClick={() => void updateProfile(() => onClearAvatar(kind))}
              >
                <Minus className="size-2.5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="top">{t('settings.avatar.remove', { type: title })}</TooltipContent>
          </Tooltip>
        ) : null}
        <input
          ref={inputRef}
          type="file"
          accept="image/png,image/jpeg,image/webp"
          className="hidden"
          onChange={(event) => void handleFileChange(event.target.files?.[0])}
        />
      </div>

      <div className="min-w-0 space-y-0.5">
        <div className="truncate text-sm font-semibold">{title}</div>
        <div className="text-xs text-muted-foreground">{t(`settings.avatar.${kind}.description`)}</div>
      </div>

      <div className="col-start-2 flex flex-wrap gap-1.5 @xl/avatar-editor:col-start-3 @xl/avatar-editor:row-start-1">
        <Button
          type="button"
          variant={profile.shape === 'circle' ? 'default' : 'outline'}
          size="sm"
          className="h-8 px-2.5"
          aria-pressed={profile.shape === 'circle'}
          disabled={disabled}
          onClick={() => void updateProfile(() => onSaveAvatarShape(kind, 'circle'))}
        >
          <UserRound />
          {t('settings.avatar.circle')}
        </Button>
        <Button
          type="button"
          variant={profile.shape === 'square' ? 'default' : 'outline'}
          size="sm"
          className="h-8 px-2.5"
          aria-pressed={profile.shape === 'square'}
          disabled={disabled}
          onClick={() => void updateProfile(() => onSaveAvatarShape(kind, 'square'))}
        >
          <Maximize />
          {t('settings.avatar.square')}
        </Button>
      </div>

      {localError ? (
        <div className="col-span-full text-xs text-destructive">{t(`settings.avatar.errors.${localError}`, { defaultValue: t('settings.avatar.errors.fallback') })}</div>
      ) : null}

      <Dialog open={Boolean(source)} onOpenChange={(open) => { if (!open && !saving) setSource(null); }}>
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{t('settings.avatar.cropTitle', { type: title })}</DialogTitle>
            <DialogDescription>{t('settings.avatar.cropDescription')}</DialogDescription>
          </DialogHeader>
          <div className="relative h-[360px] overflow-hidden rounded-xl bg-black/90">
            {source ? (
              <Suspense fallback={<div className="flex size-full items-center justify-center text-white/70"><Loader2 className="size-5 animate-spin" /></div>}>
                <Cropper
                  image={source}
                  crop={crop}
                  zoom={zoom}
                  aspect={1}
                  cropShape={profile.shape === 'circle' ? 'round' : 'rect'}
                  showGrid={profile.shape === 'square'}
                  objectFit="contain"
                  onCropChange={setCrop}
                  onZoomChange={setZoom}
                  onCropComplete={(_area, pixels) => setCroppedArea(pixels)}
                />
              </Suspense>
            ) : null}
          </div>
          <div className="flex items-center gap-3">
            <span className="text-xs text-muted-foreground">{t('settings.avatar.zoom')}</span>
            <Slider
              value={[zoom]}
              min={1}
              max={3}
              step={0.01}
              onValueChange={([value]) => setZoom(value)}
              aria-label={t('settings.avatar.zoom')}
            />
          </div>
          {localError ? (
            <div className="text-xs text-destructive">{t(`settings.avatar.errors.${localError}`, { defaultValue: t('settings.avatar.errors.fallback') })}</div>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" disabled={saving} onClick={() => setSource(null)}>
              {t('common.close')}
            </Button>
            <Button type="button" disabled={saving || !croppedArea} onClick={() => void saveCrop()}>
              {saving ? <Loader2 className="animate-spin" /> : null}
              {t('settings.avatar.apply')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
