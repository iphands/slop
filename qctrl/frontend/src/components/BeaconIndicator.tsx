import { useQuery } from '@tanstack/react-query';
import { getStatus, type BeaconStatus } from '../lib/api';

/**
 * At-a-glance health of the qbots beacon link.
 *
 * The beacon is the unix socket a co-located qbots fleet publishes the server's frame counter
 * on; it is what makes the countdown next to this an exact measurement instead of an inference
 * (see `ClockSource`). So it is worth knowing at a glance whether it is actually there.
 *
 * Three rules live here, and they are the whole component:
 *
 * 1. **Not configured ⇒ render nothing.** Most deployments have no qbots. Showing them a red
 *    "disconnected" badge for a feature they never turned on is nagging, not information.
 * 2. **Connected is about the SOCKET, not the data.** A live socket with an idle fleet is not
 *    the same thing as a dead socket, and the backend reports the two separately, so we do too.
 * 3. **This says nothing about the countdown.** The clock's anchor is a fixed monotonic instant:
 *    it stays correct and keeps ticking long after the beacon drops. A red dot here does NOT
 *    mean the timer beside it is wrong.
 */

/** No frame in this long, on a live socket, means qbots is up but nothing is feeding it. */
const IDLE_AFTER_SECONDS = 5;

type Tone = 'ok' | 'idle' | 'down';

interface Display {
  text: string;
  tone: Tone;
  title: string;
}

/** Pure, so every rule above is testable without a socket or a render. */
export function display(beacon: BeaconStatus | undefined): Display | null {
  if (!beacon?.enabled) {
    return null; // Rule 1: no qbots configured — say nothing.
  }

  if (!beacon.connected) {
    return {
      text: 'disconnected',
      tone: 'down',
      title:
        'No connection to the qbots beacon socket — qbots is probably not running. ' +
        'The map countdown falls back to inferring the map start from map changes, so it may ' +
        'read "unknown" until the next one. Any countdown already showing stays correct.',
    };
  }

  const age = beacon.last_frame_age_seconds;
  const stale = age === null || age > IDLE_AFTER_SECONDS;

  // Rule 2: the socket is up either way. Distinguish "up and fed" from "up and idle".
  if (stale || beacon.bots === 0) {
    return {
      text: 'idle',
      tone: 'idle',
      title:
        `Connected to qbots, but no server frames are arriving (${beacon.bots} bots` +
        (age === null ? ', no data yet).' : `, last frame ${age}s ago).`) +
        ' qbots is running but has no bots in the game — or the server is wedged.',
    };
  }

  return {
    text: 'connected',
    tone: 'ok',
    title:
      `Receiving the server's frame counter from qbots (${beacon.bots} ` +
      `bot${beacon.bots === 1 ? '' : 's'}). The map countdown is measured, not inferred.`,
  };
}

const DOT_CLASS: Record<Tone, string> = {
  ok: 'bg-green-400',
  idle: 'bg-yellow-400',
  down: 'bg-red-500',
};

const TEXT_CLASS: Record<Tone, string> = {
  ok: 'text-green-400',
  idle: 'text-yellow-400',
  down: 'text-red-400',
};

export function BeaconIndicator() {
  // Shares the ['status'] cache with the countdown and everything else — adds no network.
  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000,
  });

  const shown = display(status?.beacon);
  if (!shown) return null;

  const { text, tone, title } = shown;

  return (
    <span
      title={title}
      className="inline-flex items-center gap-2 px-3 py-1 bg-gray-800 rounded text-sm"
    >
      <span className="text-gray-400">qbots</span>
      <span className={`h-2 w-2 rounded-full ${DOT_CLASS[tone]}`} aria-hidden="true" />
      <span className={`font-mono ${TEXT_CLASS[tone]}`}>{text}</span>
    </span>
  );
}
