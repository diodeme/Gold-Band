import i18n from '@/i18n';

i18n.addResourceBundle('en', 'translation', { demo: { historyLoadFailed: 'Unable to load recorded history.' } }, true, true);
i18n.addResourceBundle('zh-CN', 'translation', { demo: { historyLoadFailed: '无法加载历史记录。' } }, true, true);

// The recorded dataset keeps tool call titles but not the per-call payloads, so the activity
// list inside a call group is intentionally empty. Say so instead of showing a blank list.
i18n.addResourceBundle('en', 'translation', { acp: { activityDetailUnavailable: 'The demo shows tool call titles only, not their detail.' } }, true, true);
i18n.addResourceBundle('zh-CN', 'translation', { acp: { activityDetailUnavailable: 'Demo 不展示具体工具调用明细。' } }, true, true);
