import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const partsDir = path.join(__dirname, 'ko-parts');

const ZH = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'web', 'src', 'locales', 'zh-CN.json'), 'utf8'));
const EN = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'web', 'src', 'locales', 'en.json'), 'utf8'));

const OVERRIDES = JSON.parse(fs.readFileSync(path.join(__dirname, 'zh-ko-overrides.json'), 'utf8'));

const REPLACERS = [
  [/请重试。?/g, '다시 시도하세요.'],
  [/请稍后再试。?/g, '나중에 다시 시도하세요.'],
  [/请刷新后重试。?/g, '새로고침한 뒤 다시 시도하세요.'],
  [/请检查/g, '확인하세요'],
  [/请选择/g, '선택하세요'],
  [/请输入/g, '입력하세요'],
  [/无法/g, '할 수 없'],
  [/不能/g, '할 수 없'],
  [/尚未/g, '아직 '],
  [/已经/g, '이미 '],
  [/当前/g, '현재 '],
  [/所选/g, '선택한 '],
  [/不存在/g, '존재하지 않'],
  [/失败/g, '실패'],
  [/成功/g, '성공'],
  [/暂不可用/g, '일시적으로 사용할 수 없'],
  [/暂不支持/g, '아직 지원하지 않'],
  [/操作失败/g, '작업 실패'],
  [/加载/g, '불러오'],
  [/保存/g, '저장'],
  [/删除/g, '삭제'],
  [/创建/g, '만들'],
  [/更新/g, '업데이트'],
  [/刷新/g, '새로고침'],
  [/仓库/g, '저장소'],
  [/工作区/g, '워크스페이스'],
  [/工作空间/g, '워크스페이스'],
  [/分支/g, '브랜치'],
  [/提交/g, '커밋'],
  [/冲突/g, '충돌'],
  [/远程/g, '원격'],
  [/凭据/g, '자격 증명'],
  [/网络/g, '네트워크'],
  [/权限/g, '권한'],
  [/文件/g, '파일'],
  [/文件夹/g, '폴더'],
  [/设置/g, '설정'],
  [/对话/g, '대화'],
  [/会话/g, 'Session'],
  [/任务/g, '작업'],
  [/工作流/g, 'Workflow'],
  [/节点/g, '노드'],
  [/消息/g, '메시지'],
  [/输入/g, '입력'],
  [/发送/g, '전송'],
  [/停止/g, '중지'],
  [/继续/g, '계속'],
  [/完成/g, '완료'],
  [/开始/g, '시작'],
  [/结束/g, '종료'],
  [/打开/g, '열'],
  [/关闭/g, '닫'],
  [/安装/g, '설치'],
  [/初始化/g, '초기화'],
  [/识别/g, '식별'],
  [/修复/g, '복구'],
  [/重新/g, '다시 '],
  [/后/g, ' 후 '],
  [/的/g, ' '],
  [/了/g, ''],
  [/。/g, '.'],
  [/，/g, ', '],
  [/：/g, ': '],
  [/（/g, ' ('],
  [/）/g, ')'],
  [/？/g, '?'],
  [/！/g, '!'],
];

function zhToKo(zh, en) {
  if (OVERRIDES[zh]) return OVERRIDES[zh];
  if (OVERRIDES[en]) return OVERRIDES[en];
  let out = zh;
  for (const [re, rep] of REPLACERS) out = out.replace(re, rep);
  if (/[\u4e00-\u9fff]/.test(out)) return manualFromEn(en);
  return out.replace(/\s+/g, ' ').trim();
}

function manualFromEn(en) {
  return en
    .replace(/Try again\.?/g, '다시 시도하세요.')
    .replace(/Please try again\.?/g, '다시 시도하세요.')
    .replace(/Unable to /g, '')
    .replace(/Could not /g, '')
    .replace(/Failed to /g, '')
    .replace(/The /g, '')
    .replace(/Enter /g, '')
    .replace(/Select /g, '')
    .replace(/Choose /g, '')
    .replace(/Check /g, '')
    + ' (번역 필요)';
}

function convertTree(enNode, zhNode) {
  if (typeof enNode === 'string') return zhToKo(zhNode, enNode);
  const out = {};
  for (const key of Object.keys(enNode)) out[key] = convertTree(enNode[key], zhNode[key]);
  return out;
}

for (const section of ['errors', 'sourceControl', 'settings', 'conversation']) {
  const ko = convertTree(EN[section], ZH[section]);
  fs.writeFileSync(path.join(partsDir, `${section}.json`), `${JSON.stringify(ko, null, 2)}\n`, 'utf8');
  console.log('converted', section);
}
