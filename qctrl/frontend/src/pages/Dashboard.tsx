import { useQuery } from '@tanstack/react-query';
import { getHealth, getStatus } from '../lib/api';
import { useChanges } from '../contexts/ChangesContext';
import { Section } from '../components/Section';
import { MapCountdown } from '../components/MapCountdown';
import { Link } from 'react-router-dom';

export function Dashboard() {
  const { getServerValue } = useChanges();
  const { data: health, refetch: refetchHealth } = useQuery({
    queryKey: ['health'],
    queryFn: getHealth,
    refetchInterval: 10000,
  });

  const { data: status, refetch: refetchStatus } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 10000,
  });

  const players = status?.players || [];
  const currentMap = status?.map || 'Loading...';
  const timelimit = Number(getServerValue('timelimit'));
  const fraglimit = Number(getServerValue('fraglimit'));
  const currentDmflags = Number(getServerValue('dmflags'));

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Server Dashboard</h1>

      <Section>
        <div className="flex justify-between items-center mb-4">
          <h2 className="text-lg font-semibold">Server Status</h2>
          <button
            type="button"
            onClick={() => { refetchHealth(); refetchStatus(); }}
            className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
          >
            Refresh
          </button>
        </div>
        <div className="flex items-center gap-2">
          <span className={health ? 'text-green-500 text-lg' : 'text-red-500 text-lg'}>
            ●
          </span>
          <span className={health ? 'text-green-400' : 'text-red-400'}>
            {health ? 'Online' : 'Offline'}
          </span>
        </div>
        <p className="text-sm text-gray-400 mt-2">
          Last updated: {new Date().toLocaleTimeString()}
        </p>
      </Section>

      <Section title="Quick Stats">
        <div className="grid grid-cols-2 gap-4">
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Players</div>
            <div className="text-xl font-bold">{players.length}/{status?.maxclients ?? '—'}</div>
          </div>
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Map</div>
            <div className="text-xl font-bold">{currentMap}</div>
          </div>
          <MapCountdown variant="stat" />
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Time Limit</div>
            <div className="text-xl font-bold">{timelimit} min</div>
          </div>
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Frag Limit</div>
            <div className="text-xl font-bold">{fraglimit}</div>
          </div>
        </div>
      </Section>

      <Section title="Current Deathmatch Settings">
        <div className="text-sm text-gray-400 mb-2">
          View and modify settings on the{' '}
          <Link to="/deathmatch" className="text-blue-400 hover:text-blue-300">
            Deathmatch page
          </Link>
        </div>
        <div className="grid grid-cols-2 gap-4">
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">DM Flags</div>
            <div className="text-xl font-bold font-mono">{currentDmflags}</div>
          </div>
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Server Info</div>
            <div className="text-sm font-mono text-gray-300 break-all">
              {status?.map ? `map: ${status.map}` : 'Loading...'}
            </div>
          </div>
        </div>
      </Section>
    </div>
  );
}
