import { useState, type FormEvent } from 'react';
import { useChanges } from '../contexts/ChangesContext';

interface FraglimitControlProps {
  currentValue?: number;
}

export function FraglimitControl({ currentValue = 25 }: FraglimitControlProps) {
  const { queueChange, getPendingValue, isDirty } = useChanges();

  // Get current value from pending change or use provided currentValue or default
  const pendingValue = getPendingValue('fraglimit');
  const initialValue = typeof pendingValue === 'number' ? pendingValue : currentValue;
  const [value, setValue] = useState(initialValue);

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (value >= 0 && value <= 999) {
      queueChange({
        type: 'fraglimit',
        pendingValue: value,
        description: 'Frag limit',
      });
    }
  };

  const setQuickValue = (f: number) => {
    setValue(f);
    queueChange({
      type: 'fraglimit',
      pendingValue: f,
      description: 'Frag limit',
    });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div>
        <label className="block text-sm font-medium mb-2">
          Frag Limit
          {isDirty('fraglimit') && (
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
        disabled={value < 0 || value > 999}
        className="w-full py-2 bg-blue-600 hover:bg-blue-700 rounded disabled:opacity-50"
      >
        Set Frag Limit
      </button>
    </form>
  );
}
