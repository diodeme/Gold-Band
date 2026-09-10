import type { RuntimeApi, WorkspaceFileLinkOrigin } from '@/api/client';
import { browserApi as previewApi } from '@/api/browser';
import { BrowserPreviewState } from '@/api/browserState';
import type { SkillContentVm } from '@/types';
import { demoRun, demoSessionForNode, DEMO_TASKS, DEMO_RUN_ID, DEMO_PROJECT_ID } from './fixtures';
import { readDemoLayout, readDemoPreferences, writeDemoLayout, writeDemoPreferences } from './preferences';
import { demoAgentRegistry, demoAutoTemplates, demoProfiles, demoProfileContent, demoWorkflowTemplates } from './catalog';
import { DEMO_REPORT_PATH, demoDevelopmentFiles, demoReportContent, validateDemoFileLocator } from './turn-files';
import { demoRunIds } from './scenarios';
import { demoManagementApi } from './management';
import { archiveSession, type ArchiveCatalog, type ArchiveReader } from './archive';
import { archiveEvent, archiveSessionVm, archiveTimestamp } from './archive-session';
import { ARCHIVE_ATTACHMENT_PREFIX, ARCHIVE_FILE_LIMITS, archiveAttachmentPath, parseArchiveAttachmentPath } from './archive-files';
import { ARCHIVE_DIRECTORY_PREFIX, archiveDirectoryPath, normalizeArchiveRelativePath, parseArchiveDirectoryPath, resolveArchiveRelativeReference } from './archive-directories';
import { parseLocalFileLinkTarget } from '@/lib/file-link';
import { isArchiveRunSourceReference, resolveArchiveSourceReference } from './archive-source-reference';
import { ARCHIVE_SOURCE_PREFIX, archiveHistoricalSourcePath, archiveSourceName, parseArchiveHistoricalSourcePath } from './archive-historical-source';

export interface DemoArchive { reader: ArchiveReader; catalog(): Promise<ArchiveCatalog> }

export const DEMO_READ_METHODS = [
  'getAgentRegistry', 'getProfiles', 'getProfile', 'getWorkflowTemplates', 'getWorkflow',
  'getConversationRun', 'getAcpSession', 'getAcpActivityDetail', 'getAcpToolDetail',
  'getAcpRawFrames', 'getSupportedAttachmentExtensions', 'getSystemFonts',
  'listWorkspaceDirectory', 'searchWorkspaceFiles', 'resolveWorkspaceFileLink',
  'readFileResource', 'resolveMarkdownImage', 'getConversationWorkspaces',
  'getSkillSyncStatus',
  'getSourceControlSnapshot', 'getGitHistory', 'getGitCommitDetail', 'getGitCommitReview',
  'getGitCommitReachability', 'getGitComparison', 'getGitBranchPickerSnapshot',
] as const satisfies readonly (keyof RuntimeApi)[];

const presetProjectMethods = new Set<keyof RuntimeApi>([
  'listWorkspaceDirectory', 'searchWorkspaceFiles', 'getGitCapability', 'getGitHubCapability',
  'getSourceControlSnapshot', 'getGitHistory', 'getGitCommitDetail', 'getGitCommitReview',
  'getGitCommitReachability', 'getGitComparison', 'getGitBranchPickerSnapshot',
]);

const quietMethods = new Set<keyof RuntimeApi>([
  'recordActivity', 'reportFrontendError', 'updateNotificationAttention',
  'startWorkspaceFileWatch', 'stopWorkspaceFileWatch', 'releaseWorkspaceFilePreview',
  'releaseExternalFileAccess',
]);
const subscriptions = new Set<keyof RuntimeApi>([
  'subscribeAcpSessionUpdates', 'subscribeConversationRunStateUpdates',
  'subscribeConversationTerminalResultUpdates', 'subscribeWorkspaceFileChanges',
  'subscribeInterventionNavigate', 'subscribeMulticaTaskUpdates',
  'subscribeMulticaSettingsUpdates', 'subscribeScheduledTaskUpdates', 'subscribeScheduledOccurrenceUpdates',
  'subscribeGitStateChanges', 'subscribeGitOperationUpdates', 'subscribeGitHubOperationUpdates',
]);

