import { describe, expect, it } from "vitest";
import {
  acpSessionEventsSignature,
  mergeAcpEventWindows,
} from "@/lib/acp-event-reducer";
import type { AcpSessionVm, AcpUiEventVm } from "@/types";

function event(partial: Partial<AcpUiEventVm> & Pick<AcpUiEventVm, "id" | "kind">): AcpUiEventVm {
  return {
    id: partial.id,
    seq: partial.seq ?? 1,
    timestamp: partial.timestamp ?? `${partial.seq ?? 1}Z`,
    kind: partial.kind,
    sessionId: partial.sessionId ?? "session-1",
    content: partial.content ?? null,
    title: partial.title ?? null,
    toolCallId: partial.toolCallId ?? null,
    status: partial.status ?? null,
    startedSeq: partial.startedSeq ?? partial.seq ?? 1,
    endedSeq: partial.endedSeq ?? partial.seq ?? 1,
    startedAt: partial.startedAt ?? partial.timestamp ?? `${partial.seq ?? 1}Z`,
    endedAt: partial.endedAt ?? partial.timestamp ?? `${partial.seq ?? 1}Z`,
    raw: partial.raw,
    timing: partial.timing,
  };
}

function session(events: AcpUiEventVm[]): Pick<AcpSessionVm, "events" | "eventPage"> {
  return {
    events,
    eventPage: {
      loadedCount: events.length,
      total: events.length,
      hasOlder: false,
      hasNewer: false,
    },
  };
}

describe("ACP event reducer", () => {
  it("replaces partial realtime text with the later complete stream snapshot", () => {
    const merged = mergeAcpEventWindows(
      [
        event({
          id: "assistant-message-1",
          kind: "textDelta",
          seq: 10,
          endedSeq: 10,
          content: "我找到了实现文件",
        }),
      ],
      [
        event({
          id: "assistant-message-1",
          kind: "textDelta",
          seq: 20,
          endedSeq: 20,
          content: "我找到了实现文件和现成测试文件，接下来核对代码内容并执行它们。",
        }),
      ],
    );

    expect(merged).toHaveLength(1);
    expect(merged[0]!.seq).toBe(10);
    expect(merged[0]!.endedSeq).toBe(20);
    expect(merged[0]!.content).toBe("我找到了实现文件和现成测试文件，接下来核对代码内容并执行它们。");
  });

  it("fills an initially empty realtime bubble when content arrives later", () => {
    const merged = mergeAcpEventWindows(
      [
        event({
          id: "assistant-message-empty",
          kind: "textDelta",
          seq: 10,
          endedSeq: 10,
          content: "",
        }),
      ],
      [
        event({
          id: "assistant-message-empty",
          kind: "textDelta",
          seq: 11,
          endedSeq: 11,
          content: "停止前也应该实时显示出来",
        }),
      ],
    );

    expect(merged[0]!.content).toBe("停止前也应该实时显示出来");
  });

  it("does not let an older shorter session snapshot overwrite newer live content", () => {
    const merged = mergeAcpEventWindows(
      [
        event({
          id: "assistant-message-1",
          kind: "textDelta",
          seq: 10,
          endedSeq: 30,
          timestamp: "30Z",
          content: "我已发现实现与需求存在明显偏差：代码返回的是 `hello`，不是 `hello-world`。",
        }),
      ],
      [
        event({
          id: "assistant-message-1",
          kind: "textDelta",
          seq: 9,
          endedSeq: 20,
          timestamp: "20Z",
          content: "我已发现实现与需求存在明显偏差",
        }),
      ],
    );

    expect(merged[0]!.endedSeq).toBe(30);
    expect(merged[0]!.content).toBe("我已发现实现与需求存在明显偏差：代码返回的是 `hello`，不是 `hello-world`。");
  });

  it("changes the session event signature when a non-last bubble receives more content", () => {
    const before = session([
      event({ id: "assistant-message-1", kind: "textDelta", seq: 10, content: "短文本" }),
      event({ id: "tool-call-1", kind: "toolCall", seq: 20, toolCallId: "call-1", status: "running" }),
    ]);
    const after = session([
      event({ id: "assistant-message-1", kind: "textDelta", seq: 10, endedSeq: 15, content: "短文本补齐后的完整内容" }),
      event({ id: "tool-call-1", kind: "toolCall", seq: 20, toolCallId: "call-1", status: "running" }),
    ]);

    expect(acpSessionEventsSignature(before)).not.toBe(acpSessionEventsSignature(after));
  });

  it("reorders placement-only provider history patches before the matching local prompt", () => {
    const prompt = (id: string, promptId: string, seq: number, content: string) =>
      event({
        id,
        kind: "userTextDelta",
        seq,
        content,
        raw: { source: "goldBandPrompt", promptId },
      });
    const externalRaw = (historyItemIndex: number, placement = false) => ({
      source: "providerHistory",
      historyProvider: "claude-acp",
      historyItemIndex,
      ...(placement
        ? {
            historyPlacement: {
              version: 1,
              afterPromptId: "prompt-1",
              beforePromptId: "prompt-2",
              gapTurnIndex: 1,
            },
          }
        : {}),
    });
    const previous = [
      prompt("gold-band-user-prompt-1", "prompt-1", 1, "hi"),
      event({ id: "assistant-message-first", kind: "textDelta", seq: 2, content: "hello" }),
      prompt("gold-band-user-prompt-2", "prompt-2", 29, "ask"),
      event({ id: "tool-call-ask", kind: "toolCall", seq: 30, toolCallId: "ask" }),
      event({
        id: "provider-user-external",
        kind: "userTextDelta",
        seq: 101,
        content: "这是我追加的信息",
        raw: externalRaw(1),
      }),
      event({
        id: "assistant-message-external",
        kind: "textDelta",
        seq: 102,
        content: "收到",
        raw: externalRaw(2),
      }),
    ];
    const merged = mergeAcpEventWindows(previous, [
      event({
        id: "provider-user-external",
        kind: "userTextDelta",
        seq: 101,
        content: "这是我追加的信息",
        raw: externalRaw(1, true),
      }),
      event({
        id: "assistant-message-external",
        kind: "textDelta",
        seq: 102,
        content: "收到",
        raw: externalRaw(2, true),
      }),
    ]);

    expect(merged.map((item) => item.id)).toEqual([
      "gold-band-user-prompt-1",
      "assistant-message-first",
      "provider-user-external",
      "assistant-message-external",
      "gold-band-user-prompt-2",
      "tool-call-ask",
    ]);
    expect(merged[2]!.seq).toBe(101);
  });
});
