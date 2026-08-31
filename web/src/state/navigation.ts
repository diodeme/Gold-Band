import type { TaskPage } from '../types';

export interface BreadcrumbItemVm {
  key: string;
  label?: string;
  labelKey?: string;
  page?: TaskPage;
}

export function breadcrumbs(page: TaskPage) {
  const items: BreadcrumbItemVm[] = [{ key: 'task-list', labelKey: 'navigation.taskList', page: { kind: 'task-list' } }];
  if (page.kind === 'task-list') return items;
  items.push({ key: `task-${page.taskId}`, label: page.taskId });
  items.push({ key: 'workflow', labelKey: 'navigation.workflow', page: { kind: 'workflow', taskId: page.taskId } });
  return items;
}
