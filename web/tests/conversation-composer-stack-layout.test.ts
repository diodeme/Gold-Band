import { describe, expect, it } from 'vitest';

import {
  acpComposerStackLayout,
  acpComposerStackShapeClassName,
} from '@/lib/conversation-composer-layout';

describe('ACP composer stack layout', () => {
  it('puts the info tab on the running Agents strip and joins it to the input without a divider', () => {
    const layout = acpComposerStackLayout({ agents: true, composer: true }, true);

    expect(layout.agents).toEqual({ above: 'none', below: 'seamless', infoTab: true, gapBelow: false });
    expect(layout.composer).toEqual({ above: 'seamless', below: 'none', infoTab: false, gapBelow: false });
    expect(acpComposerStackShapeClassName(layout.agents)).toContain('rounded-tl-none');
    expect(acpComposerStackShapeClassName(layout.agents)).toContain('border-b-0');
    expect(acpComposerStackShapeClassName(layout.composer)).toContain('border-t-0');
    expect(acpComposerStackShapeClassName(layout.composer)).toContain('rounded-t-none');
  });

  it('keeps divided joins between todo, queue and the input', () => {
    const layout = acpComposerStackLayout({ agents: true, todo: true, queue: true, composer: true }, true);

    expect(layout.agents?.below).toBe('seamless');
    expect(layout.todo).toEqual({ above: 'seamless', below: 'divided', infoTab: false, gapBelow: false });
    expect(layout.queue).toEqual({ above: 'divided', below: 'divided', infoTab: false, gapBelow: false });
    expect(layout.composer?.above).toBe('divided');
    expect(acpComposerStackShapeClassName(layout.queue)).not.toContain('border-');
  });

  it('moves the info tab to the first visible layer', () => {
    expect(acpComposerStackLayout({ todo: true, composer: true }, true).todo?.infoTab).toBe(true);
    expect(acpComposerStackLayout({ queue: true, composer: true }, true).queue?.infoTab).toBe(true);
    expect(acpComposerStackLayout({ composer: true }, true).composer?.infoTab).toBe(true);
    expect(acpComposerStackLayout({ composer: true }, false).composer?.infoTab).toBe(false);
  });

  it('keeps the manual check detached with a gap instead of joining neighbours', () => {
    const layout = acpComposerStackLayout({ agents: true, manual: true, todo: true, composer: true }, true);

    expect(layout.agents).toEqual({ above: 'none', below: 'none', infoTab: true, gapBelow: true });
    expect(layout.manual).toEqual({ above: 'none', below: 'none', infoTab: false, gapBelow: true });
    expect(layout.todo?.above).toBe('none');
    expect(acpComposerStackShapeClassName(layout.manual)).toContain('mb-3');
  });

  it('closes the bottom of the last layer when a read-only view has no input', () => {
    const layout = acpComposerStackLayout({ agents: true, todo: true }, false);

    expect(layout.todo).toEqual({ above: 'seamless', below: 'none', infoTab: false, gapBelow: false });
    expect(acpComposerStackShapeClassName(layout.todo)).toContain('rounded-b-2xl');
    expect(acpComposerStackShapeClassName()).toBe('rounded-t-2xl rounded-b-2xl');
  });
});
