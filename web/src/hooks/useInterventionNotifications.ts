import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { InterventionNotificationVm } from '@/types';

interface QueuedNotification {
  notification: InterventionNotificationVm;
  receivedAt: number;
}

interface ResolvedEvent {
  runId?: string;
  nodeId?: string;
  attemptId?: string;
}

interface NavigateEvent {
  taskId: string;
  runId: string;
  roundId: string;
}

export type InterventionNavigateHandler = (taskId: string, runId: string, roundId: string) => void;

/**
 * 监听工作流干预事件，管理前端通知队列。
 *
 * - OS 通知由 Rust 端直接发送，前端不调用 sendNotification()
 * - 此 hook 维护通知队列，供 UI 展示待处理干预列表使用
 * - 去重：同一 dedup_key 只保留最新一条
 * - 清除：收到 intervention-resolved 事件时移除对应通知
 * - 超时：1 分钟后自动移除
 * - 导航：收到 intervention-navigate 事件时调用 onNavigate 回调
 */
export function useInterventionNotifications(onNavigate?: InterventionNavigateHandler) {
  const [queue, setQueue] = useState<QueuedNotification[]>([]);

  useEffect(() => {
    let unlistenRequired: (() => void) | undefined;
    let unlistenResolved: (() => void) | undefined;
    let unlistenNavigate: (() => void) | undefined;
    let active = true;

    const setup = async () => {
      unlistenRequired = await listen<InterventionNotificationVm>(
        'gold-band://intervention-required',
        (event) => {
          if (!active) return;
          const n = event.payload;
          setQueue((prev) => {
            // 去重：同一 dedup_key 只保留最新一条
            const filtered = prev.filter(
              (q) => q.notification.dedup_key !== n.dedup_key,
            );
            return [...filtered, { notification: n, receivedAt: Date.now() }];
          });
        },
      );

      unlistenResolved = await listen<ResolvedEvent>(
        'gold-band://intervention-resolved',
        (event) => {
          if (!active) return;
          const { runId, nodeId, attemptId } = event.payload;
          setQueue((prev) =>
            prev.filter((q) => {
              const n = q.notification;
              // 精确匹配：nodeId + attemptId 同时提供时精确清除
              if (nodeId && attemptId) {
                return !(
                  n.run_id === runId &&
                  n.node_id === nodeId &&
                  n.attempt_id === attemptId
                );
              }
              // 否则按 runId 清除该 run 下所有通知
              return n.run_id !== runId;
            }),
          );
        },
      );

      // 监听 OS 通知"查看详情"按钮触发的导航事件
      unlistenNavigate = await listen<NavigateEvent>(
        'gold-band://intervention-navigate',
        (event) => {
          if (!active) return;
          const { taskId, runId, roundId } = event.payload;
          onNavigate?.(taskId, runId, roundId);
        },
      );
    };

    setup();

    return () => {
      active = false;
      unlistenRequired?.();
      unlistenResolved?.();
      unlistenNavigate?.();
    };
  }, [onNavigate]);

  // 1 分钟后自动移除过期通知
  useEffect(() => {
    const timer = setInterval(() => {
      const cutoff = Date.now() - 60_000;
      setQueue((prev) => prev.filter((q) => q.receivedAt > cutoff));
    }, 10_000);
    return () => clearInterval(timer);
  }, []);

  const dismissNotification = (dedupKey: string) => {
    setQueue((prev) =>
      prev.filter((q) => q.notification.dedup_key !== dedupKey),
    );
  };

  return { queue, dismissNotification };
}
