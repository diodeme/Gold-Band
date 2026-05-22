import { useEffect, useMemo, useState } from 'react';
import { Check, ChevronsUpDown, Edit, Eye, Plus, RefreshCw, Search } from 'lucide-react';
import { useForm } from 'react-hook-form';
import { useTranslation } from 'react-i18next';
import { createProfile, getProfiles, updateProfile } from '../api';
import type { ProfileInput, ProfileListVm, ProfileScope, ProfileVm } from '../types';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Page, PageHeader } from '@/components/PageScaffold';
import { Markdown } from '@/components/prompt-kit/markdown';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card';
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Pagination, PaginationContent, PaginationItem, PaginationLink } from '@/components/ui/pagination';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { cn } from '@/lib/utils';

type ProfileSheetMode = 'view' | 'create' | 'edit';
type ContextTab = 'profiles';
const pageSizes = [6, 12, 24];

export function ContextManagementPage() {
  const { t } = useTranslation();
  const [vm, setVm] = useState<ProfileListVm | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<ContextTab>('profiles');
  const [query, setQuery] = useState('');
  const [scope, setScope] = useState<'all' | ProfileScope>('all');
  const [pageIndex, setPageIndex] = useState(0);
  const [pageSize, setPageSize] = useState(6);
  const [sheetMode, setSheetMode] = useState<ProfileSheetMode | null>(null);
  const [selectedProfile, setSelectedProfile] = useState<ProfileVm | null>(null);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      setVm(await getProfiles());
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const profiles = vm?.profiles ?? [];
  const filteredProfiles = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return profiles.filter((profile) => {
      if (scope !== 'all' && profile.scope !== scope) return false;
      if (!normalizedQuery) return true;
      return profileSearchText(profile).includes(normalizedQuery);
    });
  }, [profiles, query, scope]);
  const pageCount = Math.max(1, Math.ceil(filteredProfiles.length / pageSize));
  const safePageIndex = Math.min(pageIndex, pageCount - 1);
  const pagedProfiles = filteredProfiles.slice(safePageIndex * pageSize, safePageIndex * pageSize + pageSize);

  useEffect(() => {
    if (safePageIndex !== pageIndex) setPageIndex(safePageIndex);
  }, [pageIndex, safePageIndex]);

  const openSheet = (mode: ProfileSheetMode, profile?: ProfileVm) => {
    setSheetMode(mode);
    setSelectedProfile(profile ?? null);
  };

  const saveProfile = async (input: ProfileInput) => {
    if (sheetMode === 'edit' && selectedProfile) {
      await updateProfile(selectedProfile.id, input);
    } else {
      await createProfile(input);
    }
    setSheetMode(null);
    setSelectedProfile(null);
    await refresh();
  };

  return (
    <Page flush className="flex flex-col">
      <PageHeader title={t('contextManagement.title')} />
      <div className="min-h-0 flex-1 p-5 xl:p-6">
        <AppCard className="flex h-full min-h-0 flex-col gap-0 py-0">
          <CardHeader className="border-b px-4 py-3">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as ContextTab)}>
                <TabsList>
                  <TabsTrigger value="profiles">{t('contextManagement.profileManagement')}</TabsTrigger>
                </TabsList>
              </Tabs>
              {activeTab === 'profiles' ? (
                <div className="flex flex-wrap items-center gap-2 sm:justify-end">
                  <Button variant="outline" disabled={loading} onClick={() => void refresh()}>
                    <RefreshCw className={cn(loading && 'animate-spin')} />
                    {t('common.refresh')}
                  </Button>
                  <Button onClick={() => openSheet('create')}><Plus />{t('contextManagement.addProfile')}</Button>
                </div>
              ) : null}
            </div>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col p-0">
            <div className="flex flex-col gap-2 border-b p-4 lg:flex-row lg:items-center">
              <div className="relative min-w-[240px] flex-1">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  className="pl-9"
                  value={query}
                  onChange={(event) => { setQuery(event.target.value); setPageIndex(0); }}
                  placeholder={t('contextManagement.searchPlaceholder')}
                />
              </div>
              <Select value={scope} onValueChange={(value) => { setScope(value as 'all' | ProfileScope); setPageIndex(0); }}>
                <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">{t('common.all')}</SelectItem>
                  <SelectItem value="user">{t('contextManagement.userScope')}</SelectItem>
                  <SelectItem value="project">{t('contextManagement.projectScope')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {error ? <div className="m-4 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</div> : null}
            <ScrollArea className="min-h-0 flex-1">
              {loading && !vm ? <EmptyState>{t('common.loading')}</EmptyState> : null}
              {vm ? (
                <div className="grid gap-3 p-4 md:grid-cols-2 xl:grid-cols-3">
                  {pagedProfiles.map((profile) => <ProfileCard key={`${profile.scope}:${profile.id}`} profile={profile} onView={() => openSheet('view', profile)} onEdit={() => openSheet('edit', profile)} />)}
                </div>
              ) : null}
              {vm && filteredProfiles.length === 0 ? <div className="p-5"><EmptyState>{t('contextManagement.emptyProfiles')}</EmptyState></div> : null}
            </ScrollArea>
            <div className="flex flex-wrap items-center justify-between gap-3 border-t px-4 py-3 text-sm text-muted-foreground">
              <span>{t('common.pageRange', { start: filteredProfiles.length ? safePageIndex * pageSize + 1 : 0, end: Math.min(filteredProfiles.length, (safePageIndex + 1) * pageSize), total: filteredProfiles.length })}</span>
              <div className="flex items-center gap-2">
                <span>{t('common.pageSize')}</span>
                <PageSizePicker value={pageSize} onChange={(value) => { setPageSize(value); setPageIndex(0); }} />
                <ProfilePagination pageIndex={safePageIndex} pageCount={pageCount} onPageChange={setPageIndex} />
              </div>
            </div>
          </CardContent>
        </AppCard>
      </div>
      <ProfileSheet mode={sheetMode} profile={selectedProfile} onOpenChange={(open) => { if (!open) setSheetMode(null); }} onSave={saveProfile} />
    </Page>
  );
}

