import { useState, type FormEvent } from 'react';
import { useQuery, useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

export function Logs() {
  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: async () => {
      const res = await fetch('/status');
      return res.json();
    },
    refetchInterval: 2000,
  });

  const { mutate: sendCommand, isPending } = useMutation({
    mutationFn: executeRcon,
  });

  const [command, setCommand] = useState('');

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (command.trim()) {
      sendCommand(command.trim());
      setCommand('');
    }
  };

  return (
    <div className="space-y-6">
      <section className="p-4 bg-gray-800 rounded-lg">
        <h2 className="text-lg font-semibold mb-4">Command Console</h2>
        <form onSubmit={handleSubmit} className="flex gap-2 mb-4">
          <input
            type="text"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="Enter RCON command..."
            className="flex-1 p-2 bg-gray-700 border border-gray-600 rounded focus:outline-none focus:border-blue-500"
          />
          <button
            type="submit"
            disabled={isPending}
            className="px-4 py-2 bg-blue-600 hover:bg-blue-700 rounded disabled:opacity-50"
          >
            {isPending ? 'Sending...' : 'Send'}
          </button>
        </form>
        <div className="text-sm text-gray-400">
          Try: status, dmflags, timelimit, fraglimit, kick, ban
        </div>
      </section>

      <section className="p-4 bg-gray-800 rounded-lg">
        <h2 className="text-lg font-semibold mb-4">Server Status (Live)</h2>
        <pre className="bg-gray-900 p-4 rounded text-sm overflow-auto max-h-96">
          {status ? JSON.stringify(status, null, 2) : 'Loading...'}
        </pre>
      </section>
    </div>
  );
}
