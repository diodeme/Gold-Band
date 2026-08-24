export interface ScheduledTaskDetailRefreshCoordinator<TRequest> {
  request(request: TRequest): void;
  beginForegroundRequest(): number;
  isCurrent(generation: number): boolean;
  invalidate(): void;
}

interface RefreshToken<TRequest> {
  generation: number;
  request: TRequest;
}

export function createScheduledTaskDetailRefreshCoordinator<TRequest, TResult>(options: {
  load: (request: TRequest) => Promise<TResult>;
  commit: (result: TResult) => void;
  fail: () => void;
}): ScheduledTaskDetailRefreshCoordinator<TRequest> {
  let generation = 0;
  let inFlight = false;
  let queued: RefreshToken<TRequest> | null = null;

  const isCurrent = (candidate: number) => candidate === generation;

  const drain = async (initial: RefreshToken<TRequest>) => {
    inFlight = true;
    let current: RefreshToken<TRequest> | null = initial;
    while (current) {
      try {
        const result = await options.load(current.request);
        if (isCurrent(current.generation)) options.commit(result);
      } catch {
        if (isCurrent(current.generation)) options.fail();
      }
      current = queued;
      queued = null;
    }
    inFlight = false;
  };

  return {
    request(request) {
      const token = { generation: ++generation, request };
      if (inFlight) {
        queued = token;
        return;
      }
      void drain(token);
    },
    beginForegroundRequest() {
      queued = null;
      return ++generation;
    },
    isCurrent,
    invalidate() {
      generation += 1;
      queued = null;
    },
  };
}
