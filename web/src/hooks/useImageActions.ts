import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import {
  copyImageAsset,
  IMAGE_ACTION_FEEDBACK_DURATION_MS,
  saveImageAssetAs,
  type ImageActionAsset,
} from '@/lib/image-actions';

export type ImageActionState = 'idle' | 'copying' | 'saving' | 'copied' | 'saved' | 'failed';

export function useImageActions(asset: ImageActionAsset | null | undefined) {
  const { t } = useTranslation();
  const [state, setState] = useState<ImageActionState>('idle');

  useEffect(() => {
    if (state !== 'copied' && state !== 'saved') return;
    const completedState = state;
    const timeout = window.setTimeout(() => {
      setState((current) => current === completedState ? 'idle' : current);
    }, IMAGE_ACTION_FEEDBACK_DURATION_MS);
    return () => window.clearTimeout(timeout);
  }, [state]);

  const pending = state === 'copying' || state === 'saving';

  const copyImage = useCallback(async () => {
    if (!asset || pending) return;
    setState('copying');
    try {
      await copyImageAsset(asset);
      setState('copied');
    } catch {
      setState('failed');
    }
  }, [asset, pending]);

  const saveImage = useCallback(async () => {
    if (!asset || pending) return;
    setState('saving');
    try {
      setState(await saveImageAssetAs(asset) ? 'saved' : 'idle');
    } catch {
      setState('failed');
    }
  }, [asset, pending]);

  const message = state === 'copying'
    ? t('workspace.filesPanel.copyingImage')
    : state === 'saving'
      ? t('workspace.filesPanel.savingImage')
      : state === 'copied'
        ? t('workspace.filesPanel.imageCopied')
        : state === 'saved'
          ? t('workspace.filesPanel.imageSaved')
          : state === 'failed'
            ? t('workspace.filesPanel.imageActionFailed')
            : null;

  return { copyImage, message, pending, saveImage, state };
}

export type ImageActionsController = ReturnType<typeof useImageActions>;
