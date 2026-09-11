// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { connectDemoHost } from '../../marketing/demo/embed';

afterEach(() => { vi.restoreAllMocks(); history.replaceState(null, '', '/'); });

it('does not subscribe or publish in a standalone window', () => {
  const add = vi.spyOn(window, 'addEventListener');
  connectDemoHost()();
  expect(add).not.toHaveBeenCalled();
});

it('publishes hash changes to the parent and releases its listeners', () => {
  const parent = { postMessage: vi.fn() };
  vi.spyOn(window, 'parent', 'get').mockReturnValue(parent as unknown as Window);
  const stop = connectDemoHost();
  expect(parent.postMessage).toHaveBeenCalledWith({ type: 'gold-band.demo.location', hash: '' }, location.origin);
  history.replaceState(null, '', '/#contexts');
  window.dispatchEvent(new HashChangeEvent('hashchange'));
  expect(parent.postMessage).toHaveBeenLastCalledWith({ type: 'gold-band.demo.location', hash: '#contexts' }, location.origin);
  stop();
  parent.postMessage.mockClear();
  window.dispatchEvent(new HashChangeEvent('hashchange'));
  expect(parent.postMessage).not.toHaveBeenCalled();
});

it('applies only host appearance changes that come from the embedding parent', () => {
  const parent = { postMessage: vi.fn() };
  vi.spyOn(window, 'parent', 'get').mockReturnValue(parent as unknown as Window);
  const applied: string[] = [];
  const stop = connectDemoHost(appearance => applied.push(appearance));
  const send = (source: unknown, origin: string, data: unknown) => window.dispatchEvent(
    Object.assign(new Event('message'), { source, origin, data }));
  send(window, location.origin, { type: 'gold-band.demo.appearance', appearance: 'light' });
  expect(applied).toEqual([]);
  send(parent, 'https://attacker.invalid', { type: 'gold-band.demo.appearance', appearance: 'light' });
  send(parent, location.origin, { type: 'gold-band.demo.appearance', appearance: 'sepia' });
  expect(applied).toEqual([]);
  send(parent, location.origin, { type: 'gold-band.demo.appearance', appearance: 'light' });
  expect(applied).toEqual(['light']);
  stop();
  send(parent, location.origin, { type: 'gold-band.demo.appearance', appearance: 'dark' });
  expect(applied).toEqual(['light']);
});
