import { PlayerList } from '../components/PlayerList';

export function Players() {
  return (
    <div className="space-y-6">
      <section className="p-4 bg-gray-800 rounded-lg">
        <h2 className="text-lg font-semibold mb-4">Connected Players</h2>
        <PlayerList />
      </section>
    </div>
  );
}
