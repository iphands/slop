import { useState, useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Section } from '../components/Section';
import { QueueList } from '../components/QueueList';
import { AddMapDialog } from '../components/AddMapDialog';
import { RotationModeToggle } from '../components/RotationModeToggle';
import { useNotifications } from '../hooks/useNotifications';
import { getRotationQueue, toggleRotation } from '../lib/api';

/**
 * Queue-management UI for map rotation.
 *
 * The actual automatic rotation is driven by <RotationController />, which is
 * mounted at the app root and runs regardless of which page is open. This page
 * only manages what's in the queue and the rotation mode/enable state.
 */
export function Rotation() {
  const queryClient = useQueryClient();
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [currentMode, setCurrentMode] = useState<'Sequential' | 'Random'>('Sequential');
  const [rotationEnabled, setRotationEnabled] = useState(true);

  const { addNotification } = useNotifications();

  useEffect(() => {
    const loadQueue = async () => {
      try {
        const queue = await getRotationQueue();
        setCurrentMode(queue.mode);
        setRotationEnabled(queue.enabled);
      } catch {
        // Silently fail on initial load
      }
    };
    loadQueue();
  }, []);

  const handleQueueChange = async () => {
    try {
      await queryClient.invalidateQueries({ queryKey: ['rotationQueue'] });
      const queue = await getRotationQueue();
      setCurrentMode(queue.mode);
      setRotationEnabled(queue.enabled);
    } catch {
      // Silently fail
    }
  };

  const handleToggleRotation = async () => {
    try {
      const response = await toggleRotation();
      setRotationEnabled(response.enabled);
      addNotification(
        response.enabled ? 'success' : 'info',
        response.message
      );
    } catch (error) {
      addNotification('error', 'Failed to toggle rotation');
      console.error(error);
    }
  };

  return (
    <div className="space-y-6 relative">
      <h1 className="text-2xl font-bold">Map Rotation</h1>

      <Section title="Rotation Mode">
        <div className="flex items-center justify-between mb-4">
          <div className="flex-1">
            <RotationModeToggle
              currentMode={currentMode}
              onModeChange={(newMode) => setCurrentMode(newMode)}
            />
          </div>
          <div className="ml-4">
            <button
              type="button"
              onClick={handleToggleRotation}
              className={`px-4 py-2 rounded text-sm font-medium transition-colors ${
                rotationEnabled
                  ? 'bg-green-600 hover:bg-green-700 text-white'
                  : 'bg-red-600 hover:bg-red-700 text-white'
              }`}
            >
              {rotationEnabled ? 'Rotation ON' : 'Rotation OFF'}
            </button>
          </div>
        </div>
      </Section>

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
        addNotification={addNotification}
      />
    </div>
  );
}
