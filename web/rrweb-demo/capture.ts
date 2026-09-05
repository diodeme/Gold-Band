import { record } from 'rrweb';
import { createRecordingBuffer, RECORDING_LIMITS, type StopReason } from './recording';
import { browserPreviewState } from '../src/api/browserState';

const options = new URLSearchParams(location.search);
const preferences = browserPreviewState.getPreferences();
browserPreviewState.setPreferences({
  ...preferences,
  language: options.get('language') === 'en' ? 'en' : 'zh-cn',
  appearance: { ...preferences.appearance, colorScheme: options.get('theme') === 'dark' ? 'dark' : 'light' },
});
// This entry boots the real application with its existing browser preview API.
history.replaceState(null, '', '/chat/projects/default/tasks/mock-task/runs/run-052');
void import('../src/webview-bootstrap');

let buffer = createRecordingBuffer();
let dispose: (() => void) | undefined;
let timeout: ReturnType<typeof setTimeout> | undefined;
function stop(reason: StopReason = 'manual') {
  if (dispose && reason === 'manual') record.addCustomEvent('recording-end', {});
  dispose?.();
  dispose = undefined;
  clearTimeout(timeout);
  return buffer.stop(reason);
}

window.goldBandCapture = {
  start() {
    if (dispose) return;
    buffer = createRecordingBuffer();
    dispose = record({
      emit(event) {
        const reason = buffer.append(event);
        if (reason) queueMicrotask(() => stop(reason));
      },
      inlineStylesheet: true,
      inlineImages: true,
      maskAllInputs: false,
      maskInputOptions: { password: true },
      recordCanvas: false,
      collectFonts: true,
      sampling: { mousemove: 80, scroll: 100 },
    });
    if (!dispose) throw new Error('Recorder did not initialize');
    timeout = setTimeout(() => stop('duration'), RECORDING_LIMITS.durationMs);
  },
  stop,
  status: () => ({ recording: Boolean(dispose), count: buffer.count }),
  mark: (name) => { if (dispose) record.addCustomEvent('chapter', { name }); },
};
window.addEventListener('pagehide', () => stop(), { once: true });
