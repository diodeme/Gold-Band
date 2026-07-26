import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const source = readFileSync(fileURLToPath(new URL('../src/pages/ContextManagementPage.tsx', import.meta.url)), 'utf8');

describe('context management loading boundaries', () => {
  it('loads only profile data when the profile tab mounts', () => {
    expect(source).not.toContain('getConversationSidebar');
    expect(source).toContain("const needsSkillContext = activeTab === 'skills' || skillSheetMode === 'create';");
    expect(source).toContain('if (!needsSkillContext) return;');
    expect(source).toContain('getConversationWorkspaces().then(setWorkspaces)');
    expect(source).toContain('useEffect(() => { void refresh(); }, []);');
    expect(source).toContain("useEffect(() => { if (activeTab === 'mcp'");
    expect(source).toContain("if (activeTab !== 'skills') return;");
  });
});
