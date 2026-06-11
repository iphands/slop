export function CurrentMap() {
  const currentMap = 'q2dm1';

  return (
    <div className="p-4 bg-gray-800 rounded-lg">
      <h2 className="text-lg font-semibold mb-2">Current Map</h2>
      <p className="text-2xl font-bold">{currentMap}</p>
      <p className="text-sm text-gray-400 mt-1">
        (Refresh available after status endpoint added)
      </p>
    </div>
  );
}
