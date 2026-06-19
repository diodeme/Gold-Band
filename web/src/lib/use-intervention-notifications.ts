import { useEffect, useRef } from 'react';
import { subscribeInterventionNavigate } from '../api';
import type { InterventionNavigateEventVm } from '../types';

export type InterventionNavigateHandler = (event: InterventionNavigateEventVm) => void;

/**
 * 干预弹窗导航稳定订阅 hook。
 *
 * 仅订阅 OS Toast「查看详情」点击后后端 emit 的 `gold-band://intervention-navigate`
 * 事件，触发 deep-link 导航。去重清理已由后端 `handle_toast_action` 完成，前端无需再清。
 *
 * - onNavigate 走 ref，useEffect([]) 永不重订阅（消除重订阅与事件丢失）。
 * - 应用内不再保留右上角弹窗；系统级 Toast 是唯一提醒表面。
 */
export function useInterventionNotifications(onNavigate?: InterventionNavigateHandler): void {
  const navigateRef = useRef<InterventionNavigateHandler | undefined>(onNavigate);
  useEffect(() => {
    navigateRef.current = onNavigate;
  });

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    (async () => {
      const off = await subscribeInterventionNavigate((event) => {
        if (!active) return;
        navigateRef.current?.(event);
      });
      if (active) {
        unlisten = off;
      } else {
        off();
      }
    })();

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
}
