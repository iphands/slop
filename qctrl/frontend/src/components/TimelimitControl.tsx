import { useState, type FormEvent } from 'react';
import { useChanges } from '../contexts/ChangesContext';

interface TimelimitControlProps {
  currentValue?: number;
}

export function TimelimitControl({ currentValue = 20 }: TimelimitControlProps) {
  const { queueChange, getPendingValue, isDirty } = useChanges();

  // Get current value from pending change or use provided currentValue or default
  const pendingValue = getPendingValue('timelimit');
  const initialValue = typeof pendingValue === 'number' ? pendingValue : currentValue;
  const [value, setValue] = useState(initialValue);

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (value >= 0 && value <= 999) {
      queueChange({
        type: 'timelimit',
        pendingValue: value,
        description: 'Time limit',
      });
    }
  };

  const setQuickValue = (m: number) => {
    setValue(m);
    queueChange({
      type: 'timelimit',
      pendingValue: m,
      description: 'Time limit',
    });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div>
        <label className="block text-sm font-medium mb-2">
          Time Limit (minutes)
          {isDirty('timelimit') && (
            <span className="ml-2 text-xs text-orange-400">(queued)</span>
          )}
        </label>
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
        disabled={value < 0 || value > 999}
        className="w-full py-2 bg-blue-600 hover:bg-blue-700 rounded disabled:opacity-50"
      >
        Set Time Limit
      </button>
    </form>
  );
}
