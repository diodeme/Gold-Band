import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const localesDir = path.join(root, 'web', 'src', 'locales');
const partsDir = path.join(__dirname, 'ko-parts');

const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
const zh = JSON.parse(fs.readFileSync(path.join(localesDir, 'zh-CN.json'), 'utf8'));
const existingKo = {};
for (const file of fs.readdirSync(partsDir)) {
  if (!file.endsWith('.json')) continue;
  existingKo[file.replace('.json', '')] = JSON.parse(fs.readFileSync(path.join(partsDir, file), 'utf8'));
}

const GLOSSARY = [
  'CI/CD', 'Gold Band', 'AI-DYNAMIC', 'ACP Registry', 'API Key', 'GitHub API', 'GitHub CLI', 'GitHub',
  'Workflow', 'worktree', 'worktrees', 'Profile', 'Profiles', 'runtime', 'AUTO', 'Direct', 'Agent', 'Agents',
  'Git', 'MCP', 'ACP', 'Cron', 'JSON', 'SKILL', 'SKILLs', 'Multica', 'Fetch', 'Pull', 'Push', 'HEAD',
  'Markdown', 'WebP', 'PNG', 'JPEG', 'SVG', 'HTML', 'HTTP', 'SSE', 'Stdio', 'UTF-8', 'Ctrl+V', 'MiniJinja',
  'Claude', 'Codex', 'Cursor', 'Gemini', 'OpenCode', 'progress.events', 'raw.stream', '.gold-band', '.claude',
  '.pi', '.agents', 'KEY=VALUE', 'Provider', 'Session', 'Desktop', 'Porcelain', 'Tech Gray', 'Graphite Dark',
  'Terminal Black', 'WeCom', 'DEBUG', 'INFO', 'Sub-agent', 'Context Window', 'Token Usage', 'Cache Read',
  'Cache Write', 'Initialize', 'Inbound', 'Outbound', 'Auto Accept', 'System prompt', 'Raw frames', 'Tool call',
  'Tool output', 'Tool parameters', 'Bootstrap', 'Fanout', 'Detached HEAD', 'Runtime checkpoint', 'Pull Requests',
  'End node', 'New Round', 'Canvas', 'Inspector', 'doctor', 'workflow.id', '$new-round', '$entry', 'merge',
  'acceptance', 'AI Dynamic Node', 'Dynamic Worker Node', 'Dynamic Workflow Node', 'Dynamic Merge Node',
  'Dynamic Acceptance Node', 'Unknown Node', 'Agent node', 'ACP session', 'ACP Session', 'ACP turn finished',
  'outer run', 'outer-run', 'Gold Band Logo', 'Gold Band Shared Memory', 'Gold Band Git', 'Gold Band Workflow Control',
  'runtime.log', 'runtime-internal', 'AI output decision', 'AI-DYNAMIC routing', 'AI Output Validation',
  'JSON Schema', 'JSON output constraint', 'Beautify JSON', 'Invalid JSON', 'Success expression', 'Permission mode',
  'Bootstrap agent', 'Dynamic agent', 'Fixed agent', 'Available dynamic agents', 'Agent routing guidance',
  'Max Dynamic Nodes', 'Max Fanout', 'Max Depth', 'Max Parallel', 'Group Depth', 'Max Workflow Invocations',
  'Allow nested AI-DYNAMIC', 'Manage Profiles', 'Node Config', 'Edge Config', 'Workflow Controls', 'Workflow Settings',
  'Default Template', 'Save Workflow', 'Workflow Editor', 'Workflow ID', 'Entry node', 'End node', 'New Round Start',
  'Edge Type', 'Session', 'Manual check', 'Output artifact key', 'Allowed Workflows', 'Available profiles',
  'Global goal', 'Selectable workflows', 'Unavailable workflows', 'Sync model configuration', 'Model Configuration',
  'Permission mode', 'Unspecified', 'Quick add successor', 'Current Selection', 'Review and fix',
  'Fast-forward only (recommended)', 'GitHub CLI', 'GitHub repository', 'GitHub browser login', 'GitHub operation',
  'GitHub CLI status detection failed', 'GitHub CLI is not installed', 'GitHub CLI is not authenticated',
  'GitHub API rate limit', 'Pull request', 'pull request', 'Pull requests', 'Issues', 'Create pull request',
  'Open on GitHub', 'Open in built-in browser', 'Source Control', 'Git status', 'Git operation failed',
  'Git file diff', 'Git workflow', 'Git version', 'Git repository', 'Git features', 'Git workspace',
  'Git downloads', 'Git reason', 'Git write', 'GitHub', 'worktree path', 'Create worktree', 'Remove worktree',
  'Runtime checkpoint', 'Commit review', 'Commit reachability', 'Commit list', 'Commit changes', 'Commit subject',
  'Commit body', 'Stash', 'Rebase', 'Merge', 'Fetch', 'Push tag', 'Push branch', 'Pull strategy', 'Detached HEAD',
  'Working tree is clean', 'Initialize repository', 'Runtime-internal branches', 'AUTO mode', 'Direct mode',
  'AUTO Template', 'AUTO outer run', 'AUTO outer-run', 'Workflow run', 'Workflow template', 'Workflow mode',
  'Workflow Control', 'Workflow Blueprint', 'Workflow Editor', 'Workflow cannot be saved', 'Workflow ID is required',
  'Edit AUTO', 'Edit Workflow', 'Configure AUTO', 'Configure Run Mode', 'Run Mode', 'Run Mode Management',
  'Requirement Interview', 'Requirement Grilling', 'Multica', 'WeCom', 'API Key', 'Metrics Reporting',
  'Verbose runtime logging', 'Use local Claude', 'IM remote intervention and notifications', 'ACP Registry',
  'ACP turn finished', 'ACP session', 'ACP events', 'ACP Session', 'ACP permission mode', 'Native ACP permission mode',
  'Direct Agent', 'Direct reply completion rate', 'Direct mode', 'Profile catalog', 'Profile help',
  'runtime controlled', 'runtime context', 'runtime abnormal', 'runtime error', 'runtime session',
  'Cron expression', 'six-field Cron expression', 'Direct mode', 'AUTO mode', 'AUTO Template', 'AUTO outer run',
  'workflow.id', 'JSON and try again', 'Update the JSON', 'legacy JSON Schema', 'simplified output shape',
  'JSON output constraint', 'Beautify JSON', 'Invalid JSON', 'Cron {{expression}}', 'Cron expression',
  'Direct 모드', 'AUTO 모드'
];

