import { FileText, Paperclip, Upload } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AssetItemVm } from '../../types';
import { Button } from '@/components/ui/button';

interface ConversationAssetsBarProps {
  artifacts: AssetItemVm[];
  attachments: AssetItemVm[];
  inputAttachments?: AssetItemVm[];
  onOpenArtifact?: (artifact: AssetItemVm) => void;
  onOpenAttachment?: (attachment: AssetItemVm) => void;
  onOpenInputAttachment?: (item: AssetItemVm) => void;
}

export function ConversationAssetsBar({
  artifacts,
  attachments,
  inputAttachments,
  onOpenArtifact,
  onOpenAttachment,
  onOpenInputAttachment,
}: ConversationAssetsBarProps) {
  const { t } = useTranslation();

  if (artifacts.length === 0 && attachments.length === 0 && (!inputAttachments || inputAttachments.length === 0)) return null;

  return (
    <div className="flex flex-wrap items-center gap-2 border-t border-border/50 px-4 py-2">
      {inputAttachments && inputAttachments.length > 0 ? (
        <>
          <span className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider mr-1">
            {t('conversation.runtime.assetsBar.inputAttachments')}
          </span>
          {inputAttachments.map((item) => (
            <Button
              key={`input-${item.name}`}
              variant="ghost"
              size="sm"
              className="h-7 gap-1.5 rounded-full px-2.5 text-xs"
              onClick={() => onOpenInputAttachment?.(item)}
            >
              <Upload className="size-3 text-blue-400" />
              <span className="max-w-[120px] truncate">{item.title || item.name}</span>
            </Button>
          ))}
        </>
      ) : null}
      {artifacts.map((artifact) => (
        <Button
          key={`artifact-${artifact.name}`}
          variant="ghost"
          size="sm"
          className="h-7 gap-1.5 rounded-full px-2.5 text-xs"
          onClick={() => onOpenArtifact?.(artifact)}
        >
          <FileText className="size-3 text-emerald-500" />
          <span className="max-w-[120px] truncate">{artifact.title || artifact.name}</span>
        </Button>
      ))}
      {attachments.map((attachment) => (
        <Button
          key={`attachment-${attachment.name}`}
          variant="ghost"
          size="sm"
          className="h-7 gap-1.5 rounded-full px-2.5 text-xs"
          onClick={() => onOpenAttachment?.(attachment)}
        >
          <Paperclip className="size-3 text-muted-foreground" />
          <span className="max-w-[120px] truncate">{attachment.title || attachment.name}</span>
        </Button>
      ))}
      {artifacts.length === 0 && attachments.length === 0 && (!inputAttachments || inputAttachments.length === 0) ? (
        <span className="text-xs text-muted-foreground">
          {t('conversation.runtime.assetsBar.noArtifacts')}
        </span>
      ) : null}
    </div>
  );
}
