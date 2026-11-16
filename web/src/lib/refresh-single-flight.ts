type RefreshFlight = {
  promise: Promise<unknown>;
  refresh?: () => Promise<unknown>;
};

/** One read per key; overlapping refreshes replace a single trailing read. */
export class RefreshSingleFlight {
  private readonly flights = new Map<string, RefreshFlight>();
  private readonly generations = new Map<string, number>();

  generation(key: string) {
    return this.generations.get(key) ?? 0;
  }

  isCurrent(key: string, generation: number) {
    return this.generation(key) === generation;
  }

  run<T>(key: string, request: () => Promise<T>, refresh = false): Promise<T> {
    const current = this.flights.get(key);
    if (current) {
      if (refresh) current.refresh = request;
      return current.promise as Promise<T>;
    }
    const flight: RefreshFlight = { promise: Promise.resolve() };
    this.flights.set(key, flight);
    const drain = async () => {
      let next = request;
      for (;;) {
        let result: T;
        try {
          result = await next();
        } catch (error) {
          if (this.flights.get(key) !== flight || !flight.refresh) throw error;
          next = flight.refresh as () => Promise<T>;
          flight.refresh = undefined;
          continue;
        }
        if (this.flights.get(key) !== flight || !flight.refresh) return result;
        next = flight.refresh as () => Promise<T>;
        flight.refresh = undefined;
      }
    };
    flight.promise = drain().finally(() => {
      if (this.flights.get(key) === flight) this.flights.delete(key);
    });
    return flight.promise as Promise<T>;
  }

  invalidate(key: string) {
    this.generations.set(key, this.generation(key) + 1);
    this.flights.delete(key);
  }
}
