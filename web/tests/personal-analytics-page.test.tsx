/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  cancelPersonalAnalytics: vi.fn(),
  cancelPersonalAnalyticsInsights: vi.fn(),
  getPersonalAnalytics: vi.fn(),
  queryPersonalAnalyticsReport: vi.fn(),
  startPersonalAnalyticsInsights: vi.fn(),
  syncPersonalAnalytics: vi.fn(),
  subscribePersonalAnalyticsUpdates: vi.fn(),
}));

vi.mock('@/api', () => api);

import '../src/i18n';
import { PersonalAnalyticsPage } from '../src/pages/PersonalAnalyticsPage';
import { rememberPersonalAnalyticsSelection } from '../src/lib/personal-analytics-preferences';
import type { AgentInsightOperationStatus, AgentInsightOperationVm, AgentRegistryVm, PersonalAnalyticsOperationStatus, PersonalAnalyticsReportVm, PersonalAnalyticsSnapshotVm } from '../src/types';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let mountedRoot: Root | null = null;
const intersectionObservers: Array<{
  callback: IntersectionObserverCallback;
  observed: Element[];
}> = [];

beforeEach(() => {
  window.localStorage.clear();
  intersectionObservers.length = 0;
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.stubGlobal('IntersectionObserver', class {
    callback: IntersectionObserverCallback;
    observed: Element[] = [];
    constructor(callback: IntersectionObserverCallback) {
      this.callback = callback;
      intersectionObservers.push(this);
    }
    observe(target: Element) { this.observed.push(target); }
    unobserve(target: Element) { this.observed = this.observed.filter((current) => current !== target); }
    disconnect() { this.observed = []; }
  });
  api.getPersonalAnalytics.mockResolvedValue(snapshot(null));
  api.syncPersonalAnalytics.mockResolvedValue(snapshot('queued', 2));
  api.cancelPersonalAnalytics.mockResolvedValue(snapshot('cancelled', 2));
  api.queryPersonalAnalyticsReport.mockImplementation((range) => Promise.resolve(report([], range)));
  api.startPersonalAnalyticsInsights.mockResolvedValue(insightOperation('queued', 2));
  api.cancelPersonalAnalyticsInsights.mockResolvedValue(insightOperation('cancelled', 2));
  api.subscribePersonalAnalyticsUpdates.mockResolvedValue(() => {});
});

