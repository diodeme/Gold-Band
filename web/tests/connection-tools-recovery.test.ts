// 连接工具失联恢复（连接→对话）回归（现场事故 d596d1c9）：
// agent 以失联标记结束本轮后，恢复态必须从时间线派生——最新消息是含标记的
// agent 消息 → 需要重试且重试内容 = 最近一条用户消息；用户重发（新 userTextDelta
// 落在标记之后）→ 恢复态消失；普通 agent 消息 / 复述标记的历史消息不触发。
import { describe, expect, it } from 'vitest';

import { deriveConnectionToolsRecovery } from '@/components/acp/ACPChatDialog';
import { CONNECTION_TOOLS_MISSING_MARKER } from '@/lib/slash-command';

type Item = { kind: string; content?: string | null };

function user(text: string): Item {
  return { kind: 'userTextDelta', content: text };
}
function agent(text: string): Item {
  return { kind: 'textDelta', content: text };
}
function toolCall(): Item {
  return { kind: 'toolCall', content: null };
}

describe('deriveConnectionToolsRecovery', () => {
  const userAsk = user('连接webank通过wessh aidx-core来查看日志');

  it('detects the marker on the latest agent message and keeps the last user text', () => {
    const recovery = deriveConnectionToolsRecovery([
      userAsk,
      toolCall(),
      agent(`无法继续，需要应用重新注册工具。\n\n${CONNECTION_TOOLS_MISSING_MARKER}`),
    ]);
    expect(recovery.markerDetected).toBe(true);
    expect(recovery.retryText).toBe('连接webank通过wessh aidx-core来查看日志');
  });

  it('ignores non-message items after the marker message', () => {
    const recovery = deriveConnectionToolsRecovery([
      userAsk,
      agent(`中断说明 ${CONNECTION_TOOLS_MISSING_MARKER}`),
      toolCall(),
      { kind: 'usageUpdate', content: null },
    ]);
    expect(recovery.markerDetected).toBe(true);
  });

  it('clears once the user sends a new message after the marker', () => {
    const recovery = deriveConnectionToolsRecovery([
      userAsk,
      agent(`失败 ${CONNECTION_TOOLS_MISSING_MARKER}`),
      user('重发：继续看日志'),
    ]);
    expect(recovery.markerDetected).toBe(false);
    expect(recovery.retryText).toBe('');
  });

  it('does not trigger for ordinary agent replies or a stale marker deep in history', () => {
    expect(deriveConnectionToolsRecovery([userAsk, agent('已连接，开始查看日志')]).markerDetected).toBe(false);
    // 历史轮次里出现过标记，但之后有新一轮正常对话 → 不触发。
    const recovery = deriveConnectionToolsRecovery([
      user('第一轮'),
      agent(`早前的失联 ${CONNECTION_TOOLS_MISSING_MARKER}`),
      user('第二轮'),
      agent('本轮一切正常'),
    ]);
    expect(recovery.markerDetected).toBe(false);
  });

  it('yields empty retry text when no user message exists', () => {
    const recovery = deriveConnectionToolsRecovery([
      agent(`恢复提示 ${CONNECTION_TOOLS_MISSING_MARKER}`),
    ]);
    expect(recovery.markerDetected).toBe(true);
    expect(recovery.retryText).toBe('');
  });
});
