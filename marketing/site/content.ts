export type Language = 'zh' | 'en';
export type Page = 'home' | 'documentation' | 'demo' | 'not-found';
export const CHAPTER_IDS = ['before', 'during', 'after', 'personalize'] as const;
export type ChapterId = typeof CHAPTER_IDS[number];
export const GITHUB = 'https://github.com/diodeme/Gold-Band';
export const DESKTOP_QUERY = '(min-width: 1024px)';
export const CAPTURE = { width: 1440, height: 880, minimum: 560 };

export function parseRoute(path: string, fallback: Language = 'zh'): { language: Language; page: Page } {
  const parts = path.split('/').filter(Boolean);
  const language = parts[0] === 'en' ? 'en' : parts[0] === 'zh' ? 'zh' : fallback;
  if (parts[0] === 'en' || parts[0] === 'zh') parts.shift();
  const page = parts.length === 0 ? 'home' : parts.length === 1 && (parts[0] === 'documentation' || parts[0] === 'demo') ? parts[0] : 'not-found';
  return { language, page };
}
export function pageHref(language: Language, page: Page, base = '/') {
  return `${base}${language}/${page === 'home' ? '' : page === 'not-found' ? '404' : page}`;
}
export const SCENE_DIRECTORIES = { before: 'before', during: 'workflow', after: 'after', personalize: 'personalize' } as const;
const POSTER_STEPS = { before: 'establish', during: 'result-menu', after: 'attachment', personalize: 'theme' } as const;
export function posterPath(base: string, language: Language, chapter: ChapterId, theme: 'light' | 'dark', mobile: boolean) {
  return `${base}media/${SCENE_DIRECTORIES[chapter]}/${language}-${theme}${mobile ? '-mobile' : ''}/${POSTER_STEPS[chapter]}.png`;
}
export const copy = {
  zh: {
    nav: ['首页', '文档', 'Demo'], download: '下载 Gold Band', source: '源代码',
    tagline: '你的 Agent，你的工作方式。', intro: '把 AI 对话、工作流和代码审阅，放进同一个桌面工作区。',
    kicker: '开源 · 本地优先 · ACP', story: '从一个想法，到一次交付。',
    foot: '在自己的桌面，掌握每一步。', footText: '连接你选择的 Agent，让对话与工程流程一起工作。',
    play: '播放演示', loading: '正在加载演示', retry: '重新加载', error: '演示暂时无法加载',
    interactive: '亲手试试', recording: '观看演示', preview: '交互预览', resize: '调整预览宽度',
    dark: '深色', light: '浅色', system: '跟随系统', appearance: '外观', openDemo: '打开 Demo', font: '字体', defaultFont: '默认', monoFont: '等宽', reset: '重置预览',
    placeholder: '正在准备中', docsText: '文档正在整理，当前可在 GitHub 查看安装与使用说明。', demoText: '更多示例正在准备中。首页可查看产品演示。',
    back: '返回首页', missing: '页面不存在',
    chapters: [
      { id: 'before', eyebrow: '会话前', title: '配置你的 Harness。', body: '选择 Agent，组合角色与 Skill，让 Direct、Workflow 或 AUTO 适合这次任务。', points: ['Agent、角色与 Skill', '多 Agent 管理与同步', 'Direct / Workflow / AUTO'] },
      { id: 'during', eyebrow: '会话中', title: '输出决定下一步。', body: '看清思考与工具调用，回应提问和权限请求；依据结构化输出，让工作流进入修正或继续。', points: ['输出约束与结果判定', '提问与权限处理', '修正或继续的真实路径'] },
      { id: 'after', eyebrow: '会话后', title: '审阅你的任务。', body: '打开附件与 Diff，检查工作区的最新文件，再暂存并提交确认过的修改。', points: ['附件与悬浮 Diff', '最新文件与源码审阅', '暂存和本地提交'] },
      { id: 'personalize', eyebrow: '个性化', title: '定制你的应用。', body: '选择主题、字体和头像；窗口从三栏收成单栏，再展开回你的工作区。', points: ['主题、字体与头像', '真实窗口自适应', '三栏、双栏、单栏往返'] },
    ],
  },
  en: {
    nav: ['Home', 'Documentation', 'Demo'], download: 'Download Gold Band', source: 'Source code',
    tagline: 'Your agents. Your way of working.', intro: 'AI conversations, workflows and code review, together in one desktop workspace.',
    kicker: 'Open source · Local first · ACP', story: 'From an idea to a delivery.',
    foot: 'Make every step your own.', footText: 'Connect your agent of choice. Bring conversation and engineering together.',
    play: 'Play demo', loading: 'Loading demo', retry: 'Retry', error: 'Demo could not be loaded',
    interactive: 'Try it yourself', recording: 'Watch demo', preview: 'Interactive preview', resize: 'Resize preview',
    dark: 'Dark', light: 'Light', system: 'System', appearance: 'Appearance', openDemo: 'Open Demo', font: 'Font', defaultFont: 'Default', monoFont: 'Monospace', reset: 'Reset preview',
    placeholder: 'Coming soon', docsText: 'Documentation is being prepared. Installation and usage instructions are available on GitHub.', demoText: 'More examples are on the way. Explore the product on the home page.',
    back: 'Back to home', missing: 'Page not found',
    chapters: [
      { id: 'before', eyebrow: 'Before the session', title: 'Configure your harness.', body: 'Choose an agent, combine roles and skills, and use Direct, Workflow or AUTO for the task.', points: ['Agents, roles and skills', 'Multi-agent management and sync', 'Direct / Workflow / AUTO'] },
      { id: 'during', eyebrow: 'During the session', title: 'Let the output decide what comes next.', body: 'Follow reasoning and tools, respond to questions and permission requests, and route the workflow from structured output.', points: ['Output constraints and validation', 'Questions and permission requests', 'Routes to repair or continue'] },
      { id: 'after', eyebrow: 'After the session', title: 'Review your task.', body: 'Open attachments and diffs, inspect the latest workspace files, then stage and commit the changes you have reviewed.', points: ['Attachments and hover diffs', 'Latest files and source review', 'Staging and local commits'] },
      { id: 'personalize', eyebrow: 'Make it yours', title: 'Make the app yours.', body: 'Choose a theme, fonts and avatars. Narrow three columns to one, then widen the window to restore your workspace.', points: ['Themes, fonts and avatars', 'Responsive workspace', 'Three, two, one and back'] },
    ],
  },
} as const;
