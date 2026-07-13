import type { BeaconStatus } from './api';

/**
 * How the qbots beacon link should read at a glance, as a pure function of the backend's
 * `BeaconStatus`. Lives in `lib/` rather than beside the component so it is testable without a
 * render — and so the component file exports only a component (the react-refresh rule).
 *
 * The beacon is the unix socket a co-located qbots fleet publishes the server's frame counter
 * on; it is what makes the map countdown an exact measurement instead of an inference (see
 * `ClockSource`). Three rules, and they are the whole thing:
 *
 * 1. **Always say something.** An earlier version returned null when no beacon was configured,
 *    on the theory that a badge for an unused feature is nagging. That was wrong: an absent
 *    indicator is indistinguishable from a broken one, so "is the beacon on?" — the exact
 *    question this exists to answer — became unanswerable. `off` is a state, shown quietly.
 * 2. **Connected is about the SOCKET, not the data.** A live socket with an idle fleet is not a
 *    dead socket, and the backend reports the two separately, so we do too.
 * 3. **This says nothing about the countdown.** The clock's anchor is a fixed monotonic instant:
 *    it stays correct and keeps ticking long after the beacon drops. A red dot here does NOT
 *    mean the timer beside it is wrong.
 */

/** No frame in this long, on a live socket, means qbots is up but nothing is feeding it. */
const IDLE_AFTER_SECONDS = 5;

export type BeaconTone = 'ok' | 'idle' | 'down' | 'off';

export interface BeaconDisplay {
  text: string;
  tone: BeaconTone;
  title: string;
}

export function beaconDisplay(beacon: BeaconStatus | undefined): BeaconDisplay {
  if (!beacon?.enabled) {
    // Rule 1: not configured is a state, not a reason to vanish.
    return {
      text: 'off',
      tone: 'off',
      title:
        'No qbots beacon configured, so the map countdown is inferred from map changes rather ' +
        'than measured (it reads "unknown" if qctrl started mid-map). To enable: set ' +
        "`frames.socket_path` in qctrl's config.yaml and `beacon.enabled: true` in qbots', " +
        'pointing both at the same unix socket path.',
    };
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
