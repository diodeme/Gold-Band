import type { GitCommitVm, GitRefLabelVm } from '@/types';

const LOCAL_BRANCH_PREFIX = 'refs/heads/';
const REMOTE_BRANCH_PREFIX = 'refs/remotes/';
const TAG_PREFIX = 'refs/tags/';
const DETACHED_BRANCH = 'HEAD';

export interface CommitGraphEntry {
  hash: string;
  branch: string;
  parents: string[];
  message: string;
  committerDate: string;
  author?: { name: string; email?: string | null } | null;
  refs: GitRefLabelVm[];
  runtimeCheckpoint: boolean;
}

export function toCommitGraphEntries(
  commits: GitCommitVm[],
  currentBranch?: string | null,
): CommitGraphEntry[] {
  const fallbackBranch = currentBranch?.trim() || DETACHED_BRANCH;
  return commits.map((commit) => ({
    hash: commit.oid,
    branch: resolveCommitGraphBranch(commit, fallbackBranch),
    parents: [...commit.parentOids],
    message: commit.subject,
    committerDate: commit.committer.timestamp,
    author: {
      name: commit.author.name,
      email: commit.author.email,
    },
    refs: commit.refs.map((ref) => ({ ...ref })),
    runtimeCheckpoint: commit.runtimeCheckpoint,
  }));
}

export function resolveCommitGraphBranch(commit: GitCommitVm, fallbackBranch = DETACHED_BRANCH) {
  const sourceBranch = normalizeGraphBranch(commit.sourceRef);
  if (sourceBranch) return sourceBranch;

  const localBranch = commit.refs.find((ref) => ref.kind === 'local-branch');
  if (localBranch) return normalizeGraphBranch(localBranch.fullName) || localBranch.shortName;

  const remoteBranch = commit.refs.find((ref) => ref.kind === 'remote-branch');
  if (remoteBranch) return normalizeGraphBranch(remoteBranch.fullName) || remoteBranch.shortName;

  const tag = commit.refs.find((ref) => ref.kind === 'tag');
  if (tag) return normalizeGraphBranch(tag.fullName) || `tags/${tag.shortName}`;

  return fallbackBranch;
}

function normalizeGraphBranch(refName?: string | null) {
  const value = refName?.trim();
  if (!value) return null;
  if (value.startsWith(LOCAL_BRANCH_PREFIX)) return value.slice(LOCAL_BRANCH_PREFIX.length);
  if (value.startsWith(REMOTE_BRANCH_PREFIX)) return value.slice(REMOTE_BRANCH_PREFIX.length);
  if (value.startsWith(TAG_PREFIX)) return `tags/${value.slice(TAG_PREFIX.length)}`;
  return value;
}
