import { useMapClock } from '../hooks/useMapClock';
import { formatClock } from '../lib/time';

/**
 * Time remaining on the current map.
 *
 * Every rule about what may and may not be shown lives here, in one place, so
 * the three render sites cannot drift apart on the question that matters: when
 * qctrl does not know how long the map has been running, it must say so rather
 * than show a number it cannot back up. Quake 2 publishes no map clock, so if
 * qctrl was not watching when the map started, that start is unrecoverable.
 */
type Variant = 'card' | 'chip' | 'stat';

interface MapCountdownProps {
  variant?: Variant;
}

const UNKNOWN_TOOLTIP =
  'A Quake 2 server does not report how long a map has been running. ' +
  'qctrl measures it by watching for the map to change, and it has not seen one yet ' +
  '(it started mid-map, or the server restarted). The clock becomes exact at the next map change.';

interface Display {
  text: string;
  tone: 'normal' | 'muted' | 'warn';
  title?: string;
}

function display(clock: ReturnType<typeof useMapClock>): Display {
  if (clock.quality === 'offline') {
    return { text: 'offline', tone: 'warn', title: 'Cannot reach the server.' };
  }

  if (!clock.known) {
    return {
      text: 'unknown',
      tone: 'muted',
      title: UNKNOWN_TOOLTIP,
    };
  }

  if (clock.quality === 'overdue') {
    return {
      text: 'overtime',
      tone: 'warn',
      title: 'Past the time limit with no map change — the server has not rotated.',
    };
  }

  if (clock.timelimitSeconds === 0) {
    return {
      text: formatClock(clock.elapsedSeconds ?? 0),
      tone: 'normal',
      title: 'No time limit set — counting up since the map started.',
    };
  }

  const text = formatClock(clock.remainingSeconds ?? 0);

  if (clock.quality === 'degraded') {
    return {
      text: `${text} (stale)`,
      tone: 'muted',
      title: 'Lost contact with the server; this countdown is no longer being re-synced.',
    };
  }

  return { text, tone: 'normal' };
}

const TONE_CLASS: Record<Display['tone'], string> = {
  normal: 'text-white',
  muted: 'text-gray-500',
  warn: 'text-yellow-400',
};

export function MapCountdown({ variant = 'card' }: MapCountdownProps) {
  const clock = useMapClock();
  const { text, tone, title } = display(clock);

  const label = clock.timelimitSeconds === 0 && clock.known ? 'Map Time' : 'Time Left';

  if (variant === 'chip') {
    return (
      <span
        title={title}
        className="inline-flex items-center gap-2 px-3 py-1 bg-gray-800 rounded text-sm"
      >
        <span className="text-gray-400">{label}</span>
        <span className={`font-mono font-bold ${TONE_CLASS[tone]}`}>{text}</span>
      </span>
    );
  }

  if (variant === 'stat') {
    return (
      <div className="p-3 bg-gray-700 rounded" title={title}>
        <div className="text-sm text-gray-400">{label}</div>
        <div className={`text-xl font-bold font-mono ${TONE_CLASS[tone]}`}>{text}</div>
      </div>
    );
  }

  return (
    <p className="text-sm text-gray-400 mt-1" title={title}>
      {label}:{' '}
      <span className={`font-mono font-bold ${TONE_CLASS[tone]}`}>{text}</span>
    </p>
  );
}
