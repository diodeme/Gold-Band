import type { TFunction } from 'i18next';

/**
 * 远程任务来源注册表（静态声明，远程来源解耦设计 §4.4/§4.5）。
 *
 * 来源选项在此集中声明（value = 配置指针 `desktop_remote_task_source` 的取值域），
 * 需求管理页的常驻来源选择器与 composer 绑定 chip 的来源标签都从这里取——
 * 通用层不散写 `if source === 'multica'` 式特判（不变量 2：按名片渲染，不按来源特判）。
 * 新增来源 = 向此表加一项 + 后端新增适配实现与指针换绑。
 */
export const REMOTE_TASK_SOURCES = [
  { value: 'multica', labelKey: 'remote.taskManagement.source.multica' },
] as const;

export type RemoteTaskSource = (typeof REMOTE_TASK_SOURCES)[number]['value'];

/// 注册表成员判定（类型守卫）：服务端指针值是否属于选择器闭集。
export function isRemoteTaskSource(value: string): value is RemoteTaskSource {
  return REMOTE_TASK_SOURCES.some((s) => s.value === value);
}

/// 来源标识 → 展示名（注册表未收录的值原样显示，兜底不空白）。
export function remoteTaskSourceLabel(t: TFunction, source: string): string {
  const found = REMOTE_TASK_SOURCES.find((s) => s.value === source);
  return found ? t(found.labelKey) : source;
}
