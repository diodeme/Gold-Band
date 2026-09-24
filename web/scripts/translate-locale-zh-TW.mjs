import fs from 'node:fs';
import path from 'node:path';
import { Converter } from 'opencc-js';

const ROOT = path.resolve('web/src/locales');
const en = JSON.parse(fs.readFileSync(path.join(ROOT, 'en.json'), 'utf8'));
const zhCN = JSON.parse(fs.readFileSync(path.join(ROOT, 'zh-CN.json'), 'utf8'));
const opencc = Converter({ from: 'cn', to: 'tw' });

const TW_TERMS = [
  ['當前', '目前'],
  ['当前', '目前'],
  ['丟棄', '捨棄'],
  ['丢弃', '捨棄'],
  ['演示', '示範'],
  ['界面', '介面'],
  ['訪問', '存取'],
  ['访问', '存取'],
  ['創建', '建立'],
  ['创建', '建立'],
  ['合並', '合併'],
  ['合并', '合併'],
  ['聲明', '宣告'],
  ['声明', '宣告'],
  ['進程', '程式'],
  ['进程', '程式'],
  ['程序', '程式'],
  ['內置', '內建'],
  ['内置', '內建'],
  ['後臺', '背景'],
  ['后台', '背景'],
  ['工作臺', '工作台'],
  ['平臺', '平台'],
  ['響應', '回應'],
  ['响应', '回應'],
  ['項目記憶', '專案記憶'],
  ['項目记忆', '專案記憶'],
  ['项目记忆', '專案記憶'],
  ['項目', '專案'],
  ['项目', '專案'],
  ['應用程序', '應用程式'],
  ['应用程序', '應用程式'],
  ['軟件', '軟體'],
  ['软件', '軟體'],
  ['質量', '品質'],
  ['质量', '品質'],
  ['設置', '設定'],
  ['设置', '設定'],
  ['默認', '預設'],
  ['默认', '預設'],
  ['信息', '資訊'],
  ['资讯', '資訊'],
  ['資訊息', '資訊'],
  ['網絡', '網路'],
  ['网络', '網路'],
  ['文件夾', '資料夾'],
  ['文件夹', '資料夾'],
  ['加載', '載入'],
  ['加载', '載入'],
  ['搜索', '搜尋'],
  ['磁盘', '磁碟'],
  ['磁盤', '磁碟'],
  ['內存', '記憶體'],
  ['内存', '記憶體'],
  ['視頻', '影片'],
  ['视频', '影片'],
  ['鼠標', '滑鼠'],
  ['鼠标', '滑鼠'],
  ['服務器', '伺服器'],
  ['服务器', '伺服器'],
  ['用戶', '使用者'],
  ['用户', '使用者'],
  ['賬號', '帳號'],
  ['账号', '帳號'],
  ['登錄', '登入'],
  ['登录', '登入'],
  ['導出', '匯出'],
  ['导出', '匯出'],
  ['導入', '匯入'],
  ['导入', '匯入'],
  ['屏幕', '螢幕'],
  ['窗口', '視窗'],
  ['菜單', '選單'],
  ['菜单', '選單'],
  ['複制', '複製'],
  ['复制', '複製'],
  ['粘貼', '貼上'],
  ['粘贴', '貼上'],
  ['緩存', '快取'],
  ['缓存', '快取'],
  ['支持', '支援'],
  ['禁用', '停用'],
  ['啟用', '啟用'],
  ['启用', '啟用'],
  ['配置', '設定'],
  ['數據庫', '資料庫'],
  ['数据库', '資料庫'],
  ['數據', '資料'],
  ['数据', '資料'],
  ['鏈接', '連結'],
  ['链接', '連結'],
  ['打印', '列印'],
  ['優化', '最佳化'],
  ['优化', '最佳化'],
  ['實現', '實作'],
  ['实现', '實作'],
  ['調試', '偵錯'],
  ['调试', '偵錯'],
  ['變量', '變數'],
  ['变量', '變數'],
  ['函數', '函式'],
  ['函数', '函式'],
  ['模塊', '模組'],
  ['模块', '模組'],
  ['組件', '元件'],
  ['组件', '元件'],
  ['接口', '介面'],
  ['模板', '範本'],
  ['刷新', '重新整理'],
  ['保存', '儲存'],
  ['打開', '開啟'],
  ['打开', '開啟'],
  ['恢復', '還原'],
  ['恢复', '還原'],
  ['重置', '重設'],
  ['全選', '全選'],
  ['全选', '全選'],
  ['剪切', '剪下'],
  ['撤銷', '復原'],
  ['撤销', '復原'],
  ['運行', '執行'],
  ['运行', '執行'],
  ['線程', '執行緒'],
  ['线程', '執行緒'],
  ['存儲', '儲存'],
  ['存储', '儲存'],
  ['計算機', '電腦'],
  ['计算机', '電腦'],
  ['在線', '線上'],
  ['在线', '線上'],
  ['離線', '離線'],
  ['离线', '離線'],
  ['卸載', '解除安裝'],
  ['卸载', '解除安裝'],
  ['兼容', '相容'],
  ['字符', '字元'],
  ['字節', '位元組'],
  ['字节', '位元組'],
  ['布爾', '布林'],
  ['數組', '陣列'],
  ['数组', '陣列'],
  ['對象', '物件'],
  ['对象', '物件'],
  ['字符串', '字串'],
  ['後台', '背景'],
  ['后台', '背景'],
  ['客戶端', '用戶端'],
  ['客户端', '用戶端'],
  ['文件權限', '檔案權限'],
  ['文件权限', '檔案權限'],
  ['文檔', '文件'],
  ['文档', '文件'],
  ['軟件包', '軟體套件'],
  ['软件包', '軟體套件'],
  ['日誌', '記錄檔'],
  ['日志', '記錄檔'],
  ['校驗', '驗證'],
  ['校验', '驗證'],
  ['參數', '參數'],
  ['参数', '參數'],
  ['工作空間', '工作區'],
  ['工作空间', '工作區'],
  ['會話', '工作階段'],
  ['会话', '工作階段'],
  ['消息', '訊息'],
  ['定時', '排程'],
  ['定时', '排程'],
  ['構建', '建置'],
  ['构建', '建置'],
  ['發布', '發佈'],
  ['发布', '發佈'],
  ['遠程', '遠端'],
  ['远程', '遠端'],
  ['本地', '本機'],
  ['倉庫', '儲存庫'],
  ['仓库', '儲存庫'],
  ['操作系統', '作業系統'],
  ['操作系统', '作業系統'],
  ['驅動程序', '驅動程式'],
  ['驱动程序', '驅動程式'],
  ['端口', '連接埠'],
  ['隊列', '佇列'],
  ['队列', '佇列'],
  ['異步', '非同步'],
  ['异步', '非同步'],
  ['時區', '時區'],
  ['时区', '時區'],
  ['單擊', '點選'],
  ['单击', '點選'],
  ['點擊', '點選'],
  ['点击', '點選'],
  ['拖拽', '拖曳'],
  ['剪貼板', '剪貼簿'],
  ['剪贴板', '剪貼簿'],
  ['內存泄漏', '記憶體洩漏'],
  ['内存泄漏', '記憶體洩漏'],
  ['堆棧', '堆疊'],
  ['堆栈', '堆疊'],
  ['暫不支持', '暫不支援'],
  ['暂不支持', '暫不支援'],
  ['尚未運行', '尚未執行'],
  ['尚未运行', '尚未執行'],
  ['文件', '檔案'],
];

