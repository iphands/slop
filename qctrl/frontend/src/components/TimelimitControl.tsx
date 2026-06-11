import { useState, type FormEvent } from 'react';
import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

export function TimelimitControl() {
  const [value, setValue] = useState(20);
  const { mutate: execute, isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (value >= 0 && value <= 999) {
      execute(`timelimit ${value}`);
    }
  };

  const setQuickValue = (m: number) => {
    setValue(m);
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div>
        <label className="block text-sm font-medium mb-2">Time Limit (minutes)</label>
        <input
          type="number"
          min={0}
          max={999}
          value={value}
          onChange={(e) => setValue(Number(e.target.value))}
          className="w-full p-2 bg-gray-800 border border-gray-700 rounded focus:outline-none focus:border-blue-500"
        />
      </div>
      <div className="flex flex-wrap gap-2">
        {[15, 30, 45, 60].map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => setQuickValue(m)}
            className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
          >
            {m} min
          </button>
        ))}
        <button
          type="button"
          onClick={() => setQuickValue(0)}
          className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
        >
          Unlimited
        </button>
      </div>
      <button
        type="submit"
        disabled={isPending || value < 0 || value > 999}
        className="w-full py-2 bg-blue-600 hover:bg-blue-700 rounded disabled:opacity-50"
      >
        {isPending ? 'Setting...' : 'Set Time Limit'}
      </button>
      {error && <div className="text-red-400 text-sm">Failed: {error.message}</div>}
    </form>
  );
}
