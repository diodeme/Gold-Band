import { useEffect, useMemo, useRef, useState } from 'react';
import type { TFunction } from 'i18next';
import { Check, ChevronsUpDown, Edit, Eye, Loader2, Pencil, Plus, RefreshCw, Search, Trash2 } from 'lucide-react';
import { useForm } from 'react-hook-form';
import { useTranslation } from 'react-i18next';
import {
  createProfile, deleteProfile, getProfiles, updateProfile,
  listMcpServers, addMcpServer, updateMcpServer, deleteMcpServer,
  toggleMcpServer, checkMcpServerHealth, listMcpTools,
  listSkills, listProjectSkills, readSkill, writeSkill, deleteSkill,
  getConversationSidebar,
} from '../api';
import { displayAppError } from '../i18n';
import type {
  AppErrorVm, ProfileInput, ProfileListVm, ProfileScope, ProfileVm,
  McpServerVm, SkillListVm, SkillMetaVm, SkillContentVm, ToolInfo,
} from '../types';
import { AppCard } from '@/components/AppCard';
import { EntitySection } from '@/components/EntitySection';
import { McpServerCard } from '@/components/McpServerCard';
import { EmptyState, Page, PageHeader } from '@/components/PageScaffold';
import { Markdown } from '@/components/prompt-kit/markdown';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
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
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { formatLocalDateTime } from '@/lib/datetime';

type ProfileSheetMode = 'view' | 'create' | 'edit';
type ContextTab = 'profiles' | 'mcp' | 'skills';
type ProfileListTab = 'built-in' | 'custom';
const pageSizes = [6, 12, 24];