const PLACEHOLDER_RE = /\{\{[^}]+\}\}/g;

function extractPlaceholders(text) {
  return [...text.matchAll(PLACEHOLDER_RE)].map((m) => m[0]);
}

function applyTwTerms(text) {
  let out = opencc(text);
  for (const [from, to] of TW_TERMS) {
    out = out.replaceAll(from, to);
  }
  return out;
}

function walk(enNode, zhNode, pathParts = [], entries = []) {
  for (const key of Object.keys(enNode)) {
    const enVal = enNode[key];
    const zhVal = zhNode?.[key];
    const nextPath = [...pathParts, key];
    if (typeof enVal === 'string') {
      entries.push({
        path: nextPath,
        en: enVal,
        zh: typeof zhVal === 'string' ? zhVal : '',
      });
    } else if (enVal && typeof enVal === 'object') {
      walk(enVal, zhVal && typeof zhVal === 'object' ? zhVal : {}, nextPath, entries);
    }
  }
  return entries;
}

function setAtPath(root, pathParts, value) {
  let node = root;
  for (let i = 0; i < pathParts.length - 1; i += 1) {
    node = node[pathParts[i]];
  }
  node[pathParts[pathParts.length - 1]] = value;
}

function cloneShape(node) {
  if (typeof node === 'string') return '';
  if (Array.isArray(node)) return node.map(cloneShape);
  const out = {};
  for (const key of Object.keys(node)) out[key] = cloneShape(node[key]);
  return out;
}