function ProfileCard({ profile, onView, onEdit }: { profile: ProfileVm; onView: () => void; onEdit: () => void }) {
  const { t } = useTranslation();
  return (
    <Card className="min-h-52 gap-0 bg-card/50 py-0">
      <CardHeader className="px-4 py-4">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <CardTitle className="truncate text-base">{profile.name}</CardTitle>
            <CardDescription className="mt-1 truncate font-mono text-xs">{profile.id}</CardDescription>
          </div>
          <Badge variant="outline">{profile.scope === 'project' ? t('contextManagement.projectScope') : t('contextManagement.userScope')}</Badge>
        </div>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col px-4 pb-0">
        <CardDescription className="line-clamp-3 leading-6">{profile.summary}</CardDescription>
        <dl className="mt-auto grid gap-1 pt-4 text-xs text-muted-foreground">
          <div className="flex gap-1"><dt>{t('contextManagement.createdAt')}:</dt><dd>{profile.createdAt}</dd></div>
          <div className="flex gap-1"><dt>{t('contextManagement.updatedAt')}:</dt><dd>{profile.updatedAt}</dd></div>
        </dl>
      </CardContent>
      <CardFooter className="justify-end gap-2 px-4 py-4">
        <Button variant="outline" size="sm" onClick={onView}><Eye />{t('common.detail')}</Button>
        <Button variant="outline" size="sm" onClick={onEdit}><Edit />{t('contextManagement.editProfile')}</Button>
      </CardFooter>
    </Card>
  );
}

