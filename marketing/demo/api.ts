import type { RuntimeApi } from '@/api/client';
import { browserApi as previewApi } from '@/api/browser';
import { BrowserPreviewState } from '@/api/browserState';
import type { SkillContentVm } from '@/types';
import { demoRun, demoSessionForNode, DEMO_TASKS, DEMO_RUN_ID } from './fixtures';
import { readDemoLayout, readDemoPreferences, writeDemoLayout, writeDemoPreferences } from './preferences';
import { demoAgentRegistry, demoProfiles, demoProfileContent, demoWorkflowTemplates } from './catalog';
import { DEMO_REPORT_PATH, demoDevelopmentFiles, demoReportContent, validateDemoFileLocator } from './turn-files';
import { demoRunIds } from './scenarios';

export const DEMO_READ_METHODS = [
  'getAgentRegistry', 'getProfiles', 'getProfile', 'getWorkflowTemplates', 'getWorkflow',
  'getConversationRun', 'getAcpSession', 'getAcpActivityDetail', 'getAcpToolDetail',
  'getAcpRawFrames', 'getSupportedAttachmentExtensions', 'getSystemFonts',
  'listWorkspaceDirectory', 'searchWorkspaceFiles', 'resolveWorkspaceFileLink',
  'readFileResource', 'resolveMarkdownImage', 'getConversationWorkspaces',
  'getSkillSyncStatus',
] as const satisfies readonly (keyof RuntimeApi)[];

const quietMethods = new Set<keyof RuntimeApi>([
  'recordActivity', 'reportFrontendError', 'updateNotificationAttention',
  'startWorkspaceFileWatch', 'stopWorkspaceFileWatch', 'releaseWorkspaceFilePreview',
  'releaseExternalFileAccess',
]);
const subscriptions = new Set<keyof RuntimeApi>([
  'subscribeAcpSessionUpdates', 'subscribeConversationRunStateUpdates',
  'subscribeConversationTerminalResultUpdates', 'subscribeWorkspaceFileChanges',
  'subscribeInterventionNavigate', 'subscribeMulticaTaskUpdates',
]);

export function createDemoApi(storage?: Pick<Storage, 'getItem' | 'setItem'>): RuntimeApi {
  const state = new BrowserPreviewState();
  const initial = state.getPreferences();
  initial.appearance.colorScheme = 'dark';
  state.setPreferences(readDemoPreferences(storage, initial));
  let layout = readDemoLayout(storage);
  const readMethods = new Set<string>(DEMO_READ_METHODS);
  const methods = new Map<PropertyKey, unknown>();
  const overrides: Partial<RuntimeApi> = {
    async getTurnFileChangeSet(locator, changeSetId) {
      validateDemoFileLocator(locator, changeSetId);
      const manifest = structuredClone(demoDevelopmentFiles);
      manifest.attachments[0].byteLength = new TextEncoder().encode(demoReportContent(state.getPreferences().language)).length;
      return manifest;
    },
    async getFileComparison(locator, changeSetId, changeId) {
      validateDemoFileLocator(locator, changeSetId);
      if (!demoDevelopmentFiles.changes.some((change) => change.id === changeId)) throw { code: 'demo.resource-not-found', params: { changeId } };
      return structuredClone(await previewApi.getFileComparison(locator, changeSetId, changeId));
    },
    async resolveTurnAttachmentFile(locator, changeSetId, attachmentId) {
      validateDemoFileLocator(locator, changeSetId);
      if (attachmentId !== demoDevelopmentFiles.attachments[0].id) throw { code: 'demo.resource-not-found', params: { attachmentId } };
      return { locator: { projectId: locator.projectId, canonicalPath: DEMO_REPORT_PATH, relativePath: demoDevelopmentFiles.attachments[0].relativePath, scope: 'workspace' }, target: null, externalAccessGrant: null };
    },
    async getWorkflowTemplates() { return structuredClone(demoWorkflowTemplates); },
    async getAutoTemplates() { return { version: '0.1', templates: [] }; },
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
      if (projectId !== 'default' || !DEMO_TASKS.includes(taskId as typeof DEMO_TASKS[number]) || !demoRunIds(taskId).includes(runId)) {
        throw { code: 'demo.resource-not-found', params: { projectId, taskId, runId } };
      }
      return demoRun(await previewApi.getConversationRun('default', 'mock-task', DEMO_RUN_ID), taskId, state.getPreferences().language, runId);
    },
    async getAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId) {
      const run = await overrides.getConversationRun!(projectId ?? 'default', taskId, runId);
      const leaf = run.sessionTree.rounds.find((round) => round.roundId === roundId)?.nodes.find((node) => node.nodeId === nodeId)?.attempts.find((attempt) => attempt.attemptId === attemptId);
      if (!leaf) throw { code: 'demo.resource-not-found', params: { roundId, nodeId, attemptId } };
      return demoSessionForNode(run, roundId, nodeId, state.getPreferences().language);
    },
    async getAcpActivityDetail(projectId, taskId, runId, roundId, nodeId, attemptId) {
      const session = await overrides.getAcpSession!(projectId, taskId, runId, roundId, nodeId, attemptId);
      return { items: session!.events.filter((event) => event.kind === 'toolCall'), hasMoreEarlier: false, earlierCursor: null };
    },
    async getAcpToolDetail(projectId, taskId, runId, roundId, nodeId, attemptId) {
      const session = await overrides.getAcpSession!(projectId, taskId, runId, roundId, nodeId, attemptId);
      return { event: session!.events.find((event) => event.kind === 'toolCall') ?? null };
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
    async readFileResource(...args) {
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
    workspaceFilePreviewUrl: (...args) => previewApi.workspaceFilePreviewUrl(...args),
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
      methods.set(property, method);
      return method;
    },
  });
}
