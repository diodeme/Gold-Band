import { ChevronDown, ChevronRight, Circle } from 'lucide-react';
import { useState } from 'react';
import type { ConversationSessionLeafVm, ConversationSessionTreeVm } from '../../types';
import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { cn } from '@/lib/utils';

interface ConversationSessionSwitcherProps {
  tree: ConversationSessionTreeVm;
  selectedKey?: string | null;
  onSelectSession: (leaf: ConversationSessionLeafVm) => void;
}

export function ConversationSessionSwitcher({
  tree,
  selectedKey,
  onSelectSession,
}: ConversationSessionSwitcherProps) {
  return (
    <div className="w-64 rounded-xl border border-border/60 bg-card/60 p-2 shadow-sm">
      {tree.rounds.length === 0 ? (
        <div className="px-3 py-4 text-center text-xs text-muted-foreground">No sessions</div>
      ) : (
        tree.rounds.map((round) => (
          <RoundNode key={round.roundId} round={round} selectedKey={selectedKey} onSelectSession={onSelectSession} />
        ))
      )}
    </div>
  );
}

function RoundNode({
  round,
  selectedKey,
  onSelectSession,
}: {
  round: ConversationSessionTreeVm['rounds'][0];
  selectedKey?: string | null;
  onSelectSession: (leaf: ConversationSessionLeafVm) => void;
}) {
  const [open, setOpen] = useState(true);

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger asChild>
        <Button variant="ghost" className="h-8 w-full justify-start gap-1.5 rounded-md px-2 text-xs font-medium">
          <ChevronDown className={cn('size-3 transition-transform', !open && '-rotate-90')} />
          {round.label}
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="ml-4 border-l border-border/60 pl-3">
          {round.nodes.map((node) => (
            <TreeNode key={node.nodeId} node={node} selectedKey={selectedKey} onSelectSession={onSelectSession} depth={0} />
          ))}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

function TreeNode({
  node,
  selectedKey,
  onSelectSession,
  depth,
}: {
  node: ConversationSessionTreeVm['rounds'][0]['nodes'][0];
  selectedKey?: string | null;
  onSelectSession: (leaf: ConversationSessionLeafVm) => void;
  depth: number;
}) {
  const [open, setOpen] = useState(true);

  return (
    <div>
      <Collapsible open={open} onOpenChange={setOpen}>
        <CollapsibleTrigger asChild>
          <Button variant="ghost" className="h-7 w-full justify-start gap-1.5 rounded-md px-2 text-xs">
            <ChevronDown className={cn('size-3 transition-transform', !open && '-rotate-90')} />
            <span className="truncate">{node.label}</span>
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <div className="ml-3 border-l border-border/60 pl-3">
            {node.attempts.map((attempt) => (
              <SessionLeaf
                key={`${attempt.roundId}/${attempt.nodeId}/${attempt.attemptId}`}
                leaf={attempt}
                selected={selectedKey === `${attempt.roundId}/${attempt.nodeId}/${attempt.attemptId}`}
                onSelect={() => onSelectSession(attempt)}
              />
            ))}
            {node.outerNodes?.map((outerNode) => (
              <TreeNode key={outerNode.nodeId} node={outerNode} selectedKey={selectedKey} onSelectSession={onSelectSession} depth={depth + 1} />
            ))}
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
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
  const statusColor = leaf.outcome === 'success'
    ? 'bg-emerald-500'
    : leaf.outcome === 'failure' || leaf.outcome === 'killed'
      ? 'bg-red-500'
      : leaf.status === 'running'
        ? 'bg-primary animate-pulse'
        : 'bg-yellow-500';

  return (
    <button
      type="button"
      className={cn(
        'flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-xs hover:bg-sidebar-accent',
        selected && 'bg-sidebar-accent text-sidebar-primary',
      )}
      onClick={onSelect}
    >
      <Circle className={cn('size-2 shrink-0 fill-current', statusColor)} />
      <span className="truncate">{leaf.pathLabel}</span>
    </button>
  );
}
