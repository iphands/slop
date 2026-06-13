import { useState } from 'react';
import { Section } from '../components/Section';
import { QueueList } from '../components/QueueList';
import { AddMapDialog } from '../components/AddMapDialog';

export function Rotation() {
  const [showAddDialog, setShowAddDialog] = useState(false);

  const handleQueueChange = () => {};

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Map Rotation</h1>

      <Section title="Queue Management">
        <div className="flex justify-between items-center mb-4">
          <p className="text-gray-400 text-sm">
            Manage the map rotation queue. Maps will be played in order.
          </p>
          <button
            type="button"
            onClick={() => setShowAddDialog(true)}
            className="px-4 py-2 bg-orange-600 hover:bg-orange-700 rounded text-sm font-medium transition-colors"
          >
            Add Map
          </button>
        </div>

        <QueueList onQueueChange={handleQueueChange} />
      </Section>

      <AddMapDialog
        isOpen={showAddDialog}
        onClose={() => setShowAddDialog(false)}
        onMapAdded={handleQueueChange}
      />
    </div>
  );
}