afterEach(async () => {
  if (mountedRoot) {
    await act(async () => mountedRoot?.unmount());
    mountedRoot = null;
  }
  document.body.replaceChildren();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

describe('PersonalAnalyticsPage', () => {
  it('does not surface historical failures or query the report while initial sync is active', async () => {
    const historicalInsightFailure = {
      ...insightOperation('failed', 3),
      error: { code: 'execution-failed', params: {} },
    };
    api.getPersonalAnalytics.mockResolvedValueOnce({
      ...snapshot('scanning', 3),
      insightOperation: historicalInsightFailure,
      latestReport: report(),
    });

    const container = await renderPage(registry(true));
    await act(async () => { await Promise.resolve(); });

    expect(api.queryPersonalAnalyticsReport).not.toHaveBeenCalled();
    expect(container.textContent).toContain('正在扫描全部历史');
    expect(container.textContent).not.toContain('洞察生成失败');
    expect(container.textContent).not.toContain('Agent 分析执行失败');
    expect(document.activeElement).toBe(container.querySelector('main'));
  });

  it('enables insight generation after sync completes without changing the selected Agent', async () => {
    let emit: ((next: PersonalAnalyticsSnapshotVm) => void) | undefined;
    api.getPersonalAnalytics.mockResolvedValueOnce(snapshot('scanning', 3));
    api.subscribePersonalAnalyticsUpdates.mockImplementation((listener: (next: PersonalAnalyticsSnapshotVm) => void) => {
      emit = listener;
      return Promise.resolve(() => {});
    });
    const container = await renderPage(registry(true));
    const insightButton = container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement;

    expect(insightButton.disabled).toBe(true);
    expect(api.queryPersonalAnalyticsReport).not.toHaveBeenCalled();

    await act(async () => {
      emit?.({ ...snapshot('completed', 4), latestReport: report() });
      await Promise.resolve();
    });
    await act(async () => { await Promise.resolve(); });

    expect(api.queryPersonalAnalyticsReport).toHaveBeenCalledWith(
      { start: null, end: null },
      'agent-a',
      undefined,
      undefined,
      undefined,
    );
    expect(insightButton.disabled).toBe(false);
  });

  it('restores and applies a remembered Agent, model, and thought level when the page is reopened', async () => {
    rememberPersonalAnalyticsSelection({
      agentType: 'agent-a',
      modelId: 'model-b',
      thoughtLevelOptionId: 'reasoning_effort',
      thoughtLevelValue: 'high',
    });
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });

    const container = await renderPage(registry(true));

    expect(container.querySelector('[data-personal-analytics-model="true"]')?.textContent).toContain('Model B');
    expect(container.querySelector('[data-personal-analytics-model="true"]')?.textContent).toContain('High');
    expect(api.queryPersonalAnalyticsReport).toHaveBeenLastCalledWith(
      { start: null, end: null },
      'agent-a',
      'model-b',
      'reasoning_effort',
      'high',
    );

    await act(async () => {
      (container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement).click();
      await Promise.resolve();
    });
    expect(api.startPersonalAnalyticsInsights).toHaveBeenCalledWith(
      'agent-a',
      { start: null, end: null },
      'model-b',
      'reasoning_effort',
      'high',
    );
  });

  it('keeps the header controls adaptive when Agent and model selectors are both present', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const container = await renderPage(registry(true));

    const actions = container.querySelector('[data-personal-analytics-header-actions="true"]');
    const controls = container.querySelector('[data-personal-analytics-controls="true"]');
    expect(actions?.className).toContain('min-w-0');
    expect(actions?.className).not.toContain('md:max-w-[36rem]');
    expect(controls?.className).toContain('flex-wrap');
    expect(controls?.className).not.toContain('sm:flex-row');
  });

  it('reuses the composer model selector and gives deterministic sync refresh semantics', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const modelOnlyRegistry = registry(true);
    modelOnlyRegistry.agents[0].configOptions = null;
    const container = await renderPage(modelOnlyRegistry);

    expect(container.querySelector('label[for="personal-analytics-agent"]')).toBeNull();
    expect(container.querySelector('[data-personal-analytics-model="true"]')?.textContent).toContain('模型');
    expect(container.querySelector('[data-personal-analytics-model="true"]')?.textContent).toContain('不指定');

    const modelTrigger = container.querySelector<HTMLButtonElement>('[data-personal-analytics-model="true"] [data-slot="dropdown-menu-trigger"]')!;
    await act(async () => {
      modelTrigger.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }));
    });
    const modelB = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"]'))
      .find((item) => item.textContent?.includes('Model B'))!;
    await act(async () => {
      modelB.click();
      await Promise.resolve();
    });
    expect(api.queryPersonalAnalyticsReport).toHaveBeenLastCalledWith(
      { start: null, end: null },
      'agent-a',
      'model-b',
      undefined,
      undefined,
    );

    await act(async () => {
      (container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement).click();
      await Promise.resolve();
    });
    expect(api.startPersonalAnalyticsInsights).toHaveBeenCalledWith(
      'agent-a',
      { start: null, end: null },
      'model-b',
      undefined,
      undefined,
    );

    const refreshButton = buttonByText(container, '刷新');
    const insightButton = container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement;
    expect(refreshButton?.querySelector('svg')?.classList.contains('lucide-refresh-cw')).toBe(true);
    expect(insightButton.querySelector('svg')?.classList.contains('lucide-sparkles')).toBe(true);
  });

  it('keeps deterministic sync available and exposes Agent management when no Agent is available', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce(snapshot('completed', 6));
    const container = await renderPage(registry(false));

    expect(startButton(container).disabled).toBe(false);
    expect(container.textContent).toContain('没有已通过环境诊断的可用 Agent');
    expect(buttonByText(container, '管理 Agent')).toBeDefined();
  });

  it('locks Agent selection and prevents a duplicate start while an operation is active', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce(snapshot('analyzing', 3));
    const container = await renderPage(registry(true));

    expect(startButton(container).disabled).toBe(true);
    expect((container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement).disabled).toBe(true);
    startButton(container).dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(api.syncPersonalAnalytics).not.toHaveBeenCalled();
  });

  it('syncs deterministically and merges the accepted snapshot into the page', async () => {
    const longAgentName = 'Agent A with an intentionally long display name for narrow layouts';
    const container = await renderPage(registry(true, longAgentName));

    await act(async () => {
      await Promise.resolve();
    });

    expect(api.syncPersonalAnalytics).toHaveBeenCalledTimes(1);
    expect(api.syncPersonalAnalytics).toHaveBeenCalledWith();
    expect(container.textContent).toContain('等待分析');
    expect(startButton(container).disabled).toBe(true);
    expect(container.textContent).toContain(longAgentName);
    expect(container.querySelector('[data-personal-analytics-agent="true"]')?.className).toContain('min-w-0');
  });

  it('opens task conversations and keeps duration beside token rankings', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const container = await renderPage(registry(true));
    const taskRows = Array.from(container.querySelectorAll('[data-personal-analytics-report="true"] tbody tr'));

    expect(taskRows).toHaveLength(3);
    for (const row of taskRows) {
      await act(async () => {
        row.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
    }
    expect(onOpenTask).toHaveBeenCalledTimes(3);
    expect(onOpenTask).toHaveBeenNthCalledWith(1, expect.objectContaining({
      projectId: 'project-a',
      taskId: 'task-workflow',
      latestRunId: 'run-1',
    }));
    expect(container.querySelector('#token-usage')?.textContent).toContain('累计执行耗时');
    expect(container.querySelector('#token-usage')?.textContent).toContain('1m');
  });

  it('keeps legacy cached report tasks visible without navigation', async () => {
    const legacyTask = { ...report().recentTasks[0] };
    delete legacyTask.projectId;
    delete legacyTask.taskId;
    delete legacyTask.latestRunId;
    const legacyReport = { ...report(), recentTasks: [legacyTask] };
    api.getPersonalAnalytics.mockResolvedValueOnce({
      ...snapshot('completed', 6),
      latestReport: legacyReport,
    });
    api.queryPersonalAnalyticsReport.mockResolvedValueOnce(legacyReport);
    const container = await renderPage(registry(true));
    const recentRow = container.querySelector('#recent-tasks tbody tr');

    expect(recentRow?.textContent).toContain('Workflow task');
    await act(async () => {
      recentRow?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(onOpenTask).not.toHaveBeenCalled();
  });

  it('exposes concise explanations for coverage field names', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const container = await renderPage(registry(true));
    const hints = Array.from(container.querySelectorAll('[data-personal-analytics-coverage-hint="true"]'));

    expect(hints.map((hint) => hint.textContent)).toEqual([
      '已解析文件',
      '已跳过文件',
      '损坏文件',
      '未知版本文件',
      '语义样本',
      '缺失累计执行耗时按 0 计',
    ]);
    expect(hints.every((hint) => hint.getAttribute('tabindex') === '0')).toBe(true);

    await act(async () => {
      hints[5].dispatchEvent(new Event('pointermove', { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(document.querySelector('[data-slot="tooltip-content"]')?.textContent).toBe('历史版本影响，部分任务缺少耗时统计。');
  });

  it('shows an immediate submitting state and coalesces duplicate clicks', async () => {
    let resolveStart: ((value: PersonalAnalyticsSnapshotVm) => void) | undefined;
    api.syncPersonalAnalytics.mockReturnValueOnce(new Promise((resolve) => { resolveStart = resolve; }));
    const container = await renderPage(registry(true));

    await act(async () => {
      await Promise.resolve();
    });
    expect(container.textContent).toContain('正在刷新');
    expect(startButton(container).disabled).toBe(true);

    expect(api.syncPersonalAnalytics).toHaveBeenCalledTimes(1);

    await act(async () => resolveStart?.(snapshot('queued', 2)));
  });

  it('renders all report sections, compact Token units, and Token columns for recent and duration-ranked tasks', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const container = await renderPage(registry(true));

    for (const title of ['使用概览', '最近任务', '终局可靠性', '质量', '效率', 'Token 消耗', '上下文与技能使用', '数据覆盖']) {
      expect(container.textContent).toContain(title);
    }
    expect(container.textContent).toContain('Workflow task');
    expect(container.textContent).not.toContain('Direct task');
    expect(container.textContent).toContain('未发现可验证的 Skill 调用记录');
    expect(container.textContent).toContain('1.2K');
    expect(container.textContent).toContain('0.8K');
    expect(container.textContent).toContain('0.4K');
    expect(container.textContent).toContain('0K');
    expect(Array.from(container.querySelectorAll('th')).filter((cell) => cell.textContent === 'Token')).toHaveLength(3);
    expect(container.querySelector('#overview')?.textContent).not.toContain('Direct 回复完成率');
    expect(container.querySelector('#reliability')?.textContent).toContain('Direct 回复完成率');
    expect(container.querySelector('#reliability')?.textContent).toContain('Workflow run 终局成功率');
    expect(container.querySelector('#reliability')?.textContent).toContain('AUTO outer run 终局成功率');
    expect(
      Array.from(container.querySelectorAll('[data-personal-analytics-report="true"] section[id]'))
        .slice(0, 3)
        .map((section) => section.id),
    ).toEqual(['overview', 'reliability', 'recent-tasks']);
    expect(
      Array.from(container.querySelectorAll('[data-personal-analytics-nav="true"] button'))
        .slice(0, 3)
        .map((button) => button.textContent),
    ).toEqual(['使用概览', '终局可靠性', '最近任务']);
  });

  it('queries SQLite reports for custom ranges without invoking the Agent', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const container = await renderPage(registry(true));
    const inputs = Array.from(container.querySelectorAll('input[type="date"]')) as HTMLInputElement[];
    expect(inputs).toHaveLength(0);
    await act(async () => {
      buttonByText(container, '自定义')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    const customInputs = Array.from(container.querySelectorAll('input[type="date"]')) as HTMLInputElement[];
    const setValue = (input: HTMLInputElement, value: string) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(input, value);
    };
    await act(async () => {
      setValue(customInputs[0], '2026-08-01');
      customInputs[0].dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {
      setValue(customInputs[1], '2026-08-18');
      customInputs[1].dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => { await Promise.resolve(); });
    expect(api.queryPersonalAnalyticsReport).toHaveBeenLastCalledWith(
      { start: '2026-08-01', end: '2026-08-18' },
      'agent-a',
      undefined,
      undefined,
      undefined,
    );
    expect(api.startPersonalAnalyticsInsights).not.toHaveBeenCalled();
    expect(container.querySelectorAll('[data-personal-analytics-nav="true"] button')).toHaveLength(8);
  });

  it('queries a changed range without rescanning history or invoking the Agent', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const container = await renderPage(registry(true));
    expect(api.syncPersonalAnalytics).toHaveBeenCalledTimes(1);

    await act(async () => { buttonByText(container, '自定义')?.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    const [start, end] = Array.from(container.querySelectorAll('input[type="date"]')) as HTMLInputElement[];
    const setValue = (input: HTMLInputElement, value: string) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(input, value);
    };
    await act(async () => {
      setValue(start, '2026-08-01');
      start.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {
      setValue(end, '2026-08-18');
      end.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => { await Promise.resolve(); });

    expect(api.syncPersonalAnalytics).toHaveBeenCalledTimes(1);
    expect(api.queryPersonalAnalyticsReport).toHaveBeenLastCalledWith(
      { start: '2026-08-01', end: '2026-08-18' },
      'agent-a',
      undefined,
      undefined,
      undefined,
    );
    expect(api.startPersonalAnalyticsInsights).not.toHaveBeenCalled();
  });

  it('does not display a previous range report when the selected range query fails', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    api.queryPersonalAnalyticsReport.mockImplementation((range: { start: string | null }) => (
      range.start
        ? Promise.reject({ code: 'analytics.report-query-failed' })
        : Promise.resolve(report())
    ));
    const container = await renderPage(registry(false));

    await act(async () => {
      buttonByText(container, '自定义')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    const [start, end] = Array.from(container.querySelectorAll('input[type="date"]')) as HTMLInputElement[];
    const setValue = (input: HTMLInputElement, value: string) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(input, value);
    };
    await act(async () => {
      setValue(start, '2026-08-01');
      start.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {
      setValue(end, '2026-08-18');
      end.dispatchEvent(new Event('change', { bubbles: true }));
      await Promise.resolve();
    });
    await act(async () => { await Promise.resolve(); });

    expect(container.textContent).not.toContain('Workflow task');
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('无法读取当前日期范围的分析报告');
  });

  it('ignores a stale range response after a newer report request', async () => {
    let resolveStale: ((value: PersonalAnalyticsReportVm) => void) | undefined;
    const staleReport = report();
    staleReport.recentTasks[0].title = 'Stale report';
    const latestReport = report([], { start: '2026-08-01', end: '2026-08-18' });
    latestReport.recentTasks[0].title = 'Latest report';
    api.getPersonalAnalytics.mockResolvedValueOnce({
      ...snapshot('completed', 6),
      latestReport: staleReport,
    });
    api.queryPersonalAnalyticsReport.mockReturnValueOnce(
      new Promise<PersonalAnalyticsReportVm>((resolve) => {
        resolveStale = resolve;
      }),
    );
    api.queryPersonalAnalyticsReport.mockReturnValueOnce(Promise.resolve(latestReport));
    const container = await renderPage(registry(false));

    await act(async () => {
      buttonByText(container, '自定义')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    const customInputs = Array.from(container.querySelectorAll('input[type="date"]')) as HTMLInputElement[];
    const setValue = (input: HTMLInputElement, value: string) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(input, value);
    };
    await act(async () => {
      setValue(customInputs[0], '2026-08-01');
      customInputs[0].dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {
      setValue(customInputs[1], '2026-08-18');
      customInputs[1].dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => { await Promise.resolve(); });
    expect(container.textContent).toContain('Latest report');

    await act(async () => {
      resolveStale?.(staleReport);
      await Promise.resolve();
    });
    expect(container.textContent).not.toContain('Stale report');
    expect(container.textContent).toContain('Latest report');
  });

  it('re-queries the selected range after an insight operation completes', async () => {
    const insight = {
      section: 'quality' as const,
      title: '优先收敛重入根因',
      summary: '重试集中出现在少数节点。',
      recommendation: '先补对应节点的失败隔离测试。',
      confidence: 'high' as const,
      sampleCount: 8,
      evidenceLocators: ['project-a/task-workflow/runs/run-1/node.json'],
    };
    let queryCount = 0;
    api.getPersonalAnalytics.mockResolvedValueOnce({
      ...snapshot('completed', 6),
      insightOperation: insightOperation('analyzing', 2),
      latestReport: report(),
    });
    api.queryPersonalAnalyticsReport.mockImplementation(() => {
      queryCount += 1;
      return Promise.resolve(report(queryCount > 1 ? [insight] : []));
    });
    let emit: ((next: PersonalAnalyticsSnapshotVm) => void) | undefined;
    api.subscribePersonalAnalyticsUpdates.mockImplementation((listener: (next: PersonalAnalyticsSnapshotVm) => void) => {
      emit = listener;
      return Promise.resolve(() => {});
    });
    const container = await renderPage(registry(true));
    await act(async () => {});
    await act(async () => {});
    const initialQueries = api.queryPersonalAnalyticsReport.mock.calls.length;

    await act(async () => {
      emit?.({
        ...snapshot('completed', 6),
        insightOperation: insightOperation('completed', 3),
        latestReport: report(),
      });
      await Promise.resolve();
    });
    await act(async () => {});
    await act(async () => {});

    expect(api.queryPersonalAnalyticsReport.mock.calls.length).toBeGreaterThan(initialQueries);
    expect(api.queryPersonalAnalyticsReport).toHaveBeenLastCalledWith(
      { start: null, end: null },
      'agent-a',
      undefined,
      undefined,
      undefined,
    );
    expect(container.textContent).toContain('优先收敛重入根因');
  });

  it('scrolls section navigation clicks and marks the visible section', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const container = await renderPage(registry(true));
    const quality = document.getElementById('quality');
    expect(quality).not.toBeNull();
    const scrollIntoView = vi.fn();
    Object.defineProperty(quality, 'scrollIntoView', { value: scrollIntoView });
    await act(async () => {
      buttonByText(container, '质量')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: 'smooth', block: 'start' });

    const efficiency = document.getElementById('efficiency');
    expect(efficiency).not.toBeNull();
    const observer = intersectionObservers.at(-1);
    expect(observer?.observed).toContain(quality);
    expect(observer?.observed).toContain(efficiency);
    await act(async () => {
      observer?.callback(
        [{ target: efficiency, isIntersecting: true, boundingClientRect: { top: 10 } } as unknown as IntersectionObserverEntry],
        observer as unknown as IntersectionObserver,
      );
    });
    const efficiencyButton = buttonByText(container, '效率');
    expect(efficiencyButton?.className.split(/\s+/)).toContain('bg-accent');
    expect(buttonByText(container, '质量')?.className.split(/\s+/)).not.toContain('bg-accent');
  });

  it('supports quick ranges and blocks invalid custom insight ranges', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    const container = await renderPage(registry(true));
    const today = new Date();
    const localToday = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
    await act(async () => {
      buttonByText(container, '今天')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(api.queryPersonalAnalyticsReport).toHaveBeenLastCalledWith(
      { start: localToday, end: localToday },
      'agent-a',
      undefined,
      undefined,
      undefined,
    );

    await act(async () => {
      buttonByText(container, '自定义')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('请选择有效的起止日期');
    expect((container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement).disabled).toBe(true);
    expect(startButton(container).disabled).toBe(false);
  });

  it('keeps deterministic reporting when an insight request fails', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    api.startPersonalAnalyticsInsights.mockRejectedValueOnce({ code: 'analytics.agent-unavailable' });
    const container = await renderPage(registry(true));
    await act(async () => {});
    const insightButton = container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement;

    await act(async () => {
      insightButton.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      await Promise.resolve();
    });
    expect(container.textContent).toContain('所选 Agent 当前不可用。');
    expect(container.textContent).toContain('Workflow task');
  });

  it('does not let a late start response overwrite a completed insight event', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({ ...snapshot('completed', 6), latestReport: report() });
    let emit: ((next: PersonalAnalyticsSnapshotVm) => void) | undefined;
    api.subscribePersonalAnalyticsUpdates.mockImplementation((listener: (next: PersonalAnalyticsSnapshotVm) => void) => {
      emit = listener;
      return Promise.resolve(() => {});
    });
    let resolveStart: ((operation: AgentInsightOperationVm) => void) | undefined;
    api.startPersonalAnalyticsInsights.mockReturnValueOnce(new Promise((resolve) => { resolveStart = resolve; }));
    const container = await renderPage(registry(true));
    await act(async () => {});

    await act(async () => {
      (container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement)
        .dispatchEvent(new MouseEvent('click', { bubbles: true }));
      await Promise.resolve();
    });
    await act(async () => {
      emit?.({ operation: null, insightOperation: insightOperation('completed', 3), latestReport: null });
      resolveStart?.(insightOperation('queued', 1));
      await Promise.resolve();
    });

    expect(buttonByText(container, '取消洞察')).toBeUndefined();
    expect((container.querySelector('[data-personal-analytics-insight="true"]') as HTMLButtonElement).disabled).toBe(false);
  });

  it('does not let a late cancel response overwrite a cancelled insight event', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({
      ...snapshot('completed', 6),
      insightOperation: insightOperation('analyzing', 2),
      latestReport: report(),
    });
    let emit: ((next: PersonalAnalyticsSnapshotVm) => void) | undefined;
    api.subscribePersonalAnalyticsUpdates.mockImplementation((listener: (next: PersonalAnalyticsSnapshotVm) => void) => {
      emit = listener;
      return Promise.resolve(() => {});
    });
    let resolveCancel: ((operation: AgentInsightOperationVm) => void) | undefined;
    api.cancelPersonalAnalyticsInsights.mockReturnValueOnce(new Promise((resolve) => { resolveCancel = resolve; }));
    const container = await renderPage(registry(true));

    await act(async () => {
      buttonByText(container, '取消洞察')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      await Promise.resolve();
    });
    await act(async () => {
      emit?.({ operation: null, insightOperation: insightOperation('cancelled', 4), latestReport: null });
      resolveCancel?.(insightOperation('cancelling', 3));
      await Promise.resolve();
    });

    expect(buttonByText(container, '取消洞察')).toBeUndefined();
  });

  it('allows an active insight to be cancelled independently', async () => {
    api.getPersonalAnalytics.mockResolvedValueOnce({
      ...snapshot('completed', 6),
      insightOperation: insightOperation('analyzing', 2),
      latestReport: report(),
    });
    const container = await renderPage(registry(true));
    const cancelButton = buttonByText(container, '取消洞察');

    await act(async () => {
      cancelButton?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(api.cancelPersonalAnalyticsInsights).toHaveBeenCalledWith('insight-operation-1');
    expect(api.cancelPersonalAnalytics).not.toHaveBeenCalled();
  });
});

async function renderPage(agentRegistry: AgentRegistryVm) {
  const container = document.createElement('div');
  container.style.height = '800px';
  document.body.appendChild(container);
  mountedRoot = createRoot(container);
  await act(async () => {
    mountedRoot?.render(
      <PersonalAnalyticsPage agentRegistry={agentRegistry} onOpenAgentManagement={vi.fn()} onOpenTask={onOpenTask} />,
    );
  });
  return container;
}

const onOpenTask = vi.fn();

function registry(available: boolean, displayName = 'Agent A'): AgentRegistryVm {
  return {
    agents: [{
      agentType: 'agent-a',
      displayName,
      command: 'agent-a',
      args: [],
      env: [],
      iconKey: 'agent',
      primaryAgentDir: '.agent-a',
      projectPrimaryAgentDir: null,
      compatibleAgentDirs: [],
      supportsSystemPrompt: true,
      externalSessionSyncSupported: false,
      externalSessionSyncEnabled: false,
      diagnostic: { status: available ? 'healthy' : 'unavailable', available, checkedAt: '2026-08-18T00:00:00Z' },
      supportedModels: available ? [
        { id: 'model-a', name: 'Model A', description: 'Fast model' },
        { id: 'model-b', name: 'Model B', description: 'Deep model' },
      ] : null,
      configOptions: available ? [{
        id: 'reasoning_effort',
        category: 'thought_level',
        name: 'Reasoning effort',
        currentValue: 'low',
        options: [
          { value: 'low', name: 'Low' },
          { value: 'high', name: 'High' },
        ],
      }] : null,
    }],
    catalog: [],
  };
}

function snapshot(
  status: PersonalAnalyticsOperationStatus | null,
  revision = 1,
): PersonalAnalyticsSnapshotVm {
  return {
    operation: status ? {
      operationId: 'operation-1',
      agentType: 'agent-a',
      status,
      revision,
      progress: { stage: status, processedUnits: 0, totalUnits: 10 },
      sourceWatermark: null,
      reportId: null,
      error: null,
      createdAt: '2026-08-18T00:00:00Z',
      updatedAt: '2026-08-18T00:00:00Z',
      completedAt: status === 'cancelled' ? '2026-08-18T00:01:00Z' : null,
    } : null,
    insightOperation: null,
    latestReport: null,
  };
}

function insightOperation(
  status: AgentInsightOperationStatus,
  revision: number,
  generation = 1,
): AgentInsightOperationVm {
  return {
    operationId: 'insight-operation-1',
    generation,
    agentType: 'agent-a',
    modelId: null,
    thoughtLevelOptionId: null,
    thoughtLevelValue: null,
    range: { start: null, end: null },
    schemaVersion: '2.2.0',
    indexRevision: 1,
    status,
    revision,
    progress: { stage: status, processedUnits: 0, totalUnits: 1 },
    sourceWatermark: '1',
    reportId: 'report-1',
    error: null,
    createdAt: '2026-08-18T00:00:00Z',
    updatedAt: '2026-08-18T00:00:00Z',
    completedAt: status === 'completed' || status === 'failed' || status === 'cancelled'
      ? '2026-08-18T00:01:00Z'
      : null,
  };
}

function report(
  insights: PersonalAnalyticsReportVm['insights'] = [],
  range: PersonalAnalyticsReportVm['range'] = { start: null, end: null },
): PersonalAnalyticsReportVm {
  const task = {
    taskLocator: 'project-a/task-workflow', projectId: 'project-a', taskId: 'task-workflow', latestRunId: 'run-1', title: 'Workflow task', mode: 'workflow', status: 'completed', outcome: 'success',
    agentNames: ['Agent A'], totalTokens: 1200, activeDurationSeconds: 60, activeDurationZeroFilled: false,
    terminalNode: 'accept', lastActivityAt: '2026-08-18T00:00:00Z',
  };
  const rate = (metricId: string) => ({ metricId, numerator: 1, denominator: 1, unknownCount: 0, rate: 1, evidenceLocators: ['project-a/task-workflow/run.json'] });
  return {
    schemaVersion: '2.2.0', reportId: 'report-1', generatedAt: '2026-08-18T00:01:00Z', sourceWatermark: 'watermark', indexRevision: 1, range,
    sourceCoverage: { discoveredFiles: 10, eligibleFiles: 8, parsedFiles: 8, skippedFiles: 2, corruptFiles: 0, unknownVersionFiles: 0, discoveredBytes: 1024, semanticEligibleItems: 1, semanticSampledItems: 1 },
    overview: { projectCount: 1, taskCount: 1, conversationCount: 1, runCount: 1, turnCount: 0, attemptCount: 1, earliestAt: '2026-08-18T00:00:00Z', latestAt: '2026-08-18T00:01:00Z' },
    recentTasks: [task],
    reliability: { directReplyCompletionRate: rate('direct.reply_completion_rate'), workflowRunTerminalSuccessRate: rate('workflow.run_terminal_success_rate'), autoOuterRunTerminalSuccessRate: rate('auto.outer_run_terminal_success_rate'), failedCount: 0, cancelledCount: 0, nonTerminalCount: 0 },
    quality: { retryReentryRate: rate('node.retry_reentry_rate'), recoveredAfterRetryCount: 0, terminalSignals: [] },
    efficiency: { observedTerminalRunActiveSeconds: 60, averageTerminalRunActiveSeconds: 60, terminalRunSampleCount: 1, activeDurationZeroFilledCount: 0, pauseCount: 0, resumeCount: 0, manualContinueCount: 0, topDurationTasks: [task], nodeAggregates: [] },
    tokenUsage: { inputTokens: 800, outputTokens: 400, cacheReadTokens: 0, cacheWriteTokens: 0, totalTokens: 1200, observedPromptCount: 1, topTokenTasks: [task] },
    contextAndTools: { toolCallCount: 0, permissionRequestCount: 0, elicitationRequestCount: 0, topTools: [], topAgents: [], verifiedSkillCallCount: 0, topSkills: [], eventKinds: [] },
    insights, warnings: [],
  };
}

function startButton(container: HTMLElement) {
  return container.querySelector('[data-personal-analytics-start="true"]') as HTMLButtonElement;
}

function buttonByText(container: HTMLElement, text: string) {
  return Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.includes(text));
}