const PH_RE = /\{\{[^}]+\}\}/g;
const TAG_RE = /<\/?[^>]+>/g;

function protect(text, extra = []) {
  const placeholders = [];
  const tags = [];
  const tokens = [];
  let out = text.replace(PH_RE, (m) => {
    placeholders.push(m);
    return `__PH${placeholders.length - 1}__`;
  });
  out = out.replace(TAG_RE, (m) => {
    tags.push(m);
    return `__TG${tags.length - 1}__`;
  });
  for (const token of [...new Set([...extra, ...GLOSSARY])].sort((a, b) => b.length - a.length)) {
    if (!out.includes(token)) continue;
    tokens.push(token);
    out = out.split(token).join(`__TK${tokens.length - 1}__`);
  }
  return { out, placeholders, tags, tokens };
}

function restore(text, pack) {
  let out = text;
  for (let i = 0; i < pack.tokens.length; i += 1) out = out.split(`__TK${i}__`).join(pack.tokens[i]);
  for (let i = 0; i < pack.tags.length; i += 1) out = out.split(`__TG${i}__`).join(pack.tags[i]);
  for (let i = 0; i < pack.placeholders.length; i += 1) out = out.split(`__PH${i}__`).join(pack.placeholders[i]);
  return out;
}

function tokensFromZh(zhText) {
  if (typeof zhText !== 'string') return [];
  return GLOSSARY.filter((t) => zhText.includes(t));
}