export function ContextManagementPage() {
  const { t } = useTranslation();
  const [vm, setVm] = useState<ProfileListVm | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<ContextTab>('profiles');
  const [profileListTab, setProfileListTab] = useState<ProfileListTab>('custom');
  const [builtInQuery, setBuiltInQuery] = useState('');
  const [customQuery, setCustomQuery] = useState('');
  const [customScope, setCustomScope] = useState<'all' | Exclude<ProfileScope, 'built-in'>>('all');
  const [pageIndex, setPageIndex] = useState(0);
  const [pageSize, setPageSize] = useState(6);
  const [sheetMode, setSheetMode] = useState<ProfileSheetMode | null>(null);
  const [selectedProfile, setSelectedProfile] = useState<ProfileVm | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ProfileVm | null>(null);
  const [deleteError, setDeleteError] = useState<unknown>(null);
  const [deleteConfirmationError, setDeleteConfirmationError] = useState<AppErrorVm | null>(null);
  const [deleting, setDeleting] = useState(false);

  // ── MCP state ──
  const [mcpServers, setMcpServers] = useState<McpServerVm[]>([]);
  const [mcpLoading, setMcpLoading] = useState(false);
  const [mcpError, setMcpError] = useState<string | null>(null);
  const mcpErrorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 自动清除 mcpError：6 秒后自动消失
  useEffect(() => {
    if (mcpError) {
      if (mcpErrorTimerRef.current) clearTimeout(mcpErrorTimerRef.current);
      mcpErrorTimerRef.current = setTimeout(() => setMcpError(null), 6000);
    }
    return () => {
      if (mcpErrorTimerRef.current) clearTimeout(mcpErrorTimerRef.current);
    };
  }, [mcpError]);
  const [mcpQuery, setMcpQuery] = useState('');
  const [mcpListTab, setMcpListTab] = useState<'custom' | 'built-in'>('custom');
  const [mcpSheetOpen, setMcpSheetOpen] = useState(false);
  const [mcpEditTarget, setMcpEditTarget] = useState<McpServerVm | null>(null);
  const [mcpJsonContent, setMcpJsonContent] = useState('');
  const [mcpTransportTab, setMcpTransportTab] = useState<'stdio' | 'http' | 'sse'>('stdio');
  const [mcpSaving, setMcpSaving] = useState(false);
  const [mcpDeleteTarget, setMcpDeleteTarget] = useState<McpServerVm | null>(null);
  const [mcpCheckTarget, setMcpCheckTarget] = useState<string | null>(null);
  const [mcpHealth, setMcpHealth] = useState<Record<string, { status: string; message?: string | null }>>({});
  const [toolsSheetServer, setToolsSheetServer] = useState<McpServerVm | null>(null);
  const [toolsList, setToolsList] = useState<ToolInfo[] | null>(null);
  const [toolsLoading, setToolsLoading] = useState(false);
  const [toolsError, setToolsError] = useState<string | null>(null);
  const [toolsFetchingId, setToolsFetchingId] = useState<string | null>(null);

  const builtInMcpServers = useMemo(() => mcpServers.filter((s) => s.managed), [mcpServers]);
  const customMcpServers = useMemo(() => mcpServers.filter((s) => !s.managed), [mcpServers]);
  const currentSectionMcpServers = mcpListTab === 'built-in' ? builtInMcpServers : customMcpServers;

  const filteredMcpServers = useMemo(() => {
    const q = mcpQuery.trim().toLowerCase();
    const source = mcpListTab === 'built-in' ? builtInMcpServers : customMcpServers;
    if (!q) return source;
    return source.filter((s) => s.name.toLowerCase().includes(q) || (s.command ?? s.url ?? '').toLowerCase().includes(q));
  }, [builtInMcpServers, customMcpServers, mcpListTab, mcpQuery]);

  // ── SKILL state ──
  const [skillList, setSkillList] = useState<SkillListVm | null>(null);
  const [projectSkills, setProjectSkills] = useState<SkillMetaVm[]>([]);
  const [skillLoading, setSkillLoading] = useState(false);
  const [skillError, setSkillError] = useState<string | null>(null);
  const [skillSheetMode, setSkillSheetMode] = useState<'view' | 'create' | 'edit' | null>(null);
  const [skillEditTarget, setSkillEditTarget] = useState<SkillMetaVm | null>(null);
  const [skillForm, setSkillForm] = useState({ name: '', description: '', body: '', disableModelInvocation: false, source: 'global' as string });
  const [skillSaving, setSkillSaving] = useState(false);
  const [skillDeleteTarget, setSkillDeleteTarget] = useState<SkillMetaVm | null>(null);
  const [skillEditWsPath, setSkillEditWsPath] = useState<string | null>(null);

  const [skillTab, setSkillTab] = useState<'global' | 'project'>('global');
  const [skillQuery, setSkillQuery] = useState('');
  const [selectedWorkspace, setSelectedWorkspace] = useState<string>('');
  const [workspaces, setWorkspaces] = useState<Array<{ projectId: string; workspacePath: string; name: string }>>([]);

  const filteredSkills = useMemo(() => {
    if (skillTab === 'project' && !selectedWorkspace) return [];
    const q = skillQuery.trim().toLowerCase();
    let items = skillTab === 'global' ? (skillList?.global ?? []) : projectSkills;
    if (q) {
      items = items.filter((s) => s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q));
    }
    return items;
  }, [skillList, projectSkills, skillTab, skillQuery, selectedWorkspace]);

  const skillNameConflict = useMemo(() => {
    if (skillSheetMode !== 'create' && skillSheetMode !== 'edit') return false;
    const name = skillForm.name.trim();
    if (!name) return false;
    const isRename = name !== skillEditTarget?.name;
    if (!isRename && skillSheetMode !== 'create') return false;
    return (filteredSkills ?? []).some((s) => s.name === name);
  }, [skillForm.name, skillSheetMode, skillEditTarget, filteredSkills]);

  // 选择 workspace 时加载该项目 SKILL
  const loadProjectSkills = async (wsPath: string) => {
    if (!wsPath) { setProjectSkills([]); return; }
    setSkillLoading(true);
    try { setProjectSkills(await listProjectSkills(wsPath)); } catch { setProjectSkills([]); }
    finally { setSkillLoading(false); }
  };

  useEffect(() => {
    getConversationSidebar().then((s) => setWorkspaces(s?.workspaces ?? [])).catch(() => {});
  }, []);
  // 切换到 SKILL Tab 或 Sheet 打开时刷新工作空间列表
  useEffect(() => {
    if (activeTab === 'skills' || skillSheetMode === 'create') {
      getConversationSidebar().then((s) => setWorkspaces(s?.workspaces ?? [])).catch(() => {});
    }
  }, [activeTab, skillSheetMode]);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      setVm(await getProfiles());
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setLoading(false);
    }
  };

  const refreshMcp = async () => {
    setMcpLoading(true);
    setMcpError(null);
    try {
      const servers = await listMcpServers();
      setMcpServers(servers);
      // 健康状态由后端在启动时后台预探测并写入共享缓存，list 返回时已携带；
      // 这里直接从 VM 回填，无需进入页面后再逐个触发网络检测。
      const seed: Record<string, { status: string; message?: string | null }> = {};
      for (const s of servers) {
        if (s.healthStatus) seed[s.id] = { status: s.healthStatus, message: s.healthMessage };
      }
      setMcpHealth(seed);
    } catch (err) { setMcpError(displayAppError(t, err)); }
    finally { setMcpLoading(false); }
  };
  const refreshSkills = async () => {
    setSkillLoading(true);
    setSkillError(null);
    try {
      setSkillList(await listSkills());
      if (skillTab === 'project' && selectedWorkspace) {
        void loadProjectSkills(selectedWorkspace);
      }
    } catch (err) { setSkillError(displayAppError(t, err)); }
    finally { setSkillLoading(false); }
  };

  useEffect(() => { void refresh(); }, []);
  useEffect(() => { if (activeTab === 'mcp' && mcpServers.length === 0) void refreshMcp(); }, [activeTab]);
  useEffect(() => {
    if (activeTab !== 'skills') return;
    if (!skillList) { void refreshSkills(); return; }
    if (skillTab === 'project' && selectedWorkspace) { void loadProjectSkills(selectedWorkspace); }
  }, [activeTab]);

  const handleMcpSave = async () => {
    setMcpSaving(true); setMcpError(null);
    try {
      // 对标 Zed: 先保存 settings.json
      const result = mcpEditTarget
        ? await updateMcpServer(mcpEditTarget.id, mcpJsonContent)
        : await addMcpServer(mcpJsonContent);
      setMcpServers(result);
      const savedId = mcpEditTarget?.id ?? result.find((s) => !mcpServers.some((e) => e.id === s.id))?.id;
      // 对标 Zed: 保存后立即验证，Modal 保持打开显示 "Connecting Server…"
      if (!savedId) { setMcpSaving(false); setMcpSheetOpen(false); setMcpEditTarget(null); return; }
      setMcpSaving(false);
      setMcpCheckTarget(savedId);
      try {
        const h = await checkMcpServerHealth(savedId);
        setMcpHealth((prev) => ({ ...prev, [savedId]: h }));
        if (h.status === 'healthy') {
          // 对标 Zed: Running → dismiss modal
          setMcpSheetOpen(false); setMcpEditTarget(null);
        } else {
          // 对标 Zed: Error → 显示错误但保留 Sheet（可编辑重试）
          setMcpError(h.message ?? 'Server health check failed');
        }
      } catch (err: unknown) {
        setMcpError(displayAppError(t, err));
        if (savedId) {
          setMcpHealth((prev) => ({ ...prev, [savedId]: { status: 'unhealthy', message: displayAppError(t, err) } }));
        }
      } finally {
        setMcpCheckTarget(null);
      }
    } catch (err: unknown) {
      setMcpSaving(false);
      setMcpError(displayAppError(t, err));
    }
  };

  const dismissMcpSheet = () => {
    setMcpSheetOpen(false);
    setMcpEditTarget(null);
    setMcpError(null);
    setMcpCheckTarget(null);
  };

  const profiles = vm?.profiles ?? [];
  const builtInProfiles = useMemo(() => {
    const normalizedQuery = builtInQuery.trim().toLowerCase();
    return profiles.filter((profile) => {
      if (!profile.isBuiltIn) return false;
      if (!normalizedQuery) return true;
      return profileSearchText(profile).includes(normalizedQuery);
    });
  }, [profiles, builtInQuery]);
  const customProfiles = useMemo(() => {
    const normalizedQuery = customQuery.trim().toLowerCase();
    return profiles.filter((profile) => {
      if (profile.isBuiltIn) return false;
      if (customScope !== 'all' && profile.scope !== customScope) return false;
      if (!normalizedQuery) return true;
      return profileSearchText(profile).includes(normalizedQuery);
    });
  }, [profiles, customQuery, customScope]);
  const pageCount = Math.max(1, Math.ceil(customProfiles.length / pageSize));
  const safePageIndex = Math.min(pageIndex, pageCount - 1);
  const pagedCustomProfiles = customProfiles.slice(safePageIndex * pageSize, safePageIndex * pageSize + pageSize);

  useEffect(() => {
    if (safePageIndex !== pageIndex) setPageIndex(safePageIndex);
  }, [pageIndex, safePageIndex]);

  const openSheet = (mode: ProfileSheetMode, profile?: ProfileVm) => {
    setSheetMode(mode);
    setSelectedProfile(profile ?? null);
  };

  const openDeleteDialog = (profile: ProfileVm) => {
    setDeleteTarget(profile);
    setDeleteError(null);
    setDeleteConfirmationError(null);
  };

  const saveProfile = async (input: ProfileInput) => {
    if (sheetMode === 'edit' && selectedProfile && !selectedProfile.isBuiltIn) {
      await updateProfile(selectedProfile.id, input);
    } else {
      await createProfile(input);
    }
    setSheetMode(null);
    setSelectedProfile(null);
    await refresh();
  };

  const saveProfileAsNew = async (input: ProfileInput) => {
    await createProfile(input);
    setSheetMode(null);
    setSelectedProfile(null);
    await refresh();
  };

  const confirmDeleteProfile = async () => {
    if (!deleteTarget || deleteTarget.isBuiltIn) return;
    setDeleting(true);
    setDeleteError(null);
    try {
      await deleteProfile(deleteTarget.id, Boolean(deleteConfirmationError));
      setDeleteTarget(null);
      setDeleteConfirmationError(null);
      await refresh();
    } catch (err) {
      if (isDeleteConfirmationRequiredError(err)) {
        setDeleteConfirmationError(err);
        return;
      }
      setDeleteError(err);
    } finally {
      setDeleting(false);
    }
  };

  const listQuery = profileListTab === 'built-in' ? builtInQuery : customQuery;
  const onQueryChange = (value: string) => {
    if (profileListTab === 'built-in') {
      setBuiltInQuery(value);
      return;
    }
    setCustomQuery(value);
    setPageIndex(0);
  };

  return (
    <Page flush className="flex flex-col">
      <PageHeader title={<span className="text-title">{t('contextManagement.title')}</span>} />
      <div className="border-b px-5 xl:px-6">
        <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as ContextTab)}>
          <TabsList className="rounded-none border-b-0">
            <TabsTrigger value="profiles">{t('contextManagement.profileManagement')}</TabsTrigger>
            <TabsTrigger value="mcp">{t('contextManagement.tabs.mcp', 'MCP 管理')}</TabsTrigger>
            <TabsTrigger value="skills">{t('contextManagement.tabs.skills', 'SKILL 管理')}</TabsTrigger>
          </TabsList>
        </Tabs>
      </div>
      {/* ── Profiles Tab ── */}
      {activeTab === 'profiles' && (
      <div className="min-h-0 flex-1 p-5 xl:p-6">
        <EntitySection
          tab={profileListTab}
          onTabChange={(value) => setProfileListTab(value)}
          customLabel={t('contextManagement.customSectionTitle')}
          builtInLabel={t('contextManagement.builtInSectionTitle')}
          actions={
            <>
              <Button variant="outline" disabled={loading} onClick={() => void refresh()}>
                <RefreshCw className={cn(loading && 'animate-spin')} />
                {t('common.refresh')}
              </Button>
              <Button onClick={() => openSheet('create')}><Plus />{t('contextManagement.addProfile')}</Button>
            </>
          }
          toolbar={
            <>
              <div className="relative min-w-[240px] flex-1">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  className="pl-9"
                  value={listQuery}
                  onChange={(event) => onQueryChange(event.target.value)}
                  placeholder={t('contextManagement.searchPlaceholder')}
                />
              </div>
              {profileListTab === 'custom' ? (
                <Select value={customScope} onValueChange={(value) => { setCustomScope(value as 'all' | 'user' | 'project'); setPageIndex(0); }}>
                  <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">{t('common.all')}</SelectItem>
                    <SelectItem value="user">{t('contextManagement.userScope')}</SelectItem>
                    <SelectItem value="project">{t('contextManagement.projectScope')}</SelectItem>
                  </SelectContent>
                </Select>
              ) : null}
            </>
          }
          error={error}
          footer={
            profileListTab === 'custom' && customProfiles.length > 0 ? (
              <div className="flex flex-wrap items-center justify-between gap-3 border-t px-4 py-3 text-sm text-muted-foreground">
                <span>{t('contextManagement.customProfilesPageRange', {
                  start: customProfiles.length ? safePageIndex * pageSize + 1 : 0,
                  end: Math.min(customProfiles.length, (safePageIndex + 1) * pageSize),
                  total: customProfiles.length,
                })}</span>
                <div className="flex items-center gap-2">
                  <span>{t('common.pageSize')}</span>
                  <PageSizePicker value={pageSize} onChange={(value) => { setPageSize(value); setPageIndex(0); }} />
                  <ProfilePagination pageIndex={safePageIndex} pageCount={pageCount} onPageChange={setPageIndex} />
                </div>
              </div>
            ) : null
          }
        >
          {loading && !vm ? <EmptyState>{t('common.loading')}</EmptyState> : null}
          {vm && profileListTab === 'built-in' ? (
            <div className="grid gap-3 p-4 md:grid-cols-2 xl:grid-cols-3">
              {builtInProfiles.map((profile) => (
                <BuiltInProfileCard
                  key={`${profile.scope}:${profile.id}`}
                  profile={profile}
                  onView={() => openSheet('view', profile)}
                  onEdit={() => openSheet('edit', profile)}
                />
              ))}
            </div>
          ) : null}
          {vm && profileListTab === 'custom' ? (
            <div className="grid gap-3 p-4 md:grid-cols-2 xl:grid-cols-3">
              {pagedCustomProfiles.map((profile) => (
                <CustomProfileCard
                  key={`${profile.scope}:${profile.id}`}
                  profile={profile}
                  onView={() => openSheet('view', profile)}
                  onEdit={() => openSheet('edit', profile)}
                  onDelete={() => openDeleteDialog(profile)}
                />
              ))}
            </div>
          ) : null}
          {vm && profileListTab === 'built-in' && builtInProfiles.length === 0 ? <div className="p-5"><EmptyState>{t('contextManagement.emptyProfiles')}</EmptyState></div> : null}
          {vm && profileListTab === 'custom' && customProfiles.length === 0 ? <div className="p-5"><EmptyState>{t('contextManagement.emptyProfiles')}</EmptyState></div> : null}
        </EntitySection>
      </div>
      )}
      <ProfileSheet
        mode={sheetMode}
        profile={selectedProfile}
        onOpenChange={(open) => {
          if (!open) {
            setSheetMode(null);
            setSelectedProfile(null);
          }
        }}
        onSave={saveProfile}
        onSaveAsNew={saveProfileAsNew}
      />
      <AlertDialog
        open={Boolean(deleteTarget)}
        onOpenChange={(open) => {
          if (!open) {
            setDeleteTarget(null);
            setDeleteError(null);
            setDeleteConfirmationError(null);
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('contextManagement.deleteProfileTitle')}</AlertDialogTitle>
            {!deleteConfirmationError ? (
              <AlertDialogDescription>
                {t('contextManagement.deleteProfileDescription', { name: deleteTarget?.name ?? '' })}
              </AlertDialogDescription>
            ) : null}
          </AlertDialogHeader>
          {deleteConfirmationError ? (
            <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {deleteConfirmationMessage(t, deleteConfirmationError)}
            </div>
          ) : null}
          {deleteError ? (
            <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {deleteDialogError(t, deleteError)}
            </div>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction disabled={deleting || deleteTarget?.isBuiltIn} onClick={(event) => { event.preventDefault(); void confirmDeleteProfile(); }}>
              {deleteConfirmationError ? t('contextManagement.confirmDeleteProfileAction') : t('contextManagement.deleteProfileAction')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* ── MCP Tab Content ── */}
      {activeTab === 'mcp' && (
        <div className="min-h-0 flex-1 p-5 xl:p-6">
          <EntitySection
            tab={mcpListTab}
            onTabChange={setMcpListTab}
            customLabel={t('contextManagement.mcp.customSectionTitle', '自定义 MCP')}
            builtInLabel={t('contextManagement.mcp.builtInSectionTitle', '内置 MCP')}
            actions={
              <>
                <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  <span className="flex items-center gap-0.5"><span className="size-1.5 rounded-full bg-green-500" />{mcpServers.filter((s) => mcpHealth[s.id]?.status === 'healthy').length}</span>
                  <span className="flex items-center gap-0.5"><span className="size-1.5 rounded-full bg-yellow-500" />{mcpServers.filter((s) => mcpHealth[s.id]?.status === 'auth_required').length}</span>
                  <span className="flex items-center gap-0.5"><span className="size-1.5 rounded-full bg-red-500" />{mcpServers.filter((s) => mcpHealth[s.id]?.status === 'unhealthy').length}</span>
                </span>
                <Button variant="outline" size="sm" disabled={mcpLoading} onClick={() => void refreshMcp()}><RefreshCw className={cn('size-4', mcpLoading && 'animate-spin')} /></Button>
                <Button size="sm" onClick={() => { setMcpEditTarget(null); setMcpJsonContent(MCP_STDIO_TEMPLATE); setMcpTransportTab('stdio'); setMcpSheetOpen(true); }}><Plus className="size-4" />{t('contextManagement.mcp.addServer', '添加')}</Button>
              </>
            }
            toolbar={
              <div className="relative min-w-[240px] flex-1">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input className="pl-9" placeholder={t('contextManagement.searchPlaceholder', '搜索…')} value={mcpQuery} onChange={(e) => setMcpQuery(e.target.value)} />
              </div>
            }
            error={mcpError ? (
              <div className="flex items-start gap-2">
                <span className="flex-1">{mcpError}</span>
                <button type="button" onClick={() => setMcpError(null)} className="shrink-0 rounded-sm opacity-70 transition-opacity hover:opacity-100" aria-label="Dismiss">✕</button>
              </div>
            ) : null}
          >
            {mcpLoading && mcpServers.length === 0 ? <div className="p-5"><EmptyState>{t('common.loading')}</EmptyState></div> : null}
            <div className="grid gap-3 p-4 md:grid-cols-2 xl:grid-cols-3">
              {filteredMcpServers.map((s) => (
                <McpServerCard
                  key={s.id}
                  server={s}
                  health={mcpHealth[s.id]}
                  isChecking={mcpCheckTarget === s.id}
                  isToolsFetching={toolsFetchingId === s.id}
                  onToggle={async (newEnabled) => {
                    try { setMcpServers(await toggleMcpServer(s.id, newEnabled)); } catch (err) { setMcpError(displayAppError(t, err)); return; }
                    if (newEnabled) {
                      setMcpCheckTarget(s.id);
                      try {
                        const hh = await checkMcpServerHealth(s.id);
                        setMcpHealth((prev) => ({ ...prev, [s.id]: hh }));
                      } catch (err: unknown) {
                        setMcpHealth((prev) => ({ ...prev, [s.id]: { status: 'unhealthy', message: displayAppError(t, err) } }));
                      } finally { setMcpCheckTarget(null); }
                    } else {
                      setMcpHealth((prev) => { const n = { ...prev }; delete n[s.id]; return n; });
                    }
                  }}
                  onHealthCheck={async () => {
                    setMcpCheckTarget(s.id);
                    try {
                      const result = await checkMcpServerHealth(s.id);
                      setMcpHealth((prev) => ({ ...prev, [s.id]: result }));
                    } catch (err: unknown) {
                      setMcpHealth((prev) => ({ ...prev, [s.id]: { status: 'unhealthy', message: displayAppError(t, err) } }));
                    } finally { setMcpCheckTarget(null); }
                  }}
                  onShowTools={async () => {
                    if (toolsFetchingId) return;
                    setToolsFetchingId(s.id);
                    setToolsSheetServer(s);
                    setToolsList(null);
                    setToolsError(null);
                    setToolsLoading(true);
                    try {
                      const tools = await listMcpTools(s.id);
                      setToolsList(tools);
                      setToolsError(null);
                    } catch (err: unknown) {
                      setToolsError(displayAppError(t, err));
                      setToolsList(null);
                    } finally {
                      setToolsLoading(false);
                      setToolsFetchingId(null);
                    }
                  }}
                  onEdit={s.managed ? undefined : () => { setMcpEditTarget(s); setMcpJsonContent(mcpServerToJson(s)); setMcpTransportTab(s.transport as 'stdio' | 'http' | 'sse'); setMcpSheetOpen(true); }}
                  onDelete={s.managed ? undefined : () => setMcpDeleteTarget(s)}
                />
              ))}
            </div>
            {!mcpLoading && currentSectionMcpServers.length === 0 ? <div className="p-5"><EmptyState>{t('contextManagement.mcp.emptyServers', '暂无 MCP 服务器')}</EmptyState></div> : null}
            {!mcpLoading && currentSectionMcpServers.length > 0 && filteredMcpServers.length === 0 ? <div className="p-5"><EmptyState>{t('common.noResults', '无匹配结果')}</EmptyState></div> : null}
          </EntitySection>
        </div>
      )}

      {/* ── SKILL Tab Content ── */}
      {activeTab === 'skills' && (
        <div className="min-h-0 flex-1 p-5 xl:p-6">
          <AppCard className="flex h-full min-h-0 flex-col gap-0 py-0">
            <CardContent className="flex min-h-0 flex-1 flex-col gap-3 p-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <Tabs value={skillTab} onValueChange={(v) => { setSkillTab(v as 'global' | 'project'); setSkillQuery(''); setSelectedWorkspace(''); setProjectSkills([]); }}>
                <TabsList variant="line">
                  <TabsTrigger value="global">{t('contextManagement.skills.globalTab', '全局')}</TabsTrigger>
                  <TabsTrigger value="project">{t('contextManagement.skills.projectTab', '项目')}</TabsTrigger>
                </TabsList>
              </Tabs>
              {skillTab === 'project' && workspaces.length > 0 && (
                <Select value={selectedWorkspace} onValueChange={(v) => { setSelectedWorkspace(v); setSkillQuery(''); void loadProjectSkills(v); }}>
                  <SelectTrigger className="h-8 w-44 text-xs">
                    <SelectValue placeholder="选择项目..." />
                  </SelectTrigger>
                  <SelectContent>
                    {workspaces.map((w) => (
                      <SelectItem key={w.projectId} value={w.workspacePath}>{w.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </div>
            <div className="flex items-center gap-2">
              {(skillTab === 'global' || selectedWorkspace) && (
                <div className="relative min-w-[160px]">
                  <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input className="h-8 pl-8 text-xs" placeholder="搜索 SKILL..." value={skillQuery} onChange={(e) => setSkillQuery(e.target.value)} />
                </div>
              )}
              <Button variant="outline" size="sm" disabled={skillLoading} onClick={() => void refreshSkills()}><RefreshCw className={cn('size-4', skillLoading && 'animate-spin')} /></Button>
              <Button size="sm" onClick={() => { setSkillSheetMode('create'); setSkillEditTarget(null); const defSrc = skillTab === 'global' ? 'global' : (selectedWorkspace ? `project:${selectedWorkspace}` : 'project'); setSkillForm({ name: '', description: '', body: '', disableModelInvocation: false, source: defSrc }); }}><Plus className="size-4" />{t('contextManagement.skills.createSkill', '创建')}</Button>
            </div>
          </div>
          {skillError ? <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">{skillError}</div> : null}
          <ScrollArea className="min-h-0 flex-1">
            {skillLoading && !skillList ? <EmptyState>{t('common.loading')}</EmptyState> : null}
            {skillTab === 'project' && !selectedWorkspace ? <EmptyState>选择项目以查看项目级 SKILL</EmptyState> : null}
            {skillTab === 'global' && skillList && skillList.global.length === 0 ? <EmptyState>{t('contextManagement.skills.emptySkills', '暂无 SKILL')}</EmptyState> : null}
            {skillTab === 'project' && selectedWorkspace && !skillLoading && projectSkills.length === 0 ? <EmptyState>{t('contextManagement.skills.emptySkills', '暂无 SKILL')}</EmptyState> : null}
            {skillList && filteredSkills && filteredSkills.length === 0 && skillQuery ? <EmptyState>无匹配结果</EmptyState> : null}
            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
              {filteredSkills && filteredSkills.map((skill) => (
                <Card key={`${skill.source}:${skill.name}`} className="group overflow-hidden border-border/50 transition-shadow hover:shadow-sm">
                  <div className="px-4 py-3">
                    <div className="flex items-start justify-between gap-2">
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="truncate text-sm font-semibold">{skill.name}</span>
                          {skill.disableModelInvocation && (
                            <Badge variant="outline" className="shrink-0 px-1.5 py-0 text-[10px] font-normal text-muted-foreground">manual only</Badge>
                          )}
                        </div>
                        <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-muted-foreground">{skill.description || <span className="italic text-muted-foreground/50">no description</span>}</p>
                      </div>
                      <Badge variant="secondary" className="shrink-0 px-1.5 py-0 text-[10px] font-normal">{skill.source === 'global' ? 'Global' : 'Project'}</Badge>
                    </div>
                  </div>
                  <div className="flex items-center justify-end gap-1 border-t border-border/30 px-2 py-1.5">
                    <TooltipProvider delayDuration={300}>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button size="icon" variant="ghost" className="size-8" onClick={async () => { try { const wsPath = skillTab === 'project' && selectedWorkspace ? selectedWorkspace : null; const c = await readSkill(skill.name, skill.source, wsPath); setSkillEditTarget(skill); setSkillForm({ name: c.meta.name, description: c.meta.description, body: c.body, disableModelInvocation: c.meta.disableModelInvocation, source: skill.source as string }); setSkillEditWsPath(wsPath); setSkillSheetMode('view'); } catch { /* ignore */ } }}>
                            <Eye className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent side="top">View</TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                    <TooltipProvider delayDuration={300}>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button size="icon" variant="ghost" className="size-8" onClick={async () => { try { const wsPath = skillTab === 'project' && selectedWorkspace ? selectedWorkspace : null; const c = await readSkill(skill.name, skill.source, wsPath); setSkillEditTarget(skill); setSkillForm({ name: c.meta.name, description: c.meta.description, body: c.body, disableModelInvocation: c.meta.disableModelInvocation, source: skill.source as string }); setSkillEditWsPath(wsPath); setSkillSheetMode('edit'); } catch { /* ignore */ } }}>
                            <Pencil className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent side="top">Edit</TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                    <TooltipProvider delayDuration={300}>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button size="icon" variant="ghost" className="size-8 text-muted-foreground hover:text-destructive" onClick={() => setSkillDeleteTarget(skill)}>
                            <Trash2 className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent side="top">Delete</TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                  </div>
                </Card>
              ))}
            </div>
            </ScrollArea>
          </CardContent>
        </AppCard>
        </div>
      )}

      {/* ── MCP Sheet (JSON Editor) ── */}
      <Sheet open={mcpSheetOpen} onOpenChange={(open) => { if (!open) dismissMcpSheet(); }}>
        <SheetContent className="gap-0 overflow-hidden" resizeStorageKey="context-management/mcp-sheet" defaultSize={720} minSize={520} maxSize={960}>
          <SheetHeader className="border-b px-5 py-4">
            <SheetTitle>{mcpEditTarget ? t('contextManagement.mcp.editServer', '配置 MCP 服务器') : t('contextManagement.mcp.addServer', '添加 MCP 服务器')}</SheetTitle>
            <SheetDescription>{t('contextManagement.mcp.jsonEditorHint', '查看服务器文档了解所需的参数和环境变量')}</SheetDescription>
          </SheetHeader>
          <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-5 py-4">
            {/* 对标 Zed render_tab_bar: 仅新增时显示 transport 选择，编辑时隐藏 */}
            {!mcpEditTarget ? (
            <div className="flex gap-1 border-b">
              <button type="button" className={cn('px-3 py-2 text-sm font-medium border-b-2 transition-colors', mcpTransportTab === 'stdio' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground')} onClick={() => { setMcpTransportTab('stdio'); setMcpJsonContent(MCP_STDIO_TEMPLATE); }}>{t('contextManagement.mcp.localTab', '本地 (Stdio)')}</button>
              <button type="button" className={cn('px-3 py-2 text-sm font-medium border-b-2 transition-colors', mcpTransportTab === 'http' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground')} onClick={() => { setMcpTransportTab('http'); setMcpJsonContent(MCP_HTTP_TEMPLATE); }}>{t('contextManagement.mcp.remoteTab', '远程 (HTTP)')}</button>
              <button type="button" className={cn('px-3 py-2 text-sm font-medium border-b-2 transition-colors', mcpTransportTab === 'sse' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground')} onClick={() => { setMcpTransportTab('sse'); setMcpJsonContent(MCP_SSE_TEMPLATE); }}>{t('contextManagement.mcp.sseTab', '远程 (SSE)')}</button>
            </div>
            ) : null}
            <textarea
              className="min-h-72 w-full rounded-md border bg-muted/30 p-3 font-mono text-sm leading-relaxed outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={mcpJsonContent}
              onChange={(e) => setMcpJsonContent(e.target.value)}
              spellCheck={false}
              disabled={!!mcpCheckTarget}
            />
            {/* 对标 Zed: 状态区域 — Connecting / Error */}
            {mcpCheckTarget ? (
              <div className="flex items-center gap-2 rounded-md bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                {t('contextManagement.mcp.connecting', 'Connecting Server…')}
              </div>
            ) : null}
            {mcpError ? <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive"><span className="flex-1">{mcpError}</span><button type="button" onClick={() => setMcpError(null)} className="shrink-0 rounded-sm opacity-70 transition-opacity hover:opacity-100" aria-label="Dismiss">✕</button></div> : null}
          </div>
          <SheetFooter className="border-t px-5 py-4">
            <Button variant="outline" onClick={dismissMcpSheet}>{mcpError ? t('common.close') : t('common.close')}</Button>
            <Button disabled={mcpSaving || !!mcpCheckTarget || !mcpJsonContent.trim()} onClick={() => void handleMcpSave()}>{mcpSaving ? t('common.loading') : mcpCheckTarget ? t('contextManagement.mcp.connecting', 'Connecting…') : mcpEditTarget ? t('contextManagement.mcp.saveConfigure', '配置服务器') : t('contextManagement.mcp.saveServer', '添加服务器')}</Button>
          </SheetFooter>
        </SheetContent>
      </Sheet>

      {/* ── MCP Delete Dialog ── */}
      <AlertDialog open={Boolean(mcpDeleteTarget)} onOpenChange={(open) => !open && setMcpDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('contextManagement.mcp.deleteServer', '删除 MCP 服务器')}</AlertDialogTitle>
            <AlertDialogDescription>{t('contextManagement.mcp.deleteDescription', '确定要删除 MCP 服务器吗？').replace('{name}', mcpDeleteTarget?.name ?? '')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction onClick={async () => { if (!mcpDeleteTarget) return; try { setMcpServers(await deleteMcpServer(mcpDeleteTarget.id)); setMcpDeleteTarget(null); } catch (err) { setMcpError(displayAppError(t, err)); setMcpDeleteTarget(null); } }}>{t('contextManagement.mcp.deleteServer', '删除')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* ── MCP Tools Sheet ── */}
      <Sheet open={Boolean(toolsSheetServer)} onOpenChange={(open) => { if (!open) { setToolsSheetServer(null); setToolsList(null); setToolsError(null); setToolsLoading(false); } }}>
        <SheetContent className="gap-0 overflow-hidden" resizeStorageKey="context-management/tools-sheet" defaultSize={560} minSize={420} maxSize={800}>
          <SheetHeader className="border-b px-5 py-4">
            <SheetTitle className="flex items-center gap-2">
              <span className="truncate">{toolsSheetServer?.name ?? ''}</span>
              <Badge variant="secondary" className="shrink-0 px-1.5 py-0 text-[10px] font-normal">{toolsSheetServer?.transport === 'stdio' ? 'Stdio' : toolsSheetServer?.transport === 'sse' ? 'SSE' : 'HTTP'}</Badge>
            </SheetTitle>
          </SheetHeader>
          <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-5 py-4">
            {toolsLoading ? (
              <div className="flex items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                正在获取工具列表…
              </div>
            ) : toolsError ? (
              <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">{toolsError}</div>
            ) : toolsList && toolsList.length === 0 ? (
              <div className="py-12 text-center text-sm text-muted-foreground">该服务器未提供任何工具</div>
            ) : toolsList ? (
              <>
                <p className="text-xs text-muted-foreground">共 {toolsList.length} 个工具</p>
                <div className="space-y-2">
                  {toolsList.map((tool) => (
                    <div key={tool.name} className="rounded-lg border border-border/50 bg-card/40 px-4 py-3">
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-sm font-medium">{tool.name}</p>
                          {tool.description && (
                            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{tool.description}</p>
                          )}
                        </div>
                      </div>
                      {tool.inputSchema && typeof tool.inputSchema === 'object' && Object.keys(tool.inputSchema as Record<string, unknown>).length > 0 && (
                        <details className="mt-2">
                          <summary className="cursor-pointer text-[11px] text-muted-foreground hover:text-foreground">参数 Schema</summary>
                          <pre className="mt-1.5 overflow-x-auto rounded-md bg-muted/50 px-3 py-2 font-mono text-[11px] leading-relaxed">{JSON.stringify(tool.inputSchema, null, 2)}</pre>
                        </details>
                      )}
                    </div>
                  ))}
                </div>
              </>
            ) : null}
          </div>
          <SheetFooter className="border-t px-5 py-4">
            <Button variant="outline" onClick={() => { setToolsSheetServer(null); setToolsList(null); setToolsError(null); }}>关闭</Button>
          </SheetFooter>
        </SheetContent>
      </Sheet>

      {/* ── SKILL Sheet ── */}
      <Sheet open={skillSheetMode !== null} onOpenChange={(open) => { if (!open) setSkillSheetMode(null); }}>
        <SheetContent className="gap-0 overflow-hidden" resizeStorageKey="context-management/skill-sheet" defaultSize={720} minSize={520} maxSize={960}>
          <SheetHeader className="border-b px-5 py-4">
            <SheetTitle>{skillSheetMode === 'create' ? t('contextManagement.skills.createSkill', '创建 SKILL') : skillSheetMode === 'edit' ? `编辑 ${skillEditTarget?.name ?? ''}` : skillEditTarget?.name ?? t('common.detail')}</SheetTitle>
          </SheetHeader>
          <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-5 py-4">
            {skillSheetMode === 'view' ? (
              <div className="space-y-4">
                <div className="grid gap-2 text-sm">
                  <div><span className="text-muted-foreground">{t('contextManagement.skills.name', '名称')}:</span> {skillForm.name}</div>
                  <div><span className="text-muted-foreground">{t('contextManagement.skills.description', '描述')}:</span> {skillForm.description}</div>
                  <div><span className="text-muted-foreground">Scope:</span> {skillForm.source === 'global' ? 'Global' : 'Project'}</div>
                </div>
                <div className="rounded-lg border bg-card/50 p-4">
                  <Markdown>{skillForm.body || '_no content_'}</Markdown>
                </div>
              </div>
            ) : (
              <>
                {/* 对标 Zed Scope dropdown — 默认 Project，多工作空间可选 */}
                <label className="block space-y-1">
                  <span className="text-sm font-medium">{t('contextManagement.scope', 'Scope')}</span>
                  <select
                    className="h-10 w-full rounded-md border bg-background px-3 text-sm"
                    value={skillSheetMode === 'edit' && skillEditWsPath ? `project:${skillEditWsPath}` : skillForm.source}
                    onChange={(e) => setSkillForm({ ...skillForm, source: e.target.value })}
                    disabled={skillSheetMode === 'edit'}
                  >
                    {skillSheetMode === 'edit' && skillEditWsPath ? (
                      <option value={`project:${skillEditWsPath}`}>
                        {workspaces.find((w) => w.workspacePath === skillEditWsPath)?.name ?? skillEditWsPath} (project)
                      </option>
                    ) : (
                      <>
                        {workspaces.map((w) => (
                          <option key={w.projectId} value={`project:${w.workspacePath}`}>{w.name} (project)</option>
                        ))}
                        <option value="global">Global</option>
                        {workspaces.length === 0 && <option value="project">Project</option>}
                      </>
                    )}
                  </select>
                  <p className="text-xs text-muted-foreground">
                    {skillForm.source === 'global'
                      ? 'Available across every project. Saved to ~/.agents/skills/<name>/SKILL.md'
                      : `Project-level. Saved to ${skillForm.source.startsWith('project:') ? skillForm.source.slice(8) : '<project>'}/.agents/skills/<name>/SKILL.md`}
                  </p>
                </label>
                <label className="block space-y-1">
                  <span className="text-sm font-medium">{t('contextManagement.skills.name', '名称')}</span>
                  <input className="h-10 w-full rounded-md border bg-background px-3 text-sm" value={skillForm.name} onChange={(e) => setSkillForm({ ...skillForm, name: e.target.value })} />
                </label>
                <label className="block space-y-1">
                  <span className="text-sm font-medium">{t('contextManagement.skills.description', '描述')}</span>
                  <input className="h-10 w-full rounded-md border bg-background px-3 text-sm" value={skillForm.description} onChange={(e) => setSkillForm({ ...skillForm, description: e.target.value })} />
                </label>
                {skillNameConflict && (
                  <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                    名称 "{skillForm.name.trim()}" 已存在，请使用其他名称。
                  </div>
                )}
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={skillForm.disableModelInvocation} onChange={(e) => setSkillForm({ ...skillForm, disableModelInvocation: e.target.checked })} />
                  <span className="text-sm">{t('contextManagement.skills.disableModelInvocation', '禁用自动调用')}</span>
                </label>
                <label className="block space-y-1">
                  <span className="text-sm font-medium">{t('contextManagement.skills.body', '正文 (Markdown)')}</span>
                  <textarea className="min-h-72 w-full rounded-md border bg-muted/30 p-3 font-mono text-xs leading-relaxed" value={skillForm.body} onChange={(e) => setSkillForm({ ...skillForm, body: e.target.value })} />
                </label>
              </>
            )}
          </div>
          <SheetFooter className="border-t px-5 py-4">
            <Button variant="outline" onClick={() => setSkillSheetMode(null)}>{t('common.close')}</Button>
            {(skillSheetMode === 'create' || skillSheetMode === 'edit') && (
              <Button disabled={skillSaving || !skillForm.name.trim() || skillNameConflict} onClick={async () => {
                setSkillSaving(true);
                try {
                  const scope = skillForm.source.startsWith('project:') ? 'project' : skillForm.source;
                  // 编辑时使用原始 workspace 路径，创建时从 source 提取
                  const wsPath = skillSheetMode === 'edit'
                    ? skillEditWsPath
                    : (skillForm.source.startsWith('project:') ? skillForm.source.slice(8) : null);
                  const content = `---\nname: ${skillForm.name.trim()}\ndescription: ${skillForm.description.trim()}\n${skillForm.disableModelInvocation ? 'disable-model-invocation: true\n' : ''}---\n\n${skillForm.body}`;
                  const oldName = skillSheetMode === 'edit' ? skillEditTarget?.name : null;
                  await writeSkill(skillForm.name.trim(), scope, content, wsPath, oldName);
                  setSkillList(await listSkills());
                  if (skillTab === 'project' && selectedWorkspace) { void loadProjectSkills(selectedWorkspace); }
                  setSkillSheetMode(null);
                } catch (err) { setSkillError(displayAppError(t, err)); }
                finally { setSkillSaving(false); }
              }}>
                {t('common.save')}
              </Button>
            )}
          </SheetFooter>
        </SheetContent>
      </Sheet>

      {/* ── SKILL Delete Dialog ── */}
      <AlertDialog open={Boolean(skillDeleteTarget)} onOpenChange={(open) => !open && setSkillDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('contextManagement.skills.deleteSkill', '删除 SKILL')}</AlertDialogTitle>
            <AlertDialogDescription>确定要删除 SKILL "{skillDeleteTarget?.name}" 吗？此操作不可恢复。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction onClick={async () => { if (!skillDeleteTarget) return; try { const wsPath = skillTab === 'project' && selectedWorkspace ? selectedWorkspace : null; await deleteSkill(skillDeleteTarget.name, skillDeleteTarget.source, wsPath); setSkillList(await listSkills()); if (skillTab === 'project' && selectedWorkspace) { void loadProjectSkills(selectedWorkspace); } } catch { /* ignore */ } finally { setSkillDeleteTarget(null); } }}>{t('contextManagement.skills.deleteSkill', '删除')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}

function BuiltInProfileCard({ profile, onView, onEdit }: { profile: ProfileVm; onView: () => void; onEdit: () => void }) {
  const { t } = useTranslation();
  return (
    <Card className="h-full min-h-52 gap-0 bg-card/45 py-0">
      <CardHeader className="px-4 py-4 pb-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <CardTitle className="truncate text-base">{profile.name}</CardTitle>
            <CardDescription className="mt-1 truncate font-mono text-xs">{profile.id}</CardDescription>
          </div>
          <Badge variant="secondary" className="shrink-0">{profileScopeLabel(t, profile.scope)}</Badge>
        </div>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col px-4 pb-0">
        <CardDescription className="line-clamp-3 leading-6">{profile.summary}</CardDescription>
      </CardContent>
      <CardFooter className="mt-auto justify-end gap-2 px-4 py-4 pt-3">
        <Button variant="outline" size="sm" onClick={onView}><Eye />{t('common.detail')}</Button>
        <Button variant="outline" size="sm" onClick={onEdit}><Edit />{t('contextManagement.editProfile')}</Button>
      </CardFooter>
    </Card>
  );
}

function CustomProfileCard({ profile, onView, onEdit, onDelete }: { profile: ProfileVm; onView: () => void; onEdit: () => void; onDelete: () => void }) {
  const { t } = useTranslation();
  return (
    <Card className="h-full min-h-52 gap-0 bg-card/50 py-0">
      <CardHeader className="px-4 py-4 pb-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <CardTitle className="truncate text-base">{profile.name}</CardTitle>
            <CardDescription className="mt-1 truncate font-mono text-xs">{profile.id}</CardDescription>
          </div>
          <Badge variant="outline" className="shrink-0">{profileScopeLabel(t, profile.scope)}</Badge>
        </div>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col px-4 pb-0">
        <CardDescription className="line-clamp-3 leading-6">{profile.summary}</CardDescription>
        <dl className="mt-auto grid gap-1 pt-3 text-xs text-muted-foreground">
          <div className="flex gap-1"><dt>{t('contextManagement.createdAt')}:</dt><dd>{formatLocalDateTime(profile.createdAt)}</dd></div>
          <div className="flex gap-1"><dt>{t('contextManagement.updatedAt')}:</dt><dd>{formatLocalDateTime(profile.updatedAt)}</dd></div>
        </dl>
      </CardContent>
      <CardFooter className="justify-end gap-2 px-4 py-4 pt-3">
        <Button variant="outline" size="sm" onClick={onView}><Eye />{t('common.detail')}</Button>
        <Button variant="outline" size="sm" onClick={onEdit}><Edit />{t('contextManagement.editProfile')}</Button>
        <Button
          variant="outline"
          size="sm"
          aria-label={t('contextManagement.deleteProfile', { name: profile.name })}
          onClick={onDelete}
        >
          <Trash2 />
          {t('contextManagement.deleteProfileShort')}
        </Button>
      </CardFooter>
    </Card>
  );
}

function ProfileSheet({ mode, profile, onOpenChange, onSave, onSaveAsNew }: { mode: ProfileSheetMode | null; profile: ProfileVm | null; onOpenChange: (open: boolean) => void; onSave: (input: ProfileInput) => Promise<void>; onSaveAsNew: (input: ProfileInput) => Promise<void> }) {
  const { t } = useTranslation();
  const editing = mode === 'create' || mode === 'edit';
  const isBuiltIn = Boolean(profile?.isBuiltIn);
  const [saving, setSaving] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [saveAsOpen, setSaveAsOpen] = useState(false);
  const [saveAsName, setSaveAsName] = useState('');
  const [saveAsError, setSaveAsError] = useState<string | null>(null);
  const form = useForm<ProfileInput>({
    defaultValues: profileInputDefaults(profile),
  });

  useEffect(() => {
    form.reset(profileInputDefaults(profile));
    setSubmitError(null);
    setSaveAsOpen(false);
    setSaveAsName(profile?.name ?? '');
    setSaveAsError(null);
  }, [form, mode, profile]);

  const submit = async (input: ProfileInput) => {
    setSaving(true);
    setSubmitError(null);
    try {
      await onSave({ ...input, name: input.name.trim(), summary: input.summary.trim() });
    } catch (err) {
      setSubmitError(displayAppError(t, err));
    } finally {
      setSaving(false);
    }
  };

  const openSaveAsDialog = () => {
    setSaveAsError(null);
    setSaveAsName((form.getValues('name') || profile?.name || '').trim());
    setSaveAsOpen(true);
  };

  const confirmSaveAsNew = async () => {
    const trimmedName = saveAsName.trim();
    if (!trimmedName) {
      setSaveAsError(t('contextManagement.profileRequired'));
      return;
    }
    const values = form.getValues();
    setSaving(true);
    setSaveAsError(null);
    setSubmitError(null);
    try {
      await onSaveAsNew({ ...values, name: trimmedName, summary: values.summary.trim() });
      setSaveAsOpen(false);
    } catch (err) {
      setSaveAsError(displayAppError(t, err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <Sheet open={mode !== null} onOpenChange={onOpenChange}>
        <SheetContent className="gap-0 overflow-hidden p-0" resizeStorageKey="context-management/profile-sheet" defaultSize={720} minSize={520} maxSize={960}>
          <SheetHeader className="border-b px-5 py-4 text-left">
            <SheetTitle>{mode === 'create' ? t('contextManagement.createProfile') : mode === 'edit' ? t('contextManagement.editProfile') : profile?.name}</SheetTitle>
            {editing ? (
              <SheetDescription className={cn(!isBuiltIn && 'sr-only')}>
                {isBuiltIn ? t('contextManagement.builtInReadonlyHint') : t('contextManagement.editDescription')}
              </SheetDescription>
            ) : (
              <SheetDescription className={cn(!profile?.summary && 'sr-only')}>{profile?.summary || profile?.name}</SheetDescription>
            )}
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
                          <Select value={field.value} onValueChange={(value) => field.onChange(value as ProfileScope)}>
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
                      <ProfileMeta label={t('contextManagement.scope')} value={profileScopeLabel(t, profile.scope)} />
                      <ProfileMeta label={t('contextManagement.createdAt')} value={formatLocalDateTime(profile.createdAt)} />
                      <ProfileMeta label={t('contextManagement.updatedAt')} value={formatLocalDateTime(profile.updatedAt)} />
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
            {editing ? (
              isBuiltIn && mode === 'edit'
                ? <Button type="button" disabled={saving} onClick={openSaveAsDialog}>{t('contextManagement.saveAsNewProfile')}</Button>
                : <Button type="submit" form="profile-form" disabled={saving}>{t('common.save')}</Button>
            ) : null}
          </SheetFooter>
        </SheetContent>
      </Sheet>
      <Dialog open={saveAsOpen} onOpenChange={setSaveAsOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('contextManagement.saveAsNewProfile')}</DialogTitle>
            <DialogDescription>{t('contextManagement.saveAsNewProfileDescription')}</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Input value={saveAsName} onChange={(event) => setSaveAsName(event.target.value)} placeholder={t('contextManagement.name')} />
            {saveAsError ? <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">{saveAsError}</div> : null}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setSaveAsOpen(false)}>{t('common.close')}</Button>
            <Button disabled={saving} onClick={() => void confirmSaveAsNew()}>{t('contextManagement.saveAsNewProfile')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
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
    scope: profile?.scope === 'project' ? 'project' : 'user',
    name: profile?.name ?? '',
    summary: profile?.summary ?? '',
    content: profile?.content ?? '',
  };
}

function profileScopeLabel(t: (key: string) => string, scope: ProfileScope) {
  switch (scope) {
    case 'built-in':
      return t('contextManagement.builtInScope');
    case 'project':
      return t('contextManagement.projectScope');
    case 'user':
    default:
      return t('contextManagement.userScope');
  }
}

function deleteDialogError(t: TFunction, error: unknown) {
  if (isAppErrorVm(error) && error.code === 'app.unexpected' && typeof error.params.message === 'string' && error.params.message.trim()) {
    return error.params.message;
  }
  const message = displayAppError(t, error);
  if (message !== t('errors.app.unexpected')) {
    return message;
  }
  return rawErrorText(error) || message;
}

function deleteConfirmationMessage(t: TFunction, error: AppErrorVm) {
  const targets = profileUsageTargets(t, error.params ?? {});
  if (targets) {
    return t('contextManagement.deleteProfileBlockedByReferences', { targets });
  }
  return t('contextManagement.deleteProfileConfirmationDescription');
}

function profileUsageTargets(t: TFunction, params: Record<string, unknown>) {
  return [
    numericParam(params.templateCount) > 0 ? t('contextManagement.profileUsageTemplateCount', { count: numericParam(params.templateCount) }) : null,
    numericParam(params.taskCount) > 0 ? t('contextManagement.profileUsageTaskCount', { count: numericParam(params.taskCount) }) : null,
    numericParam(params.runCount) > 0 ? t('contextManagement.profileUsageRunCount', { count: numericParam(params.runCount) }) : null,
  ].filter(Boolean).join('、');
}

function rawErrorText(error: unknown) {
  if (typeof error === 'string') {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  if (!error || typeof error !== 'object') {
    return '';
  }
  try {
    return JSON.stringify(error);
  } catch {
    return '';
  }
}

function numericParam(value: unknown) {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

function isAppErrorVm(value: unknown): value is AppErrorVm {
  return Boolean(value)
    && typeof value === 'object'
    && typeof (value as Partial<AppErrorVm>).code === 'string'
    && typeof (value as Partial<AppErrorVm>).params === 'object'
    && (value as Partial<AppErrorVm>).params !== null;
}

function isDeleteConfirmationRequiredError(value: unknown): value is AppErrorVm {
  return isAppErrorVm(value) && value.code === 'profile.delete-confirmation-required';
}

function profileSearchText(profile: ProfileVm) {
  return [profile.id, profile.name, profile.summary, profile.content, profile.scope].join('\n').toLowerCase();
}

// ── MCP JSON Templates & Helpers ──

const MCP_STDIO_TEMPLATE = `{
  /// Configure an MCP server that runs locally via stdin/stdout
  ///
  /// The name of your MCP server
  "some-mcp-server": {
    /// The command which runs the MCP server
    "command": "",
    /// The arguments to pass to the MCP server
    "args": [],
    /// The environment variables to set
    "env": {}
  }
}`;

const MCP_HTTP_TEMPLATE = `{
  /// Configure an MCP server that you connect to over HTTP
  ///
  /// The name of your remote MCP server
  "some-remote-server": {
    /// The URL of the remote MCP server
    "url": "https://example.com/mcp",
    /// Any headers to send along
    "headers": {
      // "Authorization": "Bearer <token>"
    },
    /// Optional OAuth configuration for pre-registered clients
    // "oauth": {
    //   "clientId": "your-client-id"
    // }
  }
}`;

const MCP_SSE_TEMPLATE = `{
  /// Configure an MCP server using Server-Sent Events (SSE) transport
  ///
  /// The name of your SSE MCP server
  "some-sse-server": {
    /// The transport type — must be "sse"
    "type": "sse",
    /// The URL of the SSE MCP server endpoint
    "url": "https://example.com/mcp/sse",
    /// Any headers to send along
    "headers": {
      // "Authorization": "Bearer <token>"
    }
  }
}`;

function mcpServerToJson(s: McpServerVm): string {
  if (s.transport === 'sse') {
    const headers = s.headers?.length
      ? s.headers.map((h) => `"${h.key}": "${h.value}"`).join(',\n      ')
      : '// "Authorization": "Bearer <token>"';
    return `{
  /// Configure an MCP server using Server-Sent Events (SSE) transport
  "${s.name}": {
    "type": "sse",
    "url": "${s.url ?? ''}",
    "headers": {
      ${headers}
    }
  }
}`;
  }
  if (s.transport === 'http') {
    const headers = s.headers?.length
      ? s.headers.map((h) => `"${h.key}": "${h.value}"`).join(',\n      ')
      : '// "Authorization": "Bearer <token>"';
    return `{
  /// Configure an MCP server that you connect to over HTTP
  "${s.name}": {
    "url": "${s.url ?? ''}",
    "headers": {
      ${headers}
    }
  }
}`;
  }
  const env = s.env?.length
    ? s.env.map((e) => `"${e.key}": "${e.value}"`).join(',\n      ')
    : '';
  const args = s.args?.length
    ? s.args.map((a) => `"${a}"`).join(', ')
    : '';
  return `{
  /// Configure an MCP server that runs locally via stdin/stdout
  "${s.name}": {
    "command": "${s.command ?? ''}",
    "args": [${args}],
    "env": {
      ${env}
    }
  }
}`;
}
