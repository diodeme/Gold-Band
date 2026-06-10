import { ChevronDown } from 'lucide-react';
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
  const isRunning = leaf.runtimeDisplay.tone === 'running';
  const statusDotClass = runtimeDotClass(leaf.runtimeDisplay.tone);

  return (
    <button
      type="button"
      className={cn(
        'flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-xs hover:bg-sidebar-accent',
        selected && 'bg-sidebar-accent text-sidebar-primary',
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
        {isRunning ? (
          <span className="absolute inset-0 rounded-full bg-primary/18 animate-ping" />
        ) : null}
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

function runtimeDotClass(tone?: string | null) {
  if (tone === 'success') return 'bg-emerald-500';
  if (tone === 'danger') return 'bg-red-500';
  if (tone === 'running') return 'bg-primary';
  if (tone === 'warning') return 'bg-yellow-500';
  if (tone === 'neutral') return 'bg-muted-foreground';
  return 'bg-muted-foreground';
}
