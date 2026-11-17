// CC 内建 /model 拦截回归（连接→对话）：/model 是 CLI local 命令，会话内重建
// 工具集会让 ACP 注入的连接 MCP 工具消失（现场事故）。识别必须用固定模式而非
// 动态命令目录——目录未加载/未收录时 `/model sonnet` 曾原样转发（d596d1c9）。
import { describe, expect, it } from 'vitest';

import {
  CONNECTION_TOOLS_MISSING_MARKER,
  parseModelSlashInput,
} from '@/lib/slash-command';

describe('fixed-pattern /model interception', () => {
  it('parses /model with args without any command catalog', () => {
    expect(parseModelSlashInput('/model sonnet 帮我看日志')).toBe('sonnet 帮我看日志');
    expect(parseModelSlashInput('/model sonnet')).toBe('sonnet');
    // 前导空白、大小写不敏感。
    expect(parseModelSlashInput('  /MODEL default')).toBe('default');
  });

  it('parses a bare /model to an empty arg', () => {
    expect(parseModelSlashInput('/model')).toBe('');
    expect(parseModelSlashInput('/model ')).toBe('');
    expect(parseModelSlashInput('/model   ')).toBe('');
  });

  it('does not match lookalikes or ordinary messages', () => {
    expect(parseModelSlashInput('/models sonnet')).toBeNull();
    expect(parseModelSlashInput('/modelx')).toBeNull();
    expect(parseModelSlashInput('请切换到 /model 模式')).toBeNull();
    expect(parseModelSlashInput('帮我看看日志')).toBeNull();
    expect(parseModelSlashInput('')).toBeNull();
  });

  it('exposes the connection tools missing marker for recovery detection', () => {
    expect(CONNECTION_TOOLS_MISSING_MARKER).toBe('[gold-band:connection-tools-missing]');
  });
});

