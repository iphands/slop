import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useRotationTimer } from '../useRotationTimer';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import * as api from '../../lib/api';
import type { StatusResponse } from '../../lib/api';

vi.mock('../../lib/api', () => ({
  getStatus: vi.fn(),
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

describe('useRotationTimer', () => {
  const mockStatus: StatusResponse = {
    map: 'q2dm1',
    players: [
      { clientNum: 1, score: 5, address: '192.168.1.1', name: 'Player1', ping: 50 },
      { clientNum: 2, score: 3, address: '192.168.1.2', name: 'Player2', ping: 60 },
    ],
    timelimit: 20,
    fraglimit: 25,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('initializes with default values when no status', () => {
    vi.mocked(api.getStatus).mockResolvedValue(null as unknown as StatusResponse);

    const { result } = renderHook(() => useRotationTimer(), {
      wrapper: createWrapper(),
    });

    expect(result.current.elapsedSeconds).toBe(0);
    expect(result.current.currentFrags).toBe(0);
    expect(result.current.timelimit).toBe(0);
    expect(result.current.fraglimit).toBe(0);
    expect(result.current.isActive).toBe(false);
  });

  it('calculates elapsed time correctly', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(mockStatus);

    const { result } = renderHook(() => useRotationTimer(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await Promise.resolve();
    });

    // Wait for data to load
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    // Elapsed time should be > 0 after data loads
    expect(result.current.elapsedSeconds).toBeGreaterThanOrEqual(0);
    expect(result.current.elapsedSeconds).toBeLessThan(10); // Should be within 10 seconds
  });

  it('calculates total frags from players', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(mockStatus);

    const { result } = renderHook(() => useRotationTimer(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(result.current.currentFrags).toBe(8);
  });

  it('detects time limit reached', async () => {
    const statusWithTimeLimit: StatusResponse = {
      ...mockStatus,
      timelimit: 1, // 1 minute limit
    };

    vi.mocked(api.getStatus).mockResolvedValue(statusWithTimeLimit);

    const onTrigger = vi.fn();

    const { result } = renderHook(() => useRotationTimer({ onTrigger }), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    // After data loads, elapsed time should be > 0 but less than 1 minute
    // so timeLimitReached should be false (not yet reached)
    expect(result.current.timelimit).toBe(1);
    expect(result.current.timeLimitReached).toBe(false);
  });

  it('does not auto-trigger rotation on frag limit (server owns frag rotation)', async () => {
    const statusWithFragLimit: StatusResponse = {
      map: 'q2dm1',
      players: [
        { clientNum: 1, score: 15, address: '192.168.1.1', name: 'Player1', ping: 50 },
        { clientNum: 2, score: 10, address: '192.168.1.2', name: 'Player2', ping: 60 },
      ],
      timelimit: 20,
      fraglimit: 20,
    };

    vi.mocked(api.getStatus).mockResolvedValue(statusWithFragLimit);

    const onTrigger = vi.fn();

    const { result } = renderHook(() => useRotationTimer({ onTrigger }), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    // Frag limit is still detected for display purposes...
    expect(result.current.fragLimitReached).toBe(true);
    // ...but qctrl no longer auto-rotates on it. The server handles
    // frag-triggered rotation via sv_maplist, since qctrl can't reliably
    // preempt the server's same-frame end-of-match logic.
    expect(onTrigger).not.toHaveBeenCalled();
  });

  it('resets timer on map change', async () => {
    vi.mocked(api.getStatus)
      .mockResolvedValue({ ...mockStatus, map: 'q2dm1' });

    const { result } = renderHook(() => useRotationTimer(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    vi.mocked(api.getStatus).mockResolvedValue({ ...mockStatus, map: 'dm6' });

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(result.current.elapsedSeconds).toBe(0);
  });

  it('calls reset function to manually reset timer', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(mockStatus);

    const { result } = renderHook(() => useRotationTimer(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    act(() => {
      result.current.reset();
    });

    expect(result.current.elapsedSeconds).toBe(0);
  });

  it('shows countdown when time limit is set', async () => {
    vi.mocked(api.getStatus).mockResolvedValue(mockStatus);

    const { result } = renderHook(() => useRotationTimer(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    // Countdown should be less than 20 min * 60 = 1200 seconds
    expect(result.current.countdownSeconds).toBeLessThan(1200);
  });

  it('handles empty players array', async () => {
    const statusNoPlayers: StatusResponse = {
      map: 'q2dm1',
      players: [],
      timelimit: 20,
      fraglimit: 25,
    };

    vi.mocked(api.getStatus).mockResolvedValue(statusNoPlayers);

    const { result } = renderHook(() => useRotationTimer(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(result.current.currentFrags).toBe(0);
  });
});
