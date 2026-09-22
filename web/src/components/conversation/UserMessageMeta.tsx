import { SlashCommandInputTag } from '@/components/conversation/SlashCommandInputTag';
import { UserMessageQuotes } from '@/components/conversation/UserMessageQuotes';
import { slashTokenFromName } from '@/lib/slash-command';
import type { UserPromptQuote, UserPromptRole } from '@/types';

export function UserMessageMeta({
  role,
  quotes,
}: {
  role: UserPromptRole | null;
  quotes: readonly UserPromptQuote[];
}) {
  if (!role && quotes.length === 0) return null;

  return (
    <div
      className="mb-0.5 flex flex-wrap items-center justify-end gap-1.5"
      data-user-message-meta="true"
    >
      {role ? (
        <SlashCommandInputTag
          prefix={`@${slashTokenFromName(role.name) || role.name}`}
          content={role.content}
          kind="role"
          iconSrc="/logo.svg"
          surface="meta"
        />
      ) : null}
      <UserMessageQuotes quotes={quotes} />
    </div>
  );
}
