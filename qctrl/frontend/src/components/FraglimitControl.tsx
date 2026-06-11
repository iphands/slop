import { useState, type FormEvent } from 'react';
import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

export function FraglimitControl() {
  const [value, setValue] = useState(25);
  const { mutate: execute, isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (value >= 0 && value <= 999) {
      execute(`fraglimit ${value}`);
    }
  };

  const setQuickValue = (f: number) => {
    setValue(f);
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div>
        <label className="block text-sm font-medium mb-2">Frag Limit</label>
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
        {[10, 25, 50, 100].map((f) => (
          <button
            key={f}
            type="button"
            onClick={() => setQuickValue(f)}
            className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
          >
            {f} frags
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
        {isPending ? 'Setting...' : 'Set Frag Limit'}
      </button>
      {error && <div className="text-red-400 text-sm">Failed: {error.message}</div>}
    </form>
  );
}
