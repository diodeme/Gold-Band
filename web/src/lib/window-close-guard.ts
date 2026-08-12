export interface WindowCloseRequest {
  preventDefault(): void;
}

export type WindowCloseSaveFailureDecision = 'retry' | 'cancel' | 'discard';

export interface WindowCloseTransactionDependencies {
  flushPendingChanges(): Promise<boolean>;
  requestSaveFailureDecision(): Promise<WindowCloseSaveFailureDecision>;
  completeClose(): Promise<void>;
}

export interface WindowCloseTransaction {
  (event: WindowCloseRequest): Promise<void>;
  prepareToClose(): Promise<boolean>;
}

export function createWindowCloseTransaction(dependencies: WindowCloseTransactionDependencies): WindowCloseTransaction {
  let activeSaveTransaction: Promise<boolean> | null = null;
  let activeTransaction: Promise<void> | null = null;

  const runSaveTransaction = async () => {
    while (true) {
      const saved = await dependencies.flushPendingChanges().catch(() => false);
      if (saved) return true;

      const decision = await dependencies.requestSaveFailureDecision();
      if (decision === 'retry') continue;
      return decision === 'discard';
    }
  };

  const prepareToClose = () => {
    if (activeSaveTransaction) return activeSaveTransaction;
    const transaction = runSaveTransaction().finally(() => {
      if (activeSaveTransaction === transaction) activeSaveTransaction = null;
    });
    activeSaveTransaction = transaction;
    return transaction;
  };

  const handleCloseRequest = (event: WindowCloseRequest) => {
    event.preventDefault();
    if (activeTransaction) return activeTransaction;

    const transaction = prepareToClose()
      .then(async (proceed) => {
        if (proceed) await dependencies.completeClose();
      })
      .catch(() => undefined)
      .finally(() => {
      if (activeTransaction === transaction) activeTransaction = null;
    });
    activeTransaction = transaction;
    return transaction;
  };

  handleCloseRequest.prepareToClose = prepareToClose;
  return handleCloseRequest;
}
