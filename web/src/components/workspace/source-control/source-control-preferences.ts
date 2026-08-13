const SOURCE_CONTROL_PREFERENCES_STORAGE_KEY = 'gold-band:source-control-preferences';
const SOURCE_CONTROL_PREFERENCES_VERSION = 1;
const MAX_REMEMBERED_REPOSITORIES = 64;

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

interface RemotePreference {
  remote: string;
  updatedAt: number;
}

interface SourceControlPreferencesV1 {
  version: typeof SOURCE_CONTROL_PREFERENCES_VERSION;
  remoteByRepository: Record<string, RemotePreference>;
}

export function resolvePreferredGitRemote(
  repositoryCommonDir: string,
  availableRemotes: readonly string[],
  upstreamName?: string | null,
  storage = browserStorage(),
) {
  const remembered = readPreferences(storage).remoteByRepository[normalizeRepositoryIdentity(repositoryCommonDir)]?.remote;
  if (remembered && availableRemotes.includes(remembered)) return remembered;
  const upstreamRemote = [...availableRemotes]
    .sort((left, right) => right.length - left.length)
    .find((remote) => upstreamName === remote || upstreamName?.startsWith(`${remote}/`));
  return upstreamRemote ?? availableRemotes[0] ?? '';
}

export function rememberPreferredGitRemote(
  repositoryCommonDir: string,
  remote: string,
  storage = browserStorage(),
) {
  if (!storage || !remote) return;
  const preferences = readPreferences(storage);
  preferences.remoteByRepository[normalizeRepositoryIdentity(repositoryCommonDir)] = {
    remote,
    updatedAt: Date.now(),
  };
  const entries = Object.entries(preferences.remoteByRepository)
    .sort(([, left], [, right]) => right.updatedAt - left.updatedAt)
    .slice(0, MAX_REMEMBERED_REPOSITORIES);
  storage.setItem(SOURCE_CONTROL_PREFERENCES_STORAGE_KEY, JSON.stringify({
    version: SOURCE_CONTROL_PREFERENCES_VERSION,
    remoteByRepository: Object.fromEntries(entries),
  } satisfies SourceControlPreferencesV1));
}

function readPreferences(storage: StorageLike | null): SourceControlPreferencesV1 {
  if (!storage) return emptyPreferences();
  try {
    const parsed = JSON.parse(storage.getItem(SOURCE_CONTROL_PREFERENCES_STORAGE_KEY) ?? 'null') as Partial<SourceControlPreferencesV1> | null;
    if (parsed?.version !== SOURCE_CONTROL_PREFERENCES_VERSION || !parsed.remoteByRepository || typeof parsed.remoteByRepository !== 'object') {
      return emptyPreferences();
    }
    const remoteByRepository = Object.fromEntries(Object.entries(parsed.remoteByRepository).filter((entry): entry is [string, RemotePreference] => {
      const value = entry[1] as Partial<RemotePreference> | null;
      return Boolean(value && typeof value.remote === 'string' && typeof value.updatedAt === 'number');
    }));
    return { version: SOURCE_CONTROL_PREFERENCES_VERSION, remoteByRepository };
  } catch {
    return emptyPreferences();
  }
}

function emptyPreferences(): SourceControlPreferencesV1 {
  return { version: SOURCE_CONTROL_PREFERENCES_VERSION, remoteByRepository: {} };
}

function browserStorage(): StorageLike | null {
  return typeof window === 'undefined' ? null : window.localStorage;
}

function normalizeRepositoryIdentity(commonDir: string) {
  const normalized = commonDir.replaceAll('\\', '/').replace(/\/$/u, '');
  return /^[a-z]:\//iu.test(normalized) ? normalized.toLowerCase() : normalized;
}
