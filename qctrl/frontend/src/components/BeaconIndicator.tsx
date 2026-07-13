import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';
import { beaconDisplay, type BeaconTone } from '../lib/beacon';

/** At-a-glance health of the qbots beacon link. All the rules live in `lib/beacon.ts`. */

const DOT_CLASS: Record<BeaconTone, string> = {
  ok: 'bg-green-400',
  idle: 'bg-yellow-400',
  down: 'bg-red-500',
  off: 'bg-gray-600',
};

const TEXT_CLASS: Record<BeaconTone, string> = {
  ok: 'text-green-400',
  idle: 'text-yellow-400',
  down: 'text-red-400',
  off: 'text-gray-500',
};

export function BeaconIndicator() {
  // Shares the ['status'] cache with the countdown and everything else — adds no network.
  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000,
  });

  const { text, tone, title } = beaconDisplay(status?.beacon);

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
