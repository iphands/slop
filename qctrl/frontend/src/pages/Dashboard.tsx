import { useQuery } from '@tanstack/react-query';
import { getHealth, getStatus } from '../lib/api';

export function Dashboard() {
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

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Server Dashboard</h1>

      <section className="p-4 bg-gray-800 rounded-lg">
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
      </section>

      <section className="p-4 bg-gray-800 rounded-lg">
        <h2 className="text-lg font-semibold mb-4">Quick Stats</h2>
        <div className="grid grid-cols-2 gap-4">
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Players</div>
            <div className="text-xl font-bold">{players.length}/25</div>
          </div>
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Map</div>
            <div className="text-xl font-bold">q2dm1</div>
          </div>
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Time Limit</div>
            <div className="text-xl font-bold">20 min</div>
          </div>
          <div className="p-3 bg-gray-700 rounded">
            <div className="text-sm text-gray-400">Frag Limit</div>
            <div className="text-xl font-bold">25</div>
          </div>
        </div>
      </section>

      <section className="p-4 bg-gray-800 rounded-lg">
        <h2 className="text-lg font-semibold mb-4">Quick Actions</h2>
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
      </section>
    </div>
  );
}
