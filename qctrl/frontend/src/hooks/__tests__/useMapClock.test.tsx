import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useMapClock } from '../useMapClock';
import * as api from '../../lib/api';
import type { MapClock, StatusResponse } from '../../lib/api';

vi.mock('../../lib/api', () => ({
  getStatus: vi.fn(),
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function clock(overrides: Partial<MapClock> = {}): MapClock {
  return {
    anchor: 'exact',
    elapsed_seconds: 60,
    quality: 'live',
    source: 'observed_edge',
    last_poll_age_seconds: 0,
    ...overrides,
  };
}

function status(overrides: Partial<StatusResponse> = {}): StatusResponse {
  return {
    map: 'q2dm1',
    players: [],
    timelimit: 10,
    fraglimit: 0,
    server_online: true,
    clock: clock(),
    ...overrides,
  };
}

describe('useMapClock', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('reports the elapsed time the server gave it', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(status());

    const { result } = renderHook(() => useMapClock(), { wrapper: createWrapper() });

    await waitFor(() => expect(result.current.known).toBe(true));
    expect(result.current.elapsedSeconds).toBe(60);
    // timelimit 10min = 600s, 60s elapsed.
    expect(result.current.remainingSeconds).toBe(540);
  });

  it('free-runs between polls so the display ticks every second', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(status());

    // Poll far enough apart that nothing re-anchors during this test: what is
    // under test here is purely the local 1 Hz tick.
    const { result } = renderHook(() => useMapClock({ pollingInterval: 60_000 }), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.elapsedSeconds).toBe(60));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });

    expect(result.current.elapsedSeconds).toBe(63);
    expect(result.current.remainingSeconds).toBe(537);
  });

  /**
   * The whole point of the hook. A local timer that merely counts would drift; this
   * one re-derives its anchor from the server's number on every poll, so when the
   * two disagree the server wins on the next poll rather than never.
   */
  it('snaps back to server truth when a poll disagrees with the local timer', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(status());

    const { result } = renderHook(() => useMapClock({ pollingInterval: 2000 }), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.elapsedSeconds).toBe(60));

    // Local timer runs on...
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.elapsedSeconds).toBe(61);

    // ...but the server says the map is actually much further along (this tab was
    // suspended, or the backend only just anchored the clock on a map change).
    vi.mocked(api.getStatus).mockResolvedValue(
      status({ clock: clock({ elapsed_seconds: 300 }) })
    );

    // Next poll lands.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    // The local guess (≈63) is discarded outright, not averaged or eased toward.
    await waitFor(() => expect(result.current.elapsedSeconds).toBe(300));
    expect(result.current.remainingSeconds).toBe(300);
  });

  /**
   * The honesty constraint, at the UI boundary: when the backend never saw the map
   * start, there is no number to show, and the hook must not manufacture one.
   */
  it('returns null — not zero — when the backend does not know when the map started', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(
      status({ clock: clock({ anchor: 'unknown', elapsed_seconds: null, source: 'none' }) })
    );

    const { result } = renderHook(() => useMapClock(), { wrapper: createWrapper() });

    await waitFor(() => expect(result.current.map).toBe('q2dm1'));
    expect(result.current.known).toBe(false);
    expect(result.current.elapsedSeconds).toBeNull();
    expect(result.current.remainingSeconds).toBeNull();

    // And it must not start counting from nowhere as time passes.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(result.current.elapsedSeconds).toBeNull();
  });

  it('stops counting down once the map is out of time', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(
      // timelimit 10min = 600s; 610s elapsed.
      status({ clock: clock({ elapsed_seconds: 610, quality: 'overdue' }) })
    );

    const { result } = renderHook(() => useMapClock(), { wrapper: createWrapper() });

    await waitFor(() => expect(result.current.known).toBe(true));
    expect(result.current.remainingSeconds).toBeNull();
    expect(result.current.quality).toBe('overdue');
  });

  it('counts up with no remaining time when there is no timelimit', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(status({ timelimit: 0 }));

    const { result } = renderHook(() => useMapClock(), { wrapper: createWrapper() });

    await waitFor(() => expect(result.current.elapsedSeconds).toBe(60));
    expect(result.current.remainingSeconds).toBeNull();
    expect(result.current.timelimitSeconds).toBe(0);
  });

  it('reports offline when the backend cannot reach the server', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(
      status({ server_online: false, clock: clock({ quality: 'degraded' }) })
    );

    const { result } = renderHook(() => useMapClock(), { wrapper: createWrapper() });

    await waitFor(() => expect(result.current.quality).toBe('offline'));
  });
});
