import type { ConversationWorkLocation } from '@/types';

export const CONVERSATION_WORK_LOCATION_PREFERENCE_KEY = 'conversation.workLocation';
export const CONVERSATION_WORK_LOCATION_PREFERENCE_SCHEMA_VERSION = 1;

export interface ConversationWorkLocationPreference {
  schemaVersion: typeof CONVERSATION_WORK_LOCATION_PREFERENCE_SCHEMA_VERSION;
  byProjectId: Record<string, ConversationWorkLocation>;
}

export function parseConversationWorkLocationPreference(
  value: unknown,
): ConversationWorkLocationPreference {
  const empty: ConversationWorkLocationPreference = {
    schemaVersion: CONVERSATION_WORK_LOCATION_PREFERENCE_SCHEMA_VERSION,
    byProjectId: {},
  };
  if (!value || typeof value !== 'object' || Array.isArray(value)) return empty;
  const record = value as { schemaVersion?: unknown; byProjectId?: unknown };
  if (record.schemaVersion !== CONVERSATION_WORK_LOCATION_PREFERENCE_SCHEMA_VERSION) return empty;
  if (!record.byProjectId || typeof record.byProjectId !== 'object' || Array.isArray(record.byProjectId)) {
    return empty;
  }
  const byProjectId = Object.fromEntries(
    Object.entries(record.byProjectId)
      .filter((entry): entry is [string, ConversationWorkLocation] => (
        entry[0].trim().length > 0 && (entry[1] === 'main' || entry[1] === 'worktree')
      )),
  );
  return { ...empty, byProjectId };
}

export function conversationWorkLocationForProject(
  preference: ConversationWorkLocationPreference,
  projectId: string,
): ConversationWorkLocation {
  return preference.byProjectId[projectId] ?? 'main';
}

export function setConversationWorkLocationForProject(
  preference: ConversationWorkLocationPreference,
  projectId: string,
  location: ConversationWorkLocation,
): ConversationWorkLocationPreference {
  return {
    schemaVersion: CONVERSATION_WORK_LOCATION_PREFERENCE_SCHEMA_VERSION,
    byProjectId: { ...preference.byProjectId, [projectId]: location },
  };
}
