import { ChevronDown } from 'lucide-react';
import type { ConversationSessionLeafVm, ConversationSessionTreeVm } from '../../types';
import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';
import { runtimeStatusDotClass } from '@/lib/runtime-status-dot';
import type { ConversationSessionTreeExpansion } from '@/lib/conversation-run-cache';

const SESSION_TREE_ROW_HOVER_CLASS = 'hover:bg-sidebar-accent/55 hover:text-sidebar-accent-foreground';
export const CONVERSATION_SESSION_TREE_SCROLL_MAX_HEIGHT = 'min(32rem, var(--radix-popover-content-available-height))';

interface ConversationSessionSwitcherProps {
  tree: ConversationSessionTreeVm;
  selectedKey?: string | null;
  expansion: ConversationSessionTreeExpansion;
  onExpansionChange: (branchKey: string, open: boolean) => void;
  onSelectSession: (leaf: ConversationSessionLeafVm) => void;
}

export function ConversationSessionSwitcher({
  tree,
  selectedKey,
  expansion,
  onExpansionChange,
  onSelectSession,
}: ConversationSessionSwitcherProps) {
  return (
    <ScrollArea
      data-conversation-session-tree-scroll="true"
      className="w-64 overflow-hidden [&_[data-slot=scroll-area-viewport]]:max-h-[inherit]"
      style={{ maxHeight: CONVERSATION_SESSION_TREE_SCROLL_MAX_HEIGHT }}
    >
      <div className="p-2">
        {tree.rounds.length === 0 ? (
          <div className="px-3 py-4 text-center text-xs text-muted-foreground">No sessions</div>
        ) : (
          tree.rounds.map((round) => (
            <RoundNode
              key={round.roundId}
              round={round}
              selectedKey={selectedKey}
              expansion={expansion}
              onExpansionChange={onExpansionChange}
              onSelectSession={onSelectSession}
            />
          ))
        )}
      </div>
    </ScrollArea>
  );
}

function RoundNode({
  round,
  selectedKey,
  expansion,
  onExpansionChange,
  onSelectSession,
}: {
  round: ConversationSessionTreeVm['rounds'][0];
  selectedKey?: string | null;
  expansion: ConversationSessionTreeExpansion;
  onExpansionChange: (branchKey: string, open: boolean) => void;
  onSelectSession: (leaf: ConversationSessionLeafVm) => void;
}) {
  const branchPath = ['round', round.roundId];
  const branchKey = conversationSessionTreeBranchKey(branchPath);
  const open = expansion[branchKey] ?? true;

  return (
    <Collapsible open={open} onOpenChange={(nextOpen) => onExpansionChange(branchKey, nextOpen)}>
      <CollapsibleTrigger asChild>
        <Button variant="ghost" className={cn('h-8 w-full justify-start gap-1.5 rounded-md px-2 text-xs font-medium', SESSION_TREE_ROW_HOVER_CLASS)}>
          <ChevronDown className={cn('size-3 transition-transform', !open && '-rotate-90')} />
          {round.label}
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="ml-4 border-l border-border/60 pl-3">
          {round.nodes.map((node) => (
            <TreeNode
              key={node.nodeId}
              node={node}
              selectedKey={selectedKey}
              expansion={expansion}
              onExpansionChange={onExpansionChange}
              onSelectSession={onSelectSession}
              branchPath={[...branchPath, 'node', node.nodeId]}
            />
          ))}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

function TreeNode({
  node,
  selectedKey,
  expansion,
  onExpansionChange,
  onSelectSession,
  branchPath,
}: {
  node: ConversationSessionTreeVm['rounds'][0]['nodes'][0];
  selectedKey?: string | null;
  expansion: ConversationSessionTreeExpansion;
  onExpansionChange: (branchKey: string, open: boolean) => void;
  onSelectSession: (leaf: ConversationSessionLeafVm) => void;
  branchPath: readonly string[];
}) {
  const branchKey = conversationSessionTreeBranchKey(branchPath);
  const open = expansion[branchKey] ?? true;

  return (
    <div>
      <Collapsible open={open} onOpenChange={(nextOpen) => onExpansionChange(branchKey, nextOpen)}>
        <CollapsibleTrigger asChild>
          <Button variant="ghost" className={cn('h-7 w-full justify-start gap-1.5 rounded-md px-2 text-xs', SESSION_TREE_ROW_HOVER_CLASS)}>
            <ChevronDown className={cn('size-3 transition-transform', !open && '-rotate-90')} />
            <span className="truncate">{node.label}</span>
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <div className="ml-3 border-l border-border/60 pl-3">
            {node.attempts.map((attempt) => {
                const key = attempt.outerNodeId && attempt.outerAttemptId
                  ? `${attempt.roundId}/${attempt.outerNodeId}/${attempt.outerAttemptId}/${attempt.nodeId}/${attempt.attemptId}`
                  : `${attempt.roundId}/${attempt.nodeId}/${attempt.attemptId}`;
                return (
                  <SessionLeaf
                    key={key}
                    leaf={attempt}
                    selected={selectedKey === key}
                    onSelect={() => onSelectSession(attempt)}
                  />
                );
              })}
            {node.outerNodes?.map((outerNode) => (
              <TreeNode
                key={outerNode.nodeId}
                node={outerNode}
                selectedKey={selectedKey}
                expansion={expansion}
                onExpansionChange={onExpansionChange}
                onSelectSession={onSelectSession}
                branchPath={[...branchPath, 'outer-node', outerNode.nodeId]}
              />
            ))}
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}

export function conversationSessionTreeBranchKey(branchPath: readonly string[]) {
  return JSON.stringify(branchPath);
}

function SessionLeaf({
  leaf,
  selected,
  onSelect,
}: {
  leaf: ConversationSessionLeafVm;
  selected: boolean;
  onSelect: () => void;
}) {
  const statusDotClass = runtimeStatusDotClass(leaf.runtimeDisplay.tone);

  return (
    <button
      type="button"
      aria-current={selected ? 'true' : undefined}
      data-selected={selected}
      className={cn(
        'flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-xs',
        SESSION_TREE_ROW_HOVER_CLASS,
        selected && 'bg-sidebar-accent text-sidebar-accent-foreground hover:bg-sidebar-accent',
      )}
      onClick={onSelect}
    >
      <span
        aria-hidden="true"
        className={cn(
          'relative inline-flex size-3 shrink-0 items-center justify-center rounded-full border border-background/80',
          selected && 'border-sidebar-accent/80',
        )}
      >
        <span
          className={cn(
            'relative inline-block size-2 rounded-full',
            statusDotClass,
          )}
        />
      </span>
      <span className="truncate">{leaf.pathLabel}</span>
    </button>
  );
}
