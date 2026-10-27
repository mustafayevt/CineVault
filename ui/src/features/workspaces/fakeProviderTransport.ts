import type {
  ProviderEvent,
  ProviderSearchRequest,
  ProviderSearchTransport,
} from "./providerEvents";

/** How one fake provider answers. */
export interface FakeProviderScript<T> {
  /** Milliseconds before it answers (default 0). */
  delayMs?: number;
  items?: readonly T[];
  /**
   * Hands the items over `batchSize` at a time, `batchMs` apart (default 150), as a provider does
   * while its images download: every batch but the last is a `partial` event, the last one the
   * `results` that ends the provider.
   */
  batchSize?: number;
  batchMs?: number;
  /** Answers with a failure instead of items. */
  fail?: { reason: string; retryable?: boolean };
  /** Answers that it has no key. */
  needsKey?: boolean;
  /** How it answers when asked again; by default it then delivers `items`. */
  onRetry?: Omit<FakeProviderScript<T>, "onRetry">;
}

export interface FakeProviderCalls {
  started: ProviderSearchRequest[];
  retried: { searchId: string; provider: string }[];
  cancelled: string[];
}

export interface FakeProviderTransport<T> {
  transport: ProviderSearchTransport<T>;
  calls: FakeProviderCalls;
  /** Sends an event on the newest running search, for tests that drive the stream by hand. */
  emit: (event: ProviderEvent<T>) => void;
}

interface Running<T> {
  sink: (event: ProviderEvent<T>) => void;
  timers: Set<ReturnType<typeof setTimeout>>;
}

/**
 * The synthetic backend's provider channel: scripted providers that answer after their own delay,
 * so a plain browser (and the tests) can show one provider failing while another streams. It
 * records `start`, `retry` and `cancel`, and `emit` sends hand-made events.
 */
export function createFakeProviderTransport<T>(
  script: Readonly<Record<string, FakeProviderScript<T>>>,
  options: {
    /** An event to send just before every `done`, as the scrape search says why it found nothing. */
    beforeDone?: () => ProviderEvent<T> | null;
  } = {},
): FakeProviderTransport<T> {
  const finish = (sink: (event: ProviderEvent<T>) => void) => {
    const extra = options.beforeDone?.() ?? null;
    if (extra !== null) sink(extra);
    sink({ kind: "done" });
  };
  const calls: FakeProviderCalls = { started: [], retried: [], cancelled: [] };
  const running = new Map<string, Running<T>>();
  let newest: string | null = null;

  const answer = (
    id: string,
    provider: string,
    how: Omit<FakeProviderScript<T>, "onRetry">,
    onSent: () => void,
  ) => {
    const search = running.get(id);
    if (search === undefined) return;
    search.sink({ kind: "started", provider });
    const timer = setTimeout(() => {
      search.timers.delete(timer);
      if (!running.has(id)) return;
      if (how.needsKey === true) {
        search.sink({ kind: "needsKey", provider });
        onSent();
        return;
      }
      if (how.fail !== undefined) {
        search.sink({
          kind: "failed",
          provider,
          reason: how.fail.reason,
          retryable: how.fail.retryable ?? true,
        });
        onSent();
        return;
      }
      const items = [...(how.items ?? [])];
      const size = how.batchSize ?? 0;
      if (size <= 0 || items.length <= size) {
        search.sink({ kind: "results", provider, items });
        onSent();
        return;
      }
      // The batches go out one after the other, the last as the provider's answer.
      let sent = 0;
      const next = () => {
        if (!running.has(id)) return;
        const batch = items.slice(sent, sent + size);
        sent += batch.length;
        if (sent >= items.length) {
          search.sink({ kind: "results", provider, items: batch });
          onSent();
          return;
        }
        search.sink({ kind: "partial", provider, items: batch });
        const again = setTimeout(() => {
          search.timers.delete(again);
          next();
        }, how.batchMs ?? 150);
        search.timers.add(again);
      };
      next();
    }, how.delayMs ?? 0);
    search.timers.add(timer);
  };

  const transport: ProviderSearchTransport<T> = {
    start: (request, onEvent) => {
      calls.started.push(request);
      running.set(request.searchId, { sink: onEvent, timers: new Set() });
      newest = request.searchId;
      let waiting = Object.keys(script).length;
      if (waiting === 0) {
        const timer = setTimeout(() => finish(onEvent), 0);
        running.get(request.searchId)?.timers.add(timer);
        return;
      }
      for (const [provider, how] of Object.entries(script)) {
        answer(request.searchId, provider, how, () => {
          waiting -= 1;
          const sink = running.get(request.searchId)?.sink;
          if (waiting === 0 && sink !== undefined) finish(sink);
        });
      }
    },
    retry: (searchId, provider) => {
      calls.retried.push({ searchId, provider });
      const how = script[provider];
      if (how === undefined) return;
      answer(searchId, provider, how.onRetry ?? { delayMs: how.delayMs, items: how.items }, () => {
        const sink = running.get(searchId)?.sink;
        if (sink !== undefined) finish(sink);
      });
    },
    cancel: (searchId) => {
      calls.cancelled.push(searchId);
      const search = running.get(searchId);
      if (search === undefined) return;
      for (const timer of search.timers) clearTimeout(timer);
      running.delete(searchId);
    },
  };

  return {
    transport,
    calls,
    emit: (event) => {
      if (newest !== null) running.get(newest)?.sink(event);
    },
  };
}

/**
 * One transport for many searches: each `start` builds the fake that answers it (from the
 * request, so the query and the parameters can shape the answer), and `retry` and `cancel` find
 * that fake again by the search ID. The synthetic backends use it to stand in for a streaming
 * command.
 */
export function perSearchTransport<T>(
  make: (request: ProviderSearchRequest) => FakeProviderTransport<T>,
): ProviderSearchTransport<T> {
  const running = new Map<string, FakeProviderTransport<T>>();
  return {
    start: (request, onEvent) => {
      const fake = make(request);
      running.set(request.searchId, fake);
      fake.transport.start(request, onEvent);
    },
    retry: (searchId, provider) => running.get(searchId)?.transport.retry(searchId, provider),
    cancel: (searchId) => {
      running.get(searchId)?.transport.cancel(searchId);
      running.delete(searchId);
    },
  };
}
