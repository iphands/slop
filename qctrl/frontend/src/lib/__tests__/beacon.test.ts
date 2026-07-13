import { describe, expect, it } from 'vitest';
import { beaconDisplay as display } from '../beacon';
import type { BeaconStatus } from '../api';

function beacon(overrides: Partial<BeaconStatus> = {}): BeaconStatus {
  return {
    enabled: true,
    connected: true,
    bots: 8,
    last_frame_age_seconds: 0,
    ...overrides,
  };
}

describe('beaconDisplay', () => {
  /**
   * An earlier version returned null here, so the chip vanished when no beacon was configured.
   * That made the one question this component exists to answer — "is the beacon on?" —
   * unanswerable: an absent indicator looks exactly like a broken one. Off is a state.
   */
  it('shows a quiet off state when no beacon is configured', () => {
    const d = display(beacon({ enabled: false }));
    expect(d.text).toBe('off');
    expect(d.tone).toBe('off');
    // And it says how to turn it on, rather than just sitting there grey.
    expect(d.title).toMatch(/socket_path/);
  });

  it('shows off when the backend predates the beacon field', () => {
    expect(display(undefined).text).toBe('off');
  });

  it('reports disconnected when the socket is down', () => {
    const d = display(beacon({ connected: false, bots: 0 }));
    expect(d?.text).toBe('disconnected');
    expect(d?.tone).toBe('down');
  });

  it('reassures that a dropped beacon does not invalidate the countdown', () => {
    // The clock's anchor is a fixed monotonic instant: it keeps ticking correctly with the
    // beacon gone. Someone seeing a red dot must not conclude the timer is lying.
    const d = display(beacon({ connected: false, bots: 0 }));
    expect(d?.title).toMatch(/stays correct/i);
  });

  it('reports connected when frames are arriving', () => {
    const d = display(beacon({ bots: 8, last_frame_age_seconds: 0 }));
    expect(d?.text).toBe('connected');
    expect(d?.tone).toBe('ok');
    expect(d?.title).toMatch(/8 bots/);
  });

  it('pluralizes a single bot', () => {
    expect(display(beacon({ bots: 1 }))?.title).toMatch(/1 bot\b/);
  });

  /**
   * The distinction the backend goes to the trouble of reporting separately: a live socket with
   * an idle fleet is NOT a dead socket. Collapsing the two would tell someone qbots is down when
   * it is running perfectly well with no bots in the game.
   */
  it('distinguishes a live-but-idle socket from a dead one', () => {
    const idle = display(beacon({ connected: true, bots: 0 }));
    expect(idle?.text).toBe('idle');
    expect(idle?.tone).toBe('idle');

    const dead = display(beacon({ connected: false, bots: 0 }));
    expect(dead?.text).toBe('disconnected');
  });

  it('goes idle when frames stop arriving on a live socket', () => {
    // The wedged-server case: socket up, nothing coming through it.
    const d = display(beacon({ bots: 8, last_frame_age_seconds: 30 }));
    expect(d?.text).toBe('idle');
    expect(d?.title).toMatch(/last frame 30s ago/);
  });

  it('is idle, not connected, before the first frame arrives', () => {
    const d = display(beacon({ bots: 0, last_frame_age_seconds: null }));
    expect(d?.text).toBe('idle');
    expect(d?.title).toMatch(/no data yet/);
  });

  it('tolerates the normal 1 Hz beacon heartbeat without flapping to idle', () => {
    // qbots publishes at 1 Hz, so an age of 1-2s is healthy, not a stall.
    expect(display(beacon({ last_frame_age_seconds: 2 }))?.text).toBe('connected');
  });
});