function ProfileSheet({ mode, profile, onOpenChange, onSave }: { mode: ProfileSheetMode | null; profile: ProfileVm | null; onOpenChange: (open: boolean) => void; onSave: (input: ProfileInput) => Promise<void> }) {
  const { t } = useTranslation();
  const editing = mode === 'create' || mode === 'edit';
  const [saving, setSaving] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<ProfileInput>({
    defaultValues: profileInputDefaults(profile),
  });

  useEffect(() => {
    form.reset(profileInputDefaults(profile));
    setSubmitError(null);
  }, [form, mode, profile]);

  const submit = async (input: ProfileInput) => {
    setSaving(true);
    setSubmitError(null);
    try {
      await onSave({ ...input, name: input.name.trim(), summary: input.summary.trim() });
    } catch (err) {
      setSubmitError(String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Sheet open={mode !== null} onOpenChange={onOpenChange}>
      <SheetContent className="w-[min(720px,calc(100vw-2rem))] max-w-[min(720px,calc(100vw-2rem))] gap-0 overflow-hidden p-0 sm:max-w-[min(720px,calc(100vw-2rem))]">
        <SheetHeader className="border-b px-5 py-4 text-left">
          <SheetTitle>{mode === 'create' ? t('contextManagement.createProfile') : mode === 'edit' ? t('contextManagement.editProfile') : profile?.name}</SheetTitle>
          {editing ? <SheetDescription className="sr-only">{t('contextManagement.editDescription')}</SheetDescription> : <SheetDescription className={cn(!profile?.summary && 'sr-only')}>{profile?.summary || profile?.name}</SheetDescription>}
        </SheetHeader>
        <ScrollArea className="min-h-0 flex-1">
          <div className="space-y-4 p-5">
            {submitError ? <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">{submitError}</div> : null}
            {editing ? (
              <Form {...form}>
                <form id="profile-form" className="space-y-4" onSubmit={form.handleSubmit(submit)}>
                  <FormField
                    control={form.control}
                    name="scope"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('contextManagement.scope')}</FormLabel>
                        <Select value={field.value} onValueChange={field.onChange}>
                          <FormControl>
                            <SelectTrigger><SelectValue /></SelectTrigger>
                          </FormControl>
                          <SelectContent>
                            <SelectItem value="user">{t('contextManagement.userScope')}</SelectItem>
                            <SelectItem value="project">{t('contextManagement.projectScope')}</SelectItem>
                          </SelectContent>
                        </Select>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={form.control}
                    name="name"
                    rules={{ required: t('contextManagement.profileRequired') }}
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('contextManagement.name')}</FormLabel>
                        <FormControl><Input {...field} /></FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={form.control}
                    name="summary"
                    rules={{ required: t('contextManagement.profileRequired') }}
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('contextManagement.summary')}</FormLabel>
                        <FormControl><Textarea {...field} /></FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={form.control}
                    name="content"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('contextManagement.content')}</FormLabel>
                        <FormControl><Textarea className="min-h-72 font-mono text-xs" {...field} /></FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </form>
              </Form>
            ) : profile ? (
              <div className="space-y-4">
                <Card className="bg-muted/15 py-0">
                  <CardContent className="grid gap-3 p-3 text-sm md:grid-cols-2">
                    <ProfileMeta label="ID" value={profile.id} />
                    <ProfileMeta label={t('contextManagement.scope')} value={profile.scope === 'project' ? t('contextManagement.projectScope') : t('contextManagement.userScope')} />
                    <ProfileMeta label={t('contextManagement.createdAt')} value={profile.createdAt} />
                    <ProfileMeta label={t('contextManagement.updatedAt')} value={profile.updatedAt} />
                  </CardContent>
                </Card>
                <Card className="bg-card/40 py-0">
                  <CardContent className="p-4">
                    <Markdown>{profile.content || t('contextManagement.emptyContent')}</Markdown>
                  </CardContent>
                </Card>
              </div>
            ) : null}
          </div>
        </ScrollArea>
        <SheetFooter className="border-t px-5 py-4">
          <Button variant="outline" onClick={() => onOpenChange(false)}>{t('common.close')}</Button>
          {editing ? <Button type="submit" form="profile-form" disabled={saving}>{t('common.save')}</Button> : null}
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

function ProfileMeta({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 space-y-1">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="truncate font-medium">{value}</p>
    </div>
  );
}

function PageSizePicker({ value, onChange }: { value: number; onChange: (value: number) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="outline" className="w-20 justify-between px-3 font-normal">
          {value}
          <ChevronsUpDown className="size-4 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent side="top" align="end" sideOffset={8} className="w-20 p-1">
        <div className="grid gap-1">
          {pageSizes.map((item) => (
            <Button
              key={item}
              type="button"
              variant="ghost"
              className="justify-between px-2 font-normal"
              onClick={() => { onChange(item); setOpen(false); }}
            >
              {item}
              <Check className={cn('size-4', item === value ? 'opacity-100' : 'opacity-0')} />
            </Button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}

function ProfilePagination({ pageIndex, pageCount, onPageChange }: { pageIndex: number; pageCount: number; onPageChange: (value: number) => void }) {
  const { t } = useTranslation();
  const previousDisabled = pageIndex === 0;
  const nextDisabled = pageIndex >= pageCount - 1;
  return (
    <Pagination className="w-auto">
      <PaginationContent>
        <PaginationItem>
          <PaginationLink
            href="#"
            size="default"
            aria-disabled={previousDisabled}
            className={cn('px-3', previousDisabled && 'pointer-events-none opacity-50')}
            onClick={(event) => { event.preventDefault(); if (!previousDisabled) onPageChange(Math.max(0, pageIndex - 1)); }}
          >
            {t('common.previousPage')}
          </PaginationLink>
        </PaginationItem>
        <PaginationItem>
          <PaginationLink
            href="#"
            isActive
            aria-label={`Page ${pageIndex + 1}`}
          >
            {pageIndex + 1}
          </PaginationLink>
        </PaginationItem>
        <PaginationItem>
          <PaginationLink
            href="#"
            size="default"
            aria-disabled={nextDisabled}
            className={cn('px-3', nextDisabled && 'pointer-events-none opacity-50')}
            onClick={(event) => { event.preventDefault(); if (!nextDisabled) onPageChange(Math.min(pageCount - 1, pageIndex + 1)); }}
          >
            {t('common.nextPage')}
          </PaginationLink>
        </PaginationItem>
      </PaginationContent>
    </Pagination>
  );
}

function profileInputDefaults(profile: ProfileVm | null): ProfileInput {
  return {
    scope: profile?.scope ?? 'user',
    name: profile?.name ?? '',
    summary: profile?.summary ?? '',
    content: profile?.content ?? '',
  };
}

function profileSearchText(profile: ProfileVm) {
  return [profile.id, profile.name, profile.summary, profile.content, profile.scope].join('\n').toLowerCase();
}
