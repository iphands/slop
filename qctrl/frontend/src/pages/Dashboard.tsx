import { useQuery } from '@tanstack/react-query';
import { getHealth, getStatus } from '../lib/api';
import { useChanges } from '../contexts/ChangesContext';
import { Section } from '../components/Section';

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
            <div className="text-xl font-bold">{players.length}/25</div>
          </div>
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Map</div>
            <div className="text-xl font-bold">{currentMap}</div>
          </div>
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

      <Section title="Quick Actions">
        <div className="grid grid-cols-2 gap-4">
          <button
            type="button"
            onClick={() => (window.location.href = '/?page=maps')}
            className="p-4 bg-blue-600 hover:bg-blue-700 rounded text-center"
          >
            <div className="text-lg font-medium">Change Map</div>
          </button>
          <button
            type="button"
            onClick={() => (window.location.href = '/?page=deathmatch')}
            className="p-4 bg-orange-600 hover:bg-orange-700 rounded text-center"
          >
            <div className="text-lg font-medium">Restart Map</div>
          </button>
          <button
            type="button"
            onClick={() => (window.location.href = '/?page=players')}
            className="p-4 bg-green-600 hover:bg-green-700 rounded text-center"
          >
            <div className="text-lg font-medium">Players</div>
          </button>
          <button
            type="button"
            onClick={() => (window.location.href = '/?page=logs')}
            className="p-4 bg-purple-600 hover:bg-purple-700 rounded text-center"
          >
            <div className="text-lg font-medium">Logs</div>
          </button>
        </div>
      </Section>
    </div>
  );
}
