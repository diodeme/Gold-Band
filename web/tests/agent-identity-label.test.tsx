import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { AgentIdentityLabel } from '@/components/AgentIdentityLabel';

function source(relativePath: string) {
  return readFileSync(fileURLToPath(new URL(relativePath, import.meta.url)), 'utf8');
}

describe('AgentIdentityLabel', () => {
  it('renders the registry icon beside the agent name', () => {
    const html = renderToStaticMarkup(
      React.createElement(AgentIdentityLabel, { iconKey: 'claude', name: 'Claude' }),
    );

    expect(html).toContain('/agent-icons/claude.svg');
    expect(html).toContain('Claude');
    expect(html).toContain('dark:invert');
    expect(html).not.toContain('alt="Claude"');
  });

  it('draws Codex at slot size so the glyph is neither cropped nor overflowing', () => {
    const html = renderToStaticMarkup(
      React.createElement(AgentIdentityLabel, { iconKey: 'codex', name: 'Codex' }),
    );

    expect(html).toContain('overflow-hidden');
    expect(html).not.toContain('scale-125');
    expect(html).not.toContain('scale-110');
  });
});

describe('Select-based agent pickers share registry icons', () => {
  it('AUTO composer options render AgentIdentityLabel from iconKey', () => {
    const composer = source('../src/components/conversation/ConversationComposer.tsx');
    expect(composer).toContain("import { AgentIcon, AgentIdentityLabel } from '@/components/AgentIdentityLabel'");
    expect(composer).toMatch(
      /SelectItem key=\{a\.agentType\}[\s\S]*<AgentIdentityLabel iconKey=\{a\.iconKey\} name=\{a\.displayName\}/,
    );
  });

  it('run-mode, workflow inspector, analytics, and skill filter pickers reuse the same label', () => {
    const runMode = source('../src/pages/RunModeManagementPage.tsx');
    const workflow = source('../src/components/WorkflowEditor.tsx');
    const analytics = source('../src/pages/PersonalAnalyticsPage.tsx');
    const context = source('../src/pages/ContextManagementPage.tsx');

    expect(runMode).toMatch(
      /SelectItem key=\{item\.agentType\}[\s\S]*<AgentIdentityLabel iconKey=\{item\.iconKey\} name=\{item\.displayName\}/,
    );
    expect(runMode).toMatch(
      /toggleAvailableAgent\(item\.agentType\)[\s\S]*<AgentIdentityLabel iconKey=\{item\.iconKey\} name=\{item\.displayName\}/,
    );
    expect(workflow).toMatch(
      /function AgentSelectItemContent[\s\S]*<AgentIdentityLabel iconKey=\{agent\.iconKey\} name=\{agent\.displayName\}/,
    );
    expect(analytics).toMatch(
      /data-personal-analytics-agent="true"[\s\S]*<AgentIdentityLabel iconKey=\{agent\.iconKey\} name=\{agent\.displayName\}/,
    );
    expect(analytics).not.toMatch(/<Bot className="size-4" \/>/);
    expect(context).toMatch(
      /configuredAgents\.map\(\(agent\) => \(\s*<SelectItem key=\{agent\.agentType\} value=\{agent\.agentType\}[^>]*>\s*<AgentIdentityLabel iconKey=\{agent\.iconKey\} name=\{agent\.label\}/,
    );
  });

  it('keeps the conversation sidebar on the default visual-weight icon class', () => {
    const sidebar = source('../src/components/conversation/ConversationSidebar.tsx');
    expect(sidebar).toMatch(
      /agentIconClass\(task\.agentIdentity\.iconKey, cn\('size-3'/,
    );
    expect(sidebar).not.toContain('compensateWhitespace: false');
  });

  it('keeps whitespace compensation on padded canvas and card seats', () => {
    const workflow = source('../src/components/WorkflowEditor.tsx');
    const agents = source('../src/pages/AgentManagementPage.tsx');
    expect(workflow).toContain("agentIconClass(data.iconKey, 'size-4', { compensateWhitespace: true })");
    expect(agents).toContain("agentIconClass(editor.form.icon, 'size-6', { compensateWhitespace: true })");
    expect(agents).toContain("agentIconClass(agent.iconKey, 'size-6', { compensateWhitespace: true })");
  });
});
