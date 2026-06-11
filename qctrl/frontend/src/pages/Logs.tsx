import { useState, useEffect, useRef } from 'react';
import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

interface LogEntry {
  id: number;
  timestamp: number;
  level: string;
  message: string;
}

export function Logs() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [paused, setPaused] = useState(false);
  const [command, setCommand] = useState('');
  const endRef = useRef<HTMLDivElement>(null);

  const { mutate: sendCommand, isPending } = useMutation({
    mutationFn: executeRcon,
  });

  useEffect(() => {
    const wsUrl = `ws://${window.location.host}/logs/ws`;
    const ws = new WebSocket(wsUrl);

    ws.onopen = () => {
      console.log('WebSocket connected');
    };

    ws.onmessage = (event) => {
      try {
        const entry: LogEntry = JSON.parse(event.data);
        setLogs((prev) => [...prev, entry].slice(-1000));
      } catch (e) {
        console.error('Failed to parse log entry:', e);
      }
    };

    ws.onerror = (error) => {
      console.error('WebSocket error:', error);
    };

    ws.onclose = () => {
      console.log('WebSocket disconnected');
    };

    return () => {
      ws.close();
    };
  }, []);

  useEffect(() => {
    if (!paused && endRef.current) {
      endRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [logs, paused]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (command.trim()) {
      sendCommand(command.trim());
      setCommand('');
    }
  };

  const filteredLogs = logs;

  return (
    <div className="flex flex-col h-screen">
      <div className="p-4 bg-gray-800 border-b border-gray-700">
        <div className="flex justify-between items-center mb-4">
          <h2 className="text-lg font-semibold">Log Stream</h2>
          <div className="flex gap-2">
            <span className="text-sm text-gray-400">{logs.length} entries</span>
            <button
              type="button"
              onClick={() => setPaused(!paused)}
              className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
            >
              {paused ? 'Resume' : 'Pause'}
            </button>
            <button
              type="button"
              onClick={() => setLogs([])}
              className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
            >
              Clear
            </button>
          </div>
        </div>
        <form onSubmit={handleSubmit} className="flex gap-2">
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
      </div>

      <div className="flex-1 overflow-y-auto font-mono text-sm p-4 bg-gray-900 text-green-400">
        {filteredLogs.length === 0 ? (
          <div className="text-gray-500">No logs yet. Send a command to start streaming.</div>
        ) : (
          filteredLogs.map((log) => (
            <div key={log.id} className="whitespace-pre-wrap py-1">
              <span className="text-gray-500">
                {new Date(log.timestamp * 1000).toLocaleTimeString()}
              </span>{' '}
              <span className={log.level === 'ERROR' ? 'text-red-400' : 'text-green-400'}>
                [{log.level}]
              </span>{' '}
              {log.message}
            </div>
          ))
        )}
        <div ref={endRef} />
      </div>
    </div>
  );
}
