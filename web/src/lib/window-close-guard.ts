export interface WindowCloseRequest {
  preventDefault(): void;
}

export type WindowCloseSaveFailureDecision = 'retry' | 'cancel' | 'discard';

export interface WindowCloseTransactionDependencies {
  flushPendingChanges(): Promise<boolean>;
  requestSaveFailureDecision(): Promise<WindowCloseSaveFailureDecision>;
  prepareAppExit(): Promise<unknown>;
  destroyWindow(): Promise<void>;
}

export function createWindowCloseTransaction(dependencies: WindowCloseTransactionDependencies) {
  let activeTransaction: Promise<void> | null = null;

  const runTransaction = async () => {
    while (true) {
      const saved = await dependencies.flushPendingChanges().catch(() => false);
      if (saved) break;

      const decision = await dependencies.requestSaveFailureDecision();
      if (decision === 'retry') continue;
      if (decision === 'cancel') return;
      break;
    }

    await dependencies.prepareAppExit();
    await dependencies.destroyWindow();
  };

  return (event: WindowCloseRequest) => {
    event.preventDefault();
    if (activeTransaction) return activeTransaction;

    const transaction = runTransaction().catch(() => undefined).finally(() => {
      if (activeTransaction === transaction) activeTransaction = null;
    });
    activeTransaction = transaction;
    return transaction;
  };
}