export function createDemoApi(storage?: Pick<Storage, 'getItem' | 'setItem'>, archive?: DemoArchive): RuntimeApi {
  const state = new BrowserPreviewState();
  const initial = state.getPreferences();
  initial.appearance.colorScheme = 'dark';
  state.setPreferences(readDemoPreferences(storage, initial));
  let layout = readDemoLayout(storage);
  const readMethods = new Set<string>(DEMO_READ_METHODS);
  const methods = new Map<PropertyKey, unknown>();
  async function source(projectId: string | null | undefined, taskId: string, runId: string) {
    if (!archive || projectId === 'default' || !projectId) return null;
    const catalog = await archive.catalog();
    if (catalog.projectId !== projectId || catalog.task.id !== taskId || catalog.run.id !== runId) throw { code: 'demo.resource-not-found', params: {} };
    return { catalog, reader: archive.reader };
  }
  async function directoryReference(projectId: string, rawHref: string, baseCanonicalPath?: string | null) {
    if (!archive) throw { code: 'demo.resource-not-found', params: {} };
    const direct = rawHref.startsWith(ARCHIVE_DIRECTORY_PREFIX) || rawHref.startsWith(ARCHIVE_ATTACHMENT_PREFIX);
    const path = direct ? rawHref : baseCanonicalPath;
    if (!path) return resolveArchiveSourceReference(archive.reader, await archive.catalog(), projectId, rawHref);
    let locator;
    let documentPath;
    if (path.startsWith(ARCHIVE_DIRECTORY_PREFIX)) {
      ({ locator, relativePath: documentPath } = parseArchiveDirectoryPath(path));
    } else {
      const attachment = parseArchiveAttachmentPath(path);
      locator = attachment.locator;
      if (locator.projectId !== projectId) throw { code: 'demo.resource-not-found', params: {} };
      const data = await source(projectId, locator.taskId, locator.runId);
      if (!data) throw { code: 'demo.resource-not-found', params: {} };
      const files = await data.reader.files(archiveSession(data.catalog, locator), locator.branchId, attachment.changeSetId);
      const entry = files.changeSet.attachments.find(value => value.id === attachment.attachmentId);
      if (!entry || !files.attachments[entry.id]) throw { code: 'demo.resource-not-found', params: {} };
      documentPath = `attachments/${normalizeArchiveRelativePath(entry.relativePath)}`;
    }
    if (locator.projectId !== projectId) throw { code: 'demo.resource-not-found', params: {} };
    const data = await source(projectId, locator.taskId, locator.runId);
    if (!data) throw { code: 'demo.resource-not-found', params: {} };
    const relativePath = direct ? documentPath : resolveArchiveRelativeReference(documentPath, rawHref);
    const file = await data.reader.directoryFile(archiveSession(data.catalog, locator), relativePath);
    return { file, canonicalPath: archiveDirectoryPath(locator, file.relativePath) };
  }
  async function historicalReference(projectId: string, origin: WorkspaceFileLinkOrigin, href: string) {
    if (origin.locator.projectId !== projectId) throw { code: 'demo.resource-not-found', params: {} };
    const data = await source(projectId, origin.locator.taskId, origin.locator.runId);
    if (!data) throw { code: 'demo.resource-not-found', params: {} };
    const reference = await data.reader.sourceReference(archiveSession(data.catalog, origin.locator), origin.locator.branchId, origin.eventId, href);
    return { file: { name: archiveSourceName(reference.sourcePath), relativePath: reference.sourcePath, resource: reference.resource, image: null },
      canonicalPath: archiveHistoricalSourcePath(origin, href, reference.sourcePath) };
  }
  async function historicalPathReference(projectId: string, path: string) {
    const { origin, href, name } = parseArchiveHistoricalSourcePath(path);
    const result = await historicalReference(projectId, origin, href);
    if (result.file.name !== name) throw { code: 'demo.resource-not-found', params: {} };
    return result;
  }
  const overrides: Partial<RuntimeApi> = {
    ...demoManagementApi(() => state.getPreferences().language),
    async getGitCapability() {
      return { status: 'ready', installedVersion: '2.53.0', minimumVersion: '2.36.0', repoRoot: '/default', commonDir: '/default/.git', head: '9e1d4f31c17c9bb7f382e130e8db2ab98cf58241' };
    },
    async getGitHubCapability() {
      return { status: 'repository-unresolved', version: null, host: null, account: null, repository: null, remote: null, defaultBranch: null };
    },
    async listConversationDirectory(input) {
      const data = await source(input.projectId, input.taskId, input.runId);
      if (data) {
        const session = archiveSession(data.catalog, { ...input, projectId: input.projectId! });
        const path = normalizeArchiveRelativePath(input.relativePath ?? '');
        const listing = await data.reader.directory(session, path);
        return listing.entries.map(entry => {
          const relativePath = listing.relativePath ? `${listing.relativePath}/${entry.name}` : entry.name;
          return { name: entry.name, kind: entry.kind, relativePath, canonicalPath: archiveDirectoryPath(session, relativePath),
            hasChildren: entry.kind === 'directory' && entry.hasChildren, byteLength: entry.kind === 'file' ? entry.resource.bytes : null, modifiedAtNs: null };
        });
      }
      await overrides.getAcpSession!(input.projectId ?? 'default', input.taskId, input.runId, input.roundId, input.nodeId, input.attemptId, {}, null, input.outerNodeId, input.outerAttemptId);
      const path = input.relativePath ?? '';
      if (path !== '' && path !== 'reports') throw { code: 'demo.resource-not-found', params: { path } };
      const entries = path === '' ? [{ name: 'reports', kind: 'directory' as const }] : [{ name: 'review.md', kind: 'file' as const }];
      return entries.map((entry) => ({ ...entry, relativePath: path ? `${path}/${entry.name}` : entry.name,
        canonicalPath: `/demo/runs/${input.taskId}/${input.runId}/${input.roundId}/${input.nodeId}/${path ? `${path}/` : ''}${entry.name}`,
        hasChildren: entry.kind === 'directory', byteLength: null, modifiedAtNs: null }));
    },
    async readConversationDirectoryFile(input) {
      const data = await source(input.projectId, input.taskId, input.runId);
      if (data) {
        const session = archiveSession(data.catalog, { ...input, projectId: input.projectId! });
        return overrides.readFileResource!(session.projectId, archiveDirectoryPath(session, input.relativePath ?? ''));
      }
      await overrides.getAcpSession!(input.projectId ?? 'default', input.taskId, input.runId, input.roundId, input.nodeId, input.attemptId, {}, null, input.outerNodeId, input.outerAttemptId);
      if (input.relativePath !== 'reports/review.md') throw { code: 'demo.resource-not-found', params: { path: input.relativePath } };
      const snapshot = await overrides.readFileResource!('default', DEMO_REPORT_PATH);
      return { ...snapshot, name: 'review.md', locator: { projectId: 'default', canonicalPath: `/demo/runs/${input.taskId}/${input.runId}/${input.roundId}/${input.nodeId}/reports/review.md`, relativePath: 'reports/review.md', scope: 'workspace' } };
    },
    async getTurnFileChangeSet(locator, changeSetId) {
      const data = await source(locator.projectId, locator.taskId, locator.runId);
      if (data) {
        const files = await data.reader.files(archiveSession(data.catalog, locator), locator.branchId, changeSetId);
        return { ...files.changeSet, startedAt: archiveTimestamp(files.changeSet.startedAt) ?? files.changeSet.startedAt,
          finishedAt: archiveTimestamp(files.changeSet.finishedAt) };
      }
      validateDemoFileLocator(locator, changeSetId);
      const manifest = structuredClone(demoDevelopmentFiles);
      manifest.attachments[0].byteLength = new TextEncoder().encode(demoReportContent(state.getPreferences().language)).length;
      return manifest;
    },
    async getFileComparison(locator, changeSetId, changeId) {
      const data = await source(locator.projectId, locator.taskId, locator.runId);
      if (data) return data.reader.comparison(archiveSession(data.catalog, locator), locator.branchId, changeSetId, changeId);
      validateDemoFileLocator(locator, changeSetId);
      if (!demoDevelopmentFiles.changes.some((change) => change.id === changeId)) throw { code: 'demo.resource-not-found', params: { changeId } };
      return structuredClone(await previewApi.getFileComparison(locator, changeSetId, changeId));
    },
    async resolveTurnAttachmentFile(locator, changeSetId, attachmentId) {
      const data = await source(locator.projectId, locator.taskId, locator.runId);
      if (data) {
        const files = await data.reader.files(archiveSession(data.catalog, locator), locator.branchId, changeSetId);
        const attachment = files.changeSet.attachments.find(item => item.id === attachmentId);
        if (!attachment || !files.attachments[attachmentId]) throw { code: 'demo.resource-not-found', params: {} };
        return { locator: { projectId: locator.projectId, canonicalPath: archiveAttachmentPath(locator, changeSetId, attachmentId),
          relativePath: attachment.relativePath, scope: 'workspace' }, target: null, externalAccessGrant: null };
      }
      validateDemoFileLocator(locator, changeSetId);
      if (attachmentId !== demoDevelopmentFiles.attachments[0].id) throw { code: 'demo.resource-not-found', params: { attachmentId } };
      return { locator: { projectId: locator.projectId, canonicalPath: DEMO_REPORT_PATH, relativePath: demoDevelopmentFiles.attachments[0].relativePath, scope: 'workspace' }, target: null, externalAccessGrant: null };
    },
    async getWorkflowTemplates() { return structuredClone(demoWorkflowTemplates); },
    async getAutoTemplates() { return demoAutoTemplates(state.getPreferences().language); },
    async listMcpServers() {
      return [
        { id: 'demo-http', name: 'Project Docs', enabled: true, transport: 'http', url: 'https://docs.example.com/mcp', managed: false, healthStatus: 'healthy' },
        { id: 'demo-sse', name: 'Project Search', enabled: true, transport: 'sse', url: 'https://search.example.com/sse', managed: false, healthStatus: 'healthy' },
      ];
    },
    async listMcpTools(id) {
      if (id !== 'demo-http' && id !== 'demo-sse') throw { code: 'demo.resource-not-found', params: { id } };
      const en = state.getPreferences().language === 'en';
      return [{ name: id === 'demo-http' ? 'read_document' : 'search_project', description: en ? 'Find project documentation.' : '查询项目文档。', inputSchema: { type: 'object', properties: { query: { type: 'string' } }, required: ['query'] } }];
    },
    async getAgentRegistry() { return structuredClone(demoAgentRegistry); },
    async getProfiles() {
      const profiles = demoProfiles(state.getPreferences().language);
      const en = state.getPreferences().language === 'en';
      return { profiles: [...profiles, { ...profiles[0], id: 'demo-reviewer', name: en ? 'Project reviewer' : '项目审阅员',
        summary: skill().meta.description, content: skill().body, scope: 'user', isBuiltIn: false, path: '/demo/profiles/reviewer.md' }] };
    },
    async getProfile(id) {
      const profile = (await overrides.getProfiles!()).profiles.find((item) => item.id === id);
      if (!profile) throw { code: 'demo.resource-not-found', params: { id } };
      return profile.isBuiltIn ? { ...profile, content: await demoProfileContent(id, state.getPreferences().language) } : profile;
    },
    async getConversationRun(projectId, taskId, runId) {
      const data = await source(projectId, taskId, runId);
      if (data) return data.reader.run(data.catalog);
      if (projectId !== 'default' || !DEMO_TASKS.includes(taskId as typeof DEMO_TASKS[number]) || !demoRunIds(taskId).includes(runId)) {
        throw { code: 'demo.resource-not-found', params: { projectId, taskId, runId } };
      }
      return demoRun(await previewApi.getConversationRun('default', 'mock-task', DEMO_RUN_ID), taskId, state.getPreferences().language, runId);
    },
    async getAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId, query, _fallback, outerNodeId, outerAttemptId) {
      const data = await source(projectId, taskId, runId);
      if (data) {
        const session = archiveSession(data.catalog, { projectId: projectId!, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
        return archiveSessionVm(session, query?.branchId ?? 'root', await data.reader.history(session, query));
      }
      if ((query?.branchId ?? 'root') !== 'root' || outerNodeId != null || outerAttemptId != null) {
        throw { code: 'demo.resource-not-found', params: { branchId: query?.branchId, outerNodeId, outerAttemptId } };
      }
      const run = await overrides.getConversationRun!(projectId ?? 'default', taskId, runId);
      const leaf = run.sessionTree.rounds.find((round) => round.roundId === roundId)?.nodes.find((node) => node.nodeId === nodeId)?.attempts.find((attempt) => attempt.attemptId === attemptId);
      if (!leaf) throw { code: 'demo.resource-not-found', params: { roundId, nodeId, attemptId } };
      return demoSessionForNode(run, roundId, nodeId, state.getPreferences().language);
    },
    async getAcpActivityDetail(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId) {
      const data = await source(projectId, taskId, runId);
      if (data) {
        const session = archiveSession(data.catalog, { projectId: projectId!, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
        const detail = await data.reader.activity(session, query);
        return { ...detail, items: detail.items.map(archiveEvent) };
      }
      const session = await overrides.getAcpSession!(projectId, taskId, runId, roundId, nodeId, attemptId);
      return { items: session!.events.filter((event) => event.kind === 'toolCall'), hasMoreEarlier: false, earlierCursor: null };
    },
    async getAcpToolDetail(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId) {
      const data = await source(projectId, taskId, runId);
      if (data) {
        const session = archiveSession(data.catalog, { projectId: projectId!, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
        const detail = await data.reader.detail(session);
        if (detail.snapshot?.sessionId !== query.sessionId) throw { code: 'demo.resource-not-found', params: {} };
        const event = await data.reader.tool(session, query.branchId, query.eventId);
        if (query.toolCallId != null && query.toolCallId !== event.toolCallId) throw { code: 'demo.resource-not-found', params: {} };
        return { event: archiveEvent(event) };
      }
      const session = await overrides.getAcpSession!(projectId, taskId, runId, roundId, nodeId, attemptId, { branchId: query.branchId }, null, outerNodeId, outerAttemptId);
      if (session?.sessionId !== query.sessionId) throw { code: 'demo.resource-not-found', params: { sessionId: query.sessionId } };
      return { event: session.events.find((event) => event.kind === 'toolCall' && event.id === query.eventId
        && (query.toolCallId == null || event.toolCallId === query.toolCallId)) ?? null };
    },
    async getAcpRawFrames(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId) {
      const data = await source(projectId, taskId, runId);
      if (data) {
        const session = archiveSession(data.catalog, { projectId: projectId!, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
        if (query?.search?.trim() || query?.kind?.trim() || query?.direction?.trim()) return data.reader.rawQuery(session, query);
        return data.reader.rawPage(session, query?.page, query?.pageSize, query?.order);
      }
      await overrides.getAcpSession!(projectId, taskId, runId, roundId, nodeId, attemptId, {}, null, outerNodeId, outerAttemptId);
      return previewApi.getAcpRawFrames(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId);
    },
    async getAppBootstrap() { return { ...state.getAppBootstrap(), repoRoot: '/default', recentWorkspaces: ['/default'] }; },
    async saveDesktopPreferences(appearance, personalization, language) {
      const next = state.setPreferences({ ...state.getPreferences(), appearance, personalization, language });
      writeDemoPreferences(storage, next);
      return next;
    },
    async getConversationSidebarBootstrap() {
      return { workspaces: await overrides.getConversationWorkspaces!(), pinRefs: [], lastActiveWorkspaceId: 'default', preferences: { ...layout } };
    },
    async saveConversationPreference(key, value) {
      const next = { ...layout, [key]: value };
      try { writeDemoLayout(storage, next); }
      catch { throw { code: 'demo.operation-unavailable', params: { operation: 'saveConversationPreference', key } }; }
      layout = next;
    },
    async getConversationWorkspaces() { return [{ projectId: 'default', workspacePath: '/default', name: 'Gold Band' }]; },
    async listSkills() { return { global: [skill().meta], project: [] }; },
    async listProjectSkills() { return []; },
    async readSkill(name) {
      const content = skill();
      if (name !== content.meta.name) throw { code: 'demo.resource-not-found', params: { name } };
      return content;
    },
    async getSkillSyncStatus() { return demoAgentRegistry.agents.map((agent) => ({ agentType: agent.agentType, isSynced: true })); },
    async resolveWorkspaceFileLink(projectId, rawHref, baseCanonicalPath = null, origin) {
      if (projectId === 'default' && ![rawHref, baseCanonicalPath ?? ''].some(path => [ARCHIVE_DIRECTORY_PREFIX, ARCHIVE_ATTACHMENT_PREFIX, ARCHIVE_SOURCE_PREFIX].some(prefix => path.startsWith(prefix)))) {
        return previewApi.resolveWorkspaceFileLink(projectId, rawHref, baseCanonicalPath);
      }
      const parsed = parseLocalFileLinkTarget(rawHref);
      const href = parsed ? rawHref.trim().slice(0, -parsed.sourceSuffix.length) : rawHref.trim();
      if (origin && origin.locator.projectId !== projectId) throw { code: 'demo.resource-not-found', params: {} };
      const historical = archive && origin && !baseCanonicalPath
        && ![ARCHIVE_DIRECTORY_PREFIX, ARCHIVE_ATTACHMENT_PREFIX, ARCHIVE_SOURCE_PREFIX].some(prefix => href.startsWith(prefix))
        && !isArchiveRunSourceReference(await archive.catalog(), projectId, href);
      const { file, canonicalPath } = href.startsWith(ARCHIVE_SOURCE_PREFIX)
        ? await historicalPathReference(projectId, href)
        : historical ? await historicalReference(projectId, origin!, rawHref.trim())
          : await directoryReference(projectId, href, baseCanonicalPath);
      return { locator: { projectId, canonicalPath, relativePath: file.relativePath, scope: 'workspace' },
        target: parsed ? { line: parsed.line, column: parsed.column, endLine: parsed.endLine } : null, externalAccessGrant: null };
    },
    async resolveMarkdownImage(input) {
      if (input.projectId === 'default' && ![ARCHIVE_DIRECTORY_PREFIX, ARCHIVE_ATTACHMENT_PREFIX].some(prefix => input.markdownCanonicalPath.startsWith(prefix))) return previewApi.resolveMarkdownImage(input);
      if (/^(?:[a-z][a-z\d+.-]*:|\/\/)/iu.test(input.rawSrc.trim())) throw { code: 'workspace-file.markdown-image-network-blocked', params: {} };
      const { file, canonicalPath } = await directoryReference(input.projectId, input.rawSrc.trim(), input.markdownCanonicalPath);
      if (!file.image) return { kind: 'unsupported', limitationCode: 'workspace-file.format-unsupported' };
      if (file.resource.bytes > ARCHIVE_FILE_LIMITS.imageBytes || file.image.width * file.image.height > ARCHIVE_FILE_LIMITS.imagePixels) {
        return { kind: 'unsupported', limitationCode: 'workspace-file.too-large' };
      }
      return { kind: 'ready', canonicalPath, ...file.image,
        previewGrant: { token: `archive-resource:${file.resource.sha256}`, expiresAtMs: String(Date.now() + ARCHIVE_FILE_LIMITS.previewLifetimeMs) } };
    },
    async readFileResource(...args) {
      if (archive && [ARCHIVE_DIRECTORY_PREFIX, ARCHIVE_SOURCE_PREFIX].some(prefix => args[1].startsWith(prefix))) {
        const { file, canonicalPath } = args[1].startsWith(ARCHIVE_SOURCE_PREFIX)
          ? await historicalPathReference(args[0], args[1])
          : await directoryReference(args[0], args[1]);
        const base = { locator: { projectId: args[0], canonicalPath, relativePath: file.relativePath, scope: 'workspace' as const },
          name: file.name, revision: { contentHash: file.resource.sha256, byteLength: file.resource.bytes, modifiedAtNs: '0' }, externalAccessGrant: null };
        if (file.image) {
          if (file.resource.bytes > ARCHIVE_FILE_LIMITS.imageBytes || file.image.width * file.image.height > ARCHIVE_FILE_LIMITS.imagePixels) {
            return { ...base, kind: 'unsupported', mimeType: file.image.mimeType, limitationCode: 'workspace-file.too-large' };
          }
          return { ...base, kind: 'image', ...file.image, sourceEditable: false,
            previewGrant: { token: `archive-resource:${file.resource.sha256}`, expiresAtMs: String(Date.now() + ARCHIVE_FILE_LIMITS.previewLifetimeMs) } };
        }
        if (file.resource.bytes > ARCHIVE_FILE_LIMITS.textBytes) return { ...base, kind: 'unsupported', mimeType: null, limitationCode: 'workspace-file.too-large' };
        const bytes = await archive.reader.resourceBytes(file.resource);
        let content: string;
        try { content = new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
        catch { return { ...base, kind: 'unsupported', mimeType: null, limitationCode: 'workspace-file.format-unsupported' }; }
        if (content.includes('\0')) return { ...base, kind: 'unsupported', mimeType: null, limitationCode: 'workspace-file.format-unsupported' };
        const extension = file.name.split('.').at(-1)?.toLowerCase() ?? '';
        return { ...base, kind: 'text', content, encoding: 'utf-8', language: extension === 'md' ? 'markdown' : ['jsonl', 'ndjson'].includes(extension) ? 'json' : extension,
          lineEnding: content.includes('\r\n') ? content.replaceAll('\r\n', '').includes('\n') ? 'mixed' : 'crlf' : 'lf', editable: false, limitationCode: null };
      }
      if (archive && args[0] !== 'default') {
        const { locator, changeSetId, attachmentId } = parseArchiveAttachmentPath(args[1]);
        if (locator.projectId !== args[0]) throw { code: 'demo.resource-not-found', params: {} };
        const data = (await source(locator.projectId, locator.taskId, locator.runId))!;
        const files = await data.reader.files(archiveSession(data.catalog, locator), locator.branchId, changeSetId);
        const attachment = files.changeSet.attachments.find(item => item.id === attachmentId);
        const resource = files.attachments[attachmentId];
        if (!attachment || !resource) throw { code: 'demo.resource-not-found', params: {} };
        const base = { locator: { projectId: locator.projectId, canonicalPath: args[1], relativePath: attachment.relativePath, scope: 'workspace' as const },
          name: attachment.name, revision: { contentHash: resource.sha256, byteLength: resource.bytes, modifiedAtNs: '0' }, externalAccessGrant: null };
        if (resource.kind === 'text' && resource.bytes <= ARCHIVE_FILE_LIMITS.textBytes) {
          const extension = attachment.name.split('.').at(-1)?.toLowerCase() ?? '';
          const language = ({ md: 'markdown', mdx: 'markdown', ndjson: 'json', jsonl: 'json', rs: 'rust', ts: 'typescript', js: 'javascript', py: 'python' } as Record<string, string>)[extension] ?? extension;
          return { ...base, kind: 'text', content: await data.reader.resourceText(resource), encoding: resource.encoding,
            language, lineEnding: resource.lineEnding, editable: false, limitationCode: null };
        }
        if (resource.kind === 'image' && resource.bytes <= ARCHIVE_FILE_LIMITS.imageBytes && resource.width * resource.height <= ARCHIVE_FILE_LIMITS.imagePixels) {
          return { ...base, kind: 'image', mimeType: resource.mimeType, width: resource.width, height: resource.height, animated: resource.animated,
            previewGrant: { token: `archive-resource:${resource.sha256}`, expiresAtMs: String(Date.now() + ARCHIVE_FILE_LIMITS.previewLifetimeMs) }, sourceEditable: false };
        }
        return { ...base, kind: 'unsupported', mimeType: resource.kind === 'image' ? resource.mimeType : null,
          limitationCode: resource.kind === 'unsupported' ? 'workspace-file.format-unsupported' : 'workspace-file.too-large' };
      }
      if (args[0] === 'default' && args[1] === DEMO_REPORT_PATH) {
        const content = demoReportContent(state.getPreferences().language);
        return { kind: 'text', locator: { projectId: 'default', canonicalPath: DEMO_REPORT_PATH, relativePath: 'demo-report.md', scope: 'workspace' },
          name: 'demo-report.md', content, encoding: 'utf-8', language: 'markdown', lineEnding: 'lf', editable: false, limitationCode: null,
          revision: { contentHash: `demo-report-${state.getPreferences().language}`, byteLength: new TextEncoder().encode(content).length, modifiedAtNs: '1788249660000000000' }, externalAccessGrant: null };
      }
      const snapshot = await previewApi.readFileResource(...args);
      if (snapshot.kind === 'text' && args[1] === '/default/README.md') {
        const content = state.getPreferences().language === 'en'
          ? '# Workspace notes\n\n## Project structure\n\n| File | Purpose |\n| --- | --- |\n| src/main.rs | Application entry point |\n| src/config.json | Workspace configuration |\n| assets/logo.svg | Product identity |\n\n## Review checklist\n\n- Validate configuration defaults.\n- Keep changes scoped to the requested modules.\n- Add regression coverage for behavior changes.\n\n## Acceptance\n\nThe application starts successfully and reads the expected configuration.\n'
          : '# 工作区说明\n\n## 项目结构\n\n| 文件 | 职责 |\n| --- | --- |\n| src/main.rs | 应用入口 |\n| src/config.json | 工作区配置 |\n| assets/logo.svg | 产品标识 |\n\n## 审阅清单\n\n- 检查配置字段的默认值。\n- 将修改限定在本次需求涉及的模块。\n- 为行为变化补充回归测试。\n\n## 验收\n\n应用正常启动，并读取预期的配置内容。\n';
        return { ...snapshot, content, editable: false, revision: { ...snapshot.revision,
          byteLength: new TextEncoder().encode(content).length, contentHash: `demo-readme-${state.getPreferences().language}-1` } };
      }
      return snapshot.kind === 'text' ? { ...snapshot, editable: false } : snapshot;
    },
    workspaceFilePreviewUrl: (...args) => archive && /^archive-resource:[a-f0-9]{64}$/.test(args[0])
      ? archive.reader.resourceUrl(`resources/${args[0].slice('archive-resource:'.length)}`) : previewApi.workspaceFilePreviewUrl(...args),
    async openExternalUrl(href) {
      let url: URL;
      try { url = new URL(href); }
      catch { throw { code: 'demo.operation-unavailable', params: { operation: 'openExternalUrl' } }; }
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) {
        throw { code: 'demo.operation-unavailable', params: { operation: 'openExternalUrl' } };
      }
      await previewApi.openExternalUrl(url.href);
    },
    async getSystemFonts() { return ['Arial', 'Georgia', 'Consolas', 'Courier New']; },
  };
  function skill(): SkillContentVm {
    const en = state.getPreferences().language === 'en';
    return {
      meta: { name: 'project-review', description: en ? 'Review project structure and document the result.' : '检查项目结构，整理问题并记录审阅结果。', source: 'global', directoryPath: '/demo/skills/project-review', agentSource: '.gold-band', loadWarnings: [], syncedAgentTypes: demoAgentRegistry.agents.map((agent) => agent.agentType) },
      body: en ? '# Project review\n\n1. Read the project documentation.\n2. Inspect the relevant implementation.\n3. Record findings with file references.\n4. Verify the result.\n' : '# 项目审阅\n\n1. 阅读项目说明与设计文档。\n2. 检查相关实现。\n3. 记录问题及对应的文件位置。\n4. 验证结果。\n',
    };
  }
  return new Proxy({} as RuntimeApi, {
    get(_target, property) {
      if (property === 'then' || typeof property !== 'string') return undefined;
      if (methods.has(property)) return methods.get(property);
      const key = property as keyof RuntimeApi;
      let method: unknown = overrides[key];
      if (!method && subscriptions.has(key)) method = async () => () => {};
      if (!method && quietMethods.has(key)) method = async () => {};
      if (!method && readMethods.has(property)) {
        method = async (...args: unknown[]) => structuredClone(await Reflect.apply(previewApi[key] as (...args: unknown[]) => unknown, previewApi, args));
      }
      if (!method) method = async () => { throw { code: 'demo.operation-unavailable', params: { operation: property } }; };
      if (presetProjectMethods.has(key)) {
        const implementation = method as (...args: unknown[]) => unknown;
        method = async (...args: unknown[]) => {
          if (args[0] !== DEMO_PROJECT_ID && !(key === 'getGitCapability' && args[0] == null)) {
            throw { code: 'demo.resource-not-found', params: {} };
          }
          return implementation(...args);
        };
      }
      methods.set(property, method);
      return method;
    },
  });
}
