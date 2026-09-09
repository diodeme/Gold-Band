export type Language = 'zh' | 'en';
export type Page = 'home' | 'documentation' | 'demo' | 'not-found';
export const CHAPTER_IDS = ['before', 'during', 'after', 'personalize'] as const;
export type ChapterId = typeof CHAPTER_IDS[number];
export const GITHUB = 'https://github.com/diodeme/Gold-Band';
export const DESKTOP_QUERY = '(min-width: 1024px)';
export const CAPTURE = { width: 1440, height: 880, minimum: 560 };

export function parseRoute(path: string): { language: Language; page: Page } {
  const parts = path.split('/').filter(Boolean);
  const language = parts[0] === 'en' ? 'en' : 'zh';
  if (parts[0] === 'en' || parts[0] === 'zh') parts.shift();
  const page = parts.length === 0 ? 'home' : parts.length === 1 && (parts[0] === 'documentation' || parts[0] === 'demo') ? parts[0] : 'not-found';
  return { language, page };
}
export function pageHref(language: Language, page: Page) {
  return `/${language}/${page === 'home' ? '' : page === 'not-found' ? '404' : page}`;
}
export function mediaPath(language: Language, chapter: ChapterId, kind: 'json' | 'png') {
  return new URL(`./media/${language}-${chapter}.${kind}`, import.meta.url).href;
}
export const copy = {
  zh: {
    nav: ['首页', '文档', 'Demo'], download: '下载 Gold Band', source: '源代码',
    tagline: '你的 Agent，你的工作方式。', intro: '把 AI 对话、工作流和代码审阅，放进同一个桌面工作区。',
    kicker: '开源 · 本地优先 · ACP', story: '从一个想法，到一次交付。',
    foot: '在自己的桌面，掌握每一步。', footText: '连接你选择的 Agent，让对话与工程流程一起工作。',
    play: '播放演示', loading: '正在加载演示', retry: '重新加载', error: '演示暂时无法加载',
    interactive: '亲手试试', recording: '观看演示', preview: '交互预览', resize: '调整预览宽度',
    dark: '深色', light: '浅色', font: '字体', defaultFont: '默认', monoFont: '等宽', reset: '重置预览',
    placeholder: '正在准备中', docsText: '文档正在整理，当前可在 GitHub 查看安装与使用说明。', demoText: '更多示例正在准备中。首页可查看产品演示。',
    back: '返回首页', missing: '页面不存在',
    chapters: [
      { id: 'before', eyebrow: '会话前', title: '准备好，再开始。', body: '选择 Agent，带上角色与技能。直接对话、运行工作流，或交给 Auto 编排，按任务选择合适的起点。', points: ['Direct / Workflow / Auto', '主工作区或独立 Worktree', '立即开始，或安排定时任务'] },
      { id: 'during', eyebrow: '会话中', title: '看得见进展，接得住变化。', body: '对话、思考与工具调用在同一条时间线上。需要调整方向时，停下流程继续交流，准备好后再继续工作流。', points: ['会话与工具调用', '上下文用量与运行状态', '人工介入与流程继续'] },
      { id: 'after', eyebrow: '会话后', title: '结果，不止一句“完成”。', body: '回看每轮文件变化，打开文档，逐项审阅 Diff。在右侧工作区检查产出，再决定如何提交。', points: ['每轮文件变更快照', 'Markdown 与源码预览', 'Git Diff 与源代码管理'] },
      { id: 'personalize', eyebrow: '个性化', title: '让工作区适合你。', body: '调整主题与字体，为自己的工作方式留出空间。窗口缩小时，三栏自然收成双栏、单栏；拉宽后，工作区回到原位。', points: ['主题与字体', '可拖动的工作区', '三栏、双栏、单栏自适应'] },
    ],
  },
  en: {
    nav: ['Home', 'Documentation', 'Demo'], download: 'Download Gold Band', source: 'Source code',
    tagline: 'Your agents. Your way of working.', intro: 'AI conversations, workflows and code review, together in one desktop workspace.',
    kicker: 'Open source · Local first · ACP', story: 'From an idea to a delivery.',
    foot: 'Make every step your own.', footText: 'Connect your agent of choice. Bring conversation and engineering together.',
    play: 'Play demo', loading: 'Loading demo', retry: 'Retry', error: 'Demo could not be loaded',
    interactive: 'Try it yourself', recording: 'Watch demo', preview: 'Interactive preview', resize: 'Resize preview',
    dark: 'Dark', light: 'Light', font: 'Font', defaultFont: 'Default', monoFont: 'Monospace', reset: 'Reset preview',
    placeholder: 'Coming soon', docsText: 'Documentation is being prepared. Installation and usage instructions are available on GitHub.', demoText: 'More examples are on the way. Explore the product on the home page.',
    back: 'Back to home', missing: 'Page not found',
    chapters: [
      { id: 'before', eyebrow: 'Before the session', title: 'Start with the right setup.', body: 'Choose an agent, bring your roles and skills, then pick your starting point: a direct conversation, a workflow, or Auto orchestration.', points: ['Direct / Workflow / Auto', 'Main workspace or isolated worktree', 'Start now or schedule a task'] },
      { id: 'during', eyebrow: 'During the session', title: 'Stay with the work.', body: 'Follow conversations, reasoning and tool calls in one timeline. Pause the flow to discuss a new direction, then continue the workflow when you are ready.', points: ['Conversations and tool calls', 'Context usage and execution status', 'Human intervention and workflow continuation'] },
      { id: 'after', eyebrow: 'After the session', title: 'Inspect what changed.', body: 'Revisit each turn’s file changes, open the documents and review the diff. Check the result in the workspace before deciding what to commit.', points: ['Per-turn file snapshots', 'Markdown and source previews', 'Git diffs and source control'] },
      { id: 'personalize', eyebrow: 'Make it yours', title: 'Room for your way of working.', body: 'Choose your theme and fonts. As the window narrows, three columns become two, then one. Widen it again and your workspace returns.', points: ['Themes and typography', 'Resizable workspace', 'Three, two or one column'] },
    ],
  },
} as const;
