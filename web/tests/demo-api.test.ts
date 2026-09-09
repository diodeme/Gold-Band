import { describe, expect, it } from 'vitest';
import { createDemoApi } from '../../marketing/demo/api';
import { DEMO_PREFERENCES_KEY } from '../../marketing/demo/preferences';
import { browserApi } from '../src/api/browser';
import { demoSidebar, DEMO_TASKS, DEMO_RUN_ID } from '../../marketing/demo/fixtures';
import { mcpAgentSupportStatus } from '../src/lib/mcp-agent-compatibility';
import { configuredSkillAgents, skillSourceAgents } from '../src/lib/skill-agent-display';

describe('public demo runtime', () => {
  it('exposes multiple workflow runs and rounds without crossing session identities', async () => {
    const api = createDemoApi();
    const row = demoSidebar('en').tasksByWorkspace.default.find((task) => task.taskId === 'demo-review')!;
    expect(row.runs.map((run) => run.runId)).toEqual(['run-052', 'run-051']);
    expect(row.latestRun).toEqual(row.runs[0]);
    const identities = new Set<string>();
    const contents = new Set<string>();
    for (const summary of row.runs) {
      const run = await api.getConversationRun('default', row.taskId, summary.runId);
      expect(run.runId).toBe(summary.runId);
      expect(run.sessionTree.rounds.map((round) => round.roundId)).toEqual(['round-001', 'round-002']);
      for (const round of run.sessionTree.rounds) {
        for (const node of round.nodes) {
          const leaf = node.attempts[0];
          const session = await api.getAcpSession('default', row.taskId, summary.runId, round.roundId, node.nodeId, leaf.attemptId);
          expect(session).toMatchObject({ roundId: round.roundId, nodeId: node.nodeId, sessionId: leaf.sessionId });
          expect(identities.has(session!.sessionId)).toBe(false);
          identities.add(session!.sessionId);
          contents.add(session!.events.at(-1)!.content!);
          expect(session!.events.filter((event) => event.kind === 'fileChangeSet')).toHaveLength(node.nodeId === 'dev-test' ? 1 : 0);
          if (node.nodeId === 'dev-test') {
            const event = session!.events.find((event) => event.kind === 'fileChangeSet')!;
            const manifest = await api.getTurnFileChangeSet({ projectId: 'default', taskId: row.taskId, runId: summary.runId, roundId: round.roundId, nodeId: node.nodeId, attemptId: leaf.attemptId, branchId: 'root' }, (event.raw as { changeSetId: string }).changeSetId);
            expect(manifest.attachments).toHaveLength(1);
          }
          if (node.nodeId === 'accept') expect(leaf.outcome).toBe(round.index === 1 ? 'failure' : 'success');
        }
      }
    }
    expect(contents.size).toBe(12);
    await expect(api.getAcpSession('default', 'demo-review', 'run-051', 'round-003', 'accept', 'attempt-001')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('provides the Direct Agent icon identity', async () => {
    const row = demoSidebar('zh-cn').tasksByWorkspace.default[0];
    const run = await createDemoApi().getConversationRun('default', row.taskId, DEMO_RUN_ID);
    expect(row.agentIdentity?.iconKey).toBe('claude');
    expect(row.agentIdentity).toEqual(run.agentIdentity);
  });

  it('provides development attachments and file comparisons through the readonly APIs', async () => {
    const api = createDemoApi();
    const locator = { projectId: 'default', taskId: 'demo-review', runId: DEMO_RUN_ID, roundId: 'round-001', nodeId: 'dev-test', attemptId: 'attempt-001', branchId: 'root' };
    const session = await api.getAcpSession(locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId);
    const event = session!.events.find((event) => event.kind === 'fileChangeSet');
    expect(event).toBeDefined();
    const id = (event!.raw as { changeSetId: string }).changeSetId;
    const manifest = await api.getTurnFileChangeSet(locator, id);
    expect(manifest.changes).toHaveLength(2);
    expect(manifest.attachments).toHaveLength(1);
    expect(session!.eventPage?.loadedCount).toBe(session!.events.length);
    expect(session!.diagnostics.eventCount).toBe(session!.events.length);
    for (const change of manifest.changes) {
      expect((await api.getFileComparison(locator, id, change.id)).after?.content).toBeTruthy();
    }
    const file = await api.resolveTurnAttachmentFile(locator, id, manifest.attachments[0].id);
    const snapshot = await api.readFileResource(locator.projectId, file.locator.canonicalPath);
    expect(snapshot).toMatchObject({ kind: 'text', editable: false });
    if (snapshot.kind === 'text') expect(new TextEncoder().encode(snapshot.content).length).toBe(manifest.attachments[0].byteLength);
    await expect(api.getTurnFileChangeSet({ ...locator, nodeId: 'accept' }, id)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('rejects business and platform writes without modifying preview data', async () => {
    const api = createDemoApi();
    const before = await browserApi.getProfiles();
    for (const method of ['writeFileResource', 'writeSkill', 'deleteSkill', 'saveTaskWorkflow', 'createProfile',
      'submitConversationPrompt', 'createConversationRun', 'createAgent', 'updateAgent', 'deleteAgent', 'doctorAgent', 'syncSkillToAgents', 'executeGitMutation', 'startGitOperation',
      'pickLocalDirectory', 'openFileWithSystemApp', 'submitFeedback', 'downloadAndInstallUpdate'] as const) {
      await expect(Reflect.apply(api[method], api, [])).rejects.toMatchObject({ code: 'demo.operation-unavailable', params: { operation: method } });
    }
    expect(await browserApi.getProfiles()).toEqual(before);
  });

  it('exposes every preset Agent, linked Skill targets and real bilingual built-in role bodies', async () => {
    const api = createDemoApi();
    const registry = await api.getAgentRegistry();
    expect(registry.agents.map((agent) => agent.agentType)).toEqual(registry.catalog.map((entry) => entry.agentType));
    const skills = await api.listSkills();
    expect(skillSourceAgents(skills.global[0], configuredSkillAgents(registry))[0].iconKey).toBe('gold-band');
    expect(skills.global[0].syncedAgentTypes).toEqual(registry.agents.map((agent) => agent.agentType));
    const profiles = (await api.getProfiles()).profiles.filter((profile) => profile.isBuiltIn);
    expect(profiles).toHaveLength(9);
    for (const profile of profiles) {
      expect((await api.getProfile(profile.id)).content.length).toBeGreaterThan(profile.content.length);
    }
    const preferences = (await api.getAppBootstrap()).preferences;
    const chinese = await api.getProfile(profiles[0].id);
    await api.saveDesktopPreferences(preferences.appearance, preferences.personalization, 'en', false, false);
    expect((await api.getProfile(profiles[0].id)).content).not.toBe(chinese.content);
    const workflows = await api.getWorkflowTemplates();
    expect(workflows.templates.map((template) => template.id)).toEqual(['default', 'default-lightweight']);
  });

  it('provides HTTP and SSE fixtures with explicit Codex SSE incompatibility', async () => {
    const api = createDemoApi();
    const servers = await api.listMcpServers();
    expect(servers.map((server) => server.transport)).toEqual(['http', 'sse']);
    const agents = (await api.getAgentRegistry()).agents.filter((agent) => agent.mcpHttpSupported != null);
    expect(agents.map((agent) => agent.agentType)).toEqual(['claude-acp', 'codex-acp']);
    for (const agent of agents) {
      const vm = { ...agent, label: agent.displayName, diagnosticAvailable: true };
      expect(mcpAgentSupportStatus('http', vm)).toBe('supported');
      expect(mcpAgentSupportStatus('sse', vm)).toBe(agent.agentType === 'codex-acp' ? 'unsupported' : 'supported');
    }
    for (const server of servers) expect((await api.listMcpTools(server.id)).length).toBeGreaterThan(0);
    for (const method of ['addMcpServer', 'updateMcpServer', 'deleteMcpServer', 'toggleMcpServer', 'checkMcpServerHealth'] as const) {
      await expect(Reflect.apply(api[method], api, [])).rejects.toMatchObject({ code: 'demo.operation-unavailable' });
    }
  });

  it('returns coherent, independent preset sessions and rejects unknown identities', async () => {
    const api = createDemoApi();
    const sidebar = demoSidebar('zh-cn');
    for (const taskId of DEMO_TASKS) {
      const row = sidebar.tasksByWorkspace.default.find((task) => task.taskId === taskId)!;
      const run = await api.getConversationRun(row.projectId, taskId, row.latestRun!.runId);
      expect(run.taskUuid).toBe(row.taskUuid);
      expect(run.runStatus).toBe('completed');
      expect(run.activeSessions).toEqual([]);
      const nodeId = taskId === 'demo-review' ? 'accept' : 'dev';
      expect(run.runMode).toBe(taskId === 'demo-review' ? 'workflow' : 'direct');
      const session = await api.getAcpSession('default', taskId, DEMO_RUN_ID, run.selectedSession!.roundId!, nodeId, 'attempt-001');
      expect(session).toEqual(run.selectedSession);
      session!.events[0].content = 'visitor change';
      expect((await api.getConversationRun('default', taskId, DEMO_RUN_ID)).selectedSession!.events[0].content).not.toBe('visitor change');
    }
    await expect(api.getConversationRun('another-project', 'mock-task', DEMO_RUN_ID)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });

  it('serves distinct workflow node sessions with matching identities and content', async () => {
    const api = createDemoApi();
    const run = await api.getConversationRun('default', 'demo-review', DEMO_RUN_ID);
    expect(run.workflowTemplateId).toBe('default-lightweight');
    expect(run.sessionTree.rounds[0].nodes.map((node) => node.nodeId)).toEqual(['grill', 'dev-test', 'accept']);
    const results: string[] = [];
    for (const node of run.sessionTree.rounds[0].nodes) {
      const leaf = node.attempts[0];
      const session = await api.getAcpSession('default', 'demo-review', DEMO_RUN_ID, leaf.roundId, leaf.nodeId, leaf.attemptId);
      expect(session).toMatchObject({ nodeId: leaf.nodeId, sessionId: leaf.sessionId, title: node.label });
      results.push(session!.events.at(-1)!.content!);
    }
    expect(new Set(results).size).toBe(3);
    await expect(api.getAcpSession('default', 'demo-review', DEMO_RUN_ID, 'round-001', 'missing', 'attempt-001')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });

  it('reuses file reading while advertising readonly content and serving Skill bodies', async () => {
    const api = createDemoApi();
    const snapshot = await api.readFileResource('default', '/default/README.md');
    expect(snapshot).toMatchObject({ kind: 'text', editable: false });
    const { global: skills } = await api.listSkills();
    expect(skills).toHaveLength(1);
    expect((await api.readSkill(skills[0].name, skills[0].source)).body).toContain('#');
    await expect(api.readSkill('missing', 'global')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    const profiles = await api.getProfiles();
    const custom = profiles.profiles.find((profile) => !profile.isBuiltIn)!;
    expect((await api.getProfile(custom.id)).content).toContain('#');
  });

  it('remembers bounded layout preferences without allowing arbitrary configuration writes', async () => {
    const values = new Map<string, string>();
    const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value); } };
    const api = createDemoApi(storage);
    await api.saveConversationPreference('sidebar.width', 240);
    expect((await createDemoApi(storage).getConversationSidebarBootstrap()).preferences['sidebar.width']).toBe(240);
    await expect(api.saveConversationPreference('agent.command', 'run')).rejects.toMatchObject({ code: 'demo.operation-unavailable' });
    expect((await api.getConversationSidebarBootstrap()).preferences).toEqual({ 'sidebar.width': 240 });
  });

  it('persists only appearance preferences and isolates desktop and other visitors', async () => {
    const values = new Map<string, string>();
    const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value); } };
    const desktopBefore = (await browserApi.getAppBootstrap()).preferences;
    const api = createDemoApi(storage);
    const { preferences } = await api.getAppBootstrap();
    await api.saveDesktopPreferences({ ...preferences.appearance, colorScheme: 'light' }, {
      ...preferences.personalization,
      typography: { ...preferences.personalization.typography, ui: { fontStack: { source: 'custom', families: ['Georgia'] }, fontSize: { source: 'custom', px: 16 } } },
    }, 'en', false, false);
    expect((await createDemoApi(storage).getAppBootstrap()).preferences).toMatchObject({ language: 'en', appearance: { colorScheme: 'light' }, personalization: { typography: { ui: { fontStack: { families: ['Georgia'] } } } } });
    expect((await createDemoApi().getAppBootstrap()).preferences.appearance.colorScheme).toBe('dark');
    expect((await browserApi.getAppBootstrap()).preferences).toEqual(desktopBefore);
    expect(Object.keys(JSON.parse(values.get(DEMO_PREFERENCES_KEY)!)).sort()).toEqual(['appearance', 'language', 'typography', 'version']);
    values.set(DEMO_PREFERENCES_KEY, '{invalid');
    expect((await createDemoApi(storage).getAppBootstrap()).preferences.appearance.colorScheme).toBe('dark');
  });
});
