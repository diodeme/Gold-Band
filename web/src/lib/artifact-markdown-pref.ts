/**
 * 产物详情弹窗中「Markdown 渲染 / 原文」开关的持久化偏好。
 *
 * 设计说明：
 * - 偏好为单一全局布尔值（默认渲染），不按产物维度区分；
 * - 读写逻辑抽成纯函数，便于在组件外做单元测试；
 * - localStorage key 集中为常量，避免硬编码散落。
 */
export const ARTIFACT_MARKDOWN_RENDER_STORAGE_KEY = "gold-band-artifact-render-markdown";

/** 当未存储过偏好时使用的默认值：默认渲染 Markdown。 */
export const DEFAULT_ARTIFACT_MARKDOWN_RENDER = true;

/**
 * 读取产物 Markdown 渲染偏好；未存储或环境不可用时返回默认值。
 */
export function loadArtifactMarkdownRender(): boolean {
  if (typeof localStorage === "undefined") return DEFAULT_ARTIFACT_MARKDOWN_RENDER;
  const raw = localStorage.getItem(ARTIFACT_MARKDOWN_RENDER_STORAGE_KEY);
  if (raw === "true") return true;
  if (raw === "false") return false;
  return DEFAULT_ARTIFACT_MARKDOWN_RENDER;
}

/**
 * 写入产物 Markdown 渲染偏好；环境不可用时静默忽略。
 */
export function saveArtifactMarkdownRender(value: boolean): void {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(ARTIFACT_MARKDOWN_RENDER_STORAGE_KEY, value ? "true" : "false");
  } catch {
    // 配额限制或隐私模式：偏好无法持久化，忽略即可。
  }
}