const PHRASE = new Map([
  ['Loading', '불러오는 중'], ['Saving', '저장 중'], ['Deleting', '삭제 중'], ['Connecting', '연결 중'],
  ['Checking', '확인 중'], ['Initializing', '초기화 중'], ['Processing', '처리 중'], ['Sending', '전송 중'],
  ['Stopping', '중지 중'], ['Retry', '다시 시도'], ['Cancel', '취소'], ['Close', '닫기'], ['Back', '뒤로'],
  ['Save', '저장'], ['Delete', '삭제'], ['Edit', '편집'], ['Open', '열기'], ['Create', '만들기'], ['Remove', '제거'],
  ['Add', '추가'], ['Search', '검색'], ['Refresh', '새로고침'], ['Continue', '계속'], ['Stop', '중지'],
  ['Send', '전송'], ['Copy', '복사'], ['Copied', '복사됨'], ['Confirm', '확인'], ['Apply', '적용'],
  ['Enable', '활성화'], ['Disable', '비활성화'], ['Enabled', '활성화됨'], ['Disabled', '비활성화됨'],
  ['Failed', '실패'], ['Success', '성공'], ['Running', '실행 중'], ['Pending', '대기 중'], ['Completed', '완료됨'],
  ['Settings', '설정'], ['Workspace', '워크스페이스'], ['Task', '작업'], ['Tasks', '작업'], ['Status', '상태'],
  ['All', '전체'], ['None', '없음'], ['Model', '모델'], ['Permission', '권한'], ['Branch', '브랜치'],
  ['Commit', '커밋'], ['History', '기록'], ['Changes', '변경'], ['Repository', '저장소'], ['Remote', '원격'],
  ['Overview', '개요'], ['Description', '설명'], ['Title', '제목'], ['Name', '이름'], ['Path', '경로'],
  ['File', '파일'], ['Files', '파일'], ['Browser', '브라우저'], ['Light', '라이트'], ['Dark', '다크'],
  ['System', '시스템'], ['General', '일반'], ['Advanced', '고급'], ['Appearance', '모양'], ['Language', '언어'],
  ['Conversation', '대화'], ['Submit', '제출'], ['Next', '다음'], ['Previous', '이전'], ['Skip', '건너뛰기'],
  ['Try again', '다시 시도하세요'], ['Please try again', '다시 시도하세요'], ['Please try again later', '나중에 다시 시도하세요'],
  ['Unable to', '다음을 할 수 없습니다:'], ['Could not', '다음을 할 수 없습니다:'], ['Failed to', '다음에 실패했습니다:'],
  ['The ', ''], [' is ', '은(는) '], [' are ', '은(는) '], [' has ', '이(가) '], [' have ', '이(가) '],
  [' not ', ' 아님 '], [' and ', ' 및 '], [' or ', ' 또는 '], [' with ', ' 와(과) '], [' for ', ' 용 '],
  [' in ', ' 에서 '], [' on ', ' 에 '], [' to ', ' 로 '], [' from ', ' 에서 '], [' before ', ' 전에 '],
  [' after ', ' 후에 '], [' when ', ' 때 '], [' while ', ' 동안 '], [' this ', ' 이 '], [' that ', ' 그 '],
  [' these ', ' 이 '], [' those ', ' 그 '], [' your ', ' 사용자 '], [' you ', ' 사용자 '], [' current ', ' 현재 '],
  [' selected ', ' 선택한 '], [' available ', ' 사용 가능한 '], [' invalid ', ' 유효하지 않은 '],
  [' required ', ' 필요 '], [' missing ', ' 누락된 '], [' already ', ' 이미 '], [' still ', ' 아직 '],
  [' first ', ' 첫 '], [' new ', ' 새 '], [' no ', ' 없음 '], [' not found', ' 찾을 수 없음'],
  ['does not exist', '존재하지 않습니다'], ['does not support', '지원하지 않습니다'], ['cannot be', '할 수 없습니다'],
  ['must be', '해야 합니다'], ['need to', '해야 합니다'], ['Check ', '확인하세요: '], ['Enter ', '입력하세요: '],
  ['Select ', '선택하세요: '], ['Choose ', '선택하세요: '], ['Install ', '설치하세요: '], ['Update ', '업데이트하세요: '],
  ['Repair ', '복구하세요: '], ['Resolve ', '해결하세요: '], ['Complete ', '완료하세요: '], ['Abort ', '중단하세요: '],
  ['Wait for', '다음을 기다리세요:'], ['Follow the', '다음을 따르세요:'], ['Use the', '다음을 사용하세요:'],
]);

function translateSentence(enText, zhText) {
  const pack = protect(enText, tokensFromZh(zhText));
  let t = pack.out;
  for (const [from, to] of PHRASE.entries()) {
    if (t.includes(from)) t = t.split(from).join(to);
  }
  const templates = [
    [/Try again\.?$/, '다시 시도하세요.'],
    [/Please try again\.?$/, '다시 시도하세요.'],
    [/\.{3}$/, '…'],
  ];
  for (const [re, rep] of templates) t = t.replace(re, rep);
  if (/^[A-Za-z0-9_{}.,:;+\-/\\()\[\] "'\u201c\u201d]+$/.test(restore(t, pack))) {
    return manualFallback(enText, zhText);
  }
  return restore(t, pack);
}

const MANUAL = JSON.parse(fs.readFileSync(path.join(__dirname, 'ko-manual-overrides.json'), 'utf8'));

function manualFallback(enText, zhText) {
  if (MANUAL[enText]) return MANUAL[enText];
  return enText;
}

function walk(enNode, zhNode, pathParts = []) {
  if (typeof enNode === 'string') {
    const key = pathParts.join('.');
    for (const scope of ['', ...pathParts.slice(0, -1).map((_, i, arr) => arr.slice(0, i + 1).join('.'))].reverse()) {
      const scoped = scope ? `${scope}.${pathParts.at(-1)}` : pathParts.at(-1);
      if (MANUAL[scoped]) return MANUAL[scoped];
    }
    return translateSentence(enNode, zhNode);
  }
  const out = {};
  for (const key of Object.keys(enNode)) out[key] = walk(enNode[key], zhNode?.[key], [...pathParts, key]);
  return out;
}

const remaining = ['errors', 'workflowEditor', 'sourceControl', 'settings', 'conversation'];
for (const section of remaining) {
  if (existingKo[section]) continue;
  const translated = walk(en[section], zh[section], [section]);
  fs.writeFileSync(path.join(partsDir, `${section}.json`), `${JSON.stringify(translated, null, 2)}\n`, 'utf8');
  console.log('generated', section);
}
