import { Replayer } from 'rrweb';
import { readRecording } from './timeline';
import { REPLAY_OPTIONS, settleReplayAnimations } from './replay-options';
import 'rrweb/dist/style.css';

let player: Replayer | undefined;
let data: ReturnType<typeof readRecording>;
let size = { width: 1440, height: 880 };
const host = document.createElement('div');
document.body.append(host);
const capture = {
  async load(name: string) {
    if (!/^(zh|en)-(before|during|after|personalize)(-light)?$/.test(name)) throw new Error('Invalid asset');
    player?.destroy();
    data = readRecording(await (await fetch(`/media/${name}.json`)).json());
    player = new Replayer(data.recording.events, { ...REPLAY_OPTIONS, root: host });
    player.on('resize', value => { size = value as typeof size; });
    return data.shots.map(shot => shot.id);
  },
  async seek(id: string) {
    const started = performance.now();
    const index = data.shots.findIndex(shot => shot.id === id);
    const shot = data.shots[index];
    const end = data.shots[index + 1]?.at ?? data.duration;
    const at = Math.min(end - 1, shot.at + Math.max(shot.transition, 100));
    player!.pause(at);
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    const doc = host.querySelector('iframe')!.contentDocument!;
    settleReplayAnimations(doc);
    await doc.fonts.ready;
    await Promise.all([...doc.images].filter(image => image.getAttribute('src')).map(image => image.decode()));
    for (const overlay of doc.querySelectorAll('[role="listbox"][data-state="open"], [role="menu"][data-state="open"]')) {
      if (Number(doc.defaultView!.getComputedStyle(overlay).opacity) < 0.99) throw new Error('Open replay overlay is invisible after seeking');
    }
    if ((doc.body.textContent?.length ?? 0) < 50) throw new Error('Blank checkpoint');
    return { ...size, desktop: shot.desktop, mobile: shot.mobile, at, elapsedMs: performance.now() - started };
  },
};
Object.assign(window, { checkpointCapture: capture });