function keyTreeEqual(a, b, path = '') {
  const ak = Object.keys(a);
  const bk = Object.keys(b);
  if (ak.length !== bk.length) {
    console.error(`Key count mismatch at ${path || '<root>'}: en=${ak.length}, other=${bk.length}`);
    return false;
  }
  let ok = true;
  for (const key of ak) {
    if (!Object.prototype.hasOwnProperty.call(b, key)) {
      console.error(`Missing key at ${path}.${key}`);
      ok = false;
      continue;
    }
    const av = a[key];
    const bv = b[key];
    if (typeof av !== typeof bv) {
      console.error(`Type mismatch at ${path}.${key}: ${typeof av} vs ${typeof bv}`);
      ok = false;
    } else if (av && typeof av === 'object') {
      ok = keyTreeEqual(av, bv, `${path}.${key}`) && ok;
    }
  }
  return ok;
}

function placeholderEqual(a, b, path = '') {
  let ok = true;
  for (const key of Object.keys(a)) {
    const av = a[key];
    const bv = b[key];
    const p = path ? `${path}.${key}` : key;
    if (typeof av === 'string') {
      const ap = extractPlaceholders(av).sort().join('|');
      const bp = extractPlaceholders(bv).sort().join('|');
      if (ap !== bp) {
        console.error(`Placeholder mismatch at ${p}: en=${ap} tw=${bp}`);
        ok = false;
      }
    } else if (av && typeof av === 'object') {
      ok = placeholderEqual(av, bv, p) && ok;
    }
  }
  return ok;
}

function main() {
  const entries = walk(en, zhCN);
  const zhTW = cloneShape(en);

  for (const { path: entryPath, en: enText, zh } of entries) {
    const source = zh || enText;
    let value = applyTwTerms(source);
    if (!zh && source === enText) {
      value = source;
    }
    setAtPath(zhTW, entryPath, value);
  }

  const effective = zhTW.scheduled?.settings?.keepAwakeState?.effective;
  if (effective && !zhCN.scheduled?.settings?.keepAwakeState?.effective_other) {
    zhTW.scheduled.settings.keepAwakeState.effective_other = effective;
  }

  const outPath = path.join(ROOT, 'zh-TW.json');
  fs.writeFileSync(outPath, `${JSON.stringify(zhTW, null, 2)}\n`, 'utf8');

  const treeMatch = keyTreeEqual(en, zhTW);
  const placeholderMatch = placeholderEqual(en, zhTW);
  console.log(JSON.stringify({ treeMatch, placeholderMatch, strings: entries.length, outPath }, null, 2));
}

main();
