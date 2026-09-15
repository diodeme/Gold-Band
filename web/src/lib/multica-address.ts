/// Multica 连接地址前端校验（与后端 `normalize_multica_base_url` 的 scheme/host 要求对齐）。
/// 连接确认弹窗与连接地址设置弹窗共用。
export function isValidHttpUrl(raw: string): boolean {
  try {
    const url = new URL(raw);
    return (url.protocol === 'http:' || url.protocol === 'https:') && url.hostname !== '';
  } catch {
    return false;
  }
}
