// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { AcpActivityImageStrip } from '../src/components/acp/AcpImageStrip';
import { getAcpActivityImages } from '../src/api';
import type { TurnFileLocatorVm } from '../src/types';

vi.mock('../src/api', () => ({ getAcpActivityImages: vi.fn() }));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
let visibility: (entries: { isIntersecting: boolean }[]) => void;
vi.stubGlobal('IntersectionObserver', class {
  constructor(callback: typeof visibility) { visibility = callback; }
  observe() {} disconnect() {}
});
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); vi.clearAllMocks(); });
const locator = { projectId: 'project', taskId: 'task', runId: 'run', roundId: 'round',
  nodeId: 'node', attemptId: 'attempt', branchId: 'main' } as TurnFileLocatorVm;

it('loads only when visible and keeps completed results across viewport reentry', async () => {
  vi.mocked(getAcpActivityImages).mockResolvedValue({ images: [], nextCursor: null, generation: 1 });
  const root = createRoot(document.createElement('div'));
  dispose = () => act(() => root.unmount());
  await act(async () => root.render(<AcpActivityImageStrip locator={locator} start={1} end={40} generation={1} />));
  expect(getAcpActivityImages).not.toHaveBeenCalled();
  await act(async () => visibility([{ isIntersecting: true }]));
  expect(getAcpActivityImages).toHaveBeenCalledTimes(1);
  await act(async () => visibility([{ isIntersecting: false }]));
  await act(async () => visibility([{ isIntersecting: true }]));
  expect(getAcpActivityImages).toHaveBeenCalledTimes(1);
});

it('stops a nonadvancing cursor with a local retry instead of looping', async () => {
  vi.mocked(getAcpActivityImages)
    .mockResolvedValueOnce({ images: [], nextCursor: 'tool', generation: 1 })
    .mockResolvedValueOnce({ images: [], nextCursor: 'tool', generation: 1 })
    .mockRejectedValue(new Error('unexpected extra request'));
  const host = document.createElement('div');
  const root = createRoot(host);
  dispose = () => act(() => root.unmount());
  await act(async () => root.render(<AcpActivityImageStrip locator={locator} start={1} end={40} />));
  await act(async () => visibility([{ isIntersecting: true }]));
  expect(getAcpActivityImages).toHaveBeenCalledTimes(2);
  expect(host.textContent).toContain('common.retry');
});

it('resumes the last accepted cursor after leaving the viewport and rejects a late page', async () => {
  let finish!: (page: { images: []; nextCursor: string; generation: number }) => void;
  vi.mocked(getAcpActivityImages)
    .mockResolvedValueOnce({ images: [], nextCursor: 'tool-32', generation: 1 })
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValue({ images: [], nextCursor: null, generation: 1 });
  const root = createRoot(document.createElement('div'));
  dispose = () => act(() => root.unmount());
  await act(async () => root.render(<AcpActivityImageStrip locator={locator} start={1} end={90} generation={1} />));
  await act(async () => visibility([{ isIntersecting: true }]));
  expect(getAcpActivityImages).toHaveBeenCalledTimes(2);
  await act(async () => visibility([{ isIntersecting: false }]));
  await act(async () => finish({ images: [], nextCursor: 'tool-64', generation: 1 }));
  expect(getAcpActivityImages).toHaveBeenCalledTimes(2);
  await act(async () => visibility([{ isIntersecting: true }]));
  expect(getAcpActivityImages).toHaveBeenLastCalledWith(expect.objectContaining({ after: 'tool-32', generation: 1 }));
  expect(getAcpActivityImages).toHaveBeenCalledTimes(3);
});
