import { useState, useEffect } from 'react';
import { Section } from '../components/Section';
import { QueueList } from '../components/QueueList';
import { AddMapDialog } from '../components/AddMapDialog';
import { RotationModeToggle } from '../components/RotationModeToggle';
import { useRotationTimer } from '../hooks/useRotationTimer';
import type { TimerTriggerEvent } from '../hooks/useRotationTimer';
import { useMapRotation } from '../hooks/useMapRotation';
import { useNotifications } from '../hooks/useNotifications';
import { NotificationContainer } from '../components/NotificationContainer';
import { getRotationQueue } from '../lib/api';

export function Rotation() {
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [queueMaps, setQueueMaps] = useState<string[]>([]);
  const [currentMode, setCurrentMode] = useState<'Sequential' | 'Random'>('Sequential');
  const [currentMap, setCurrentMap] = useState<string | null>(null);

  const { notifications, addNotification, removeNotification } = useNotifications();

  useEffect(() => {
    const loadQueue = async () => {
      try {
        const queue = await getRotationQueue();
        setQueueMaps(queue.maps);
        setCurrentMode(queue.mode);
        setCurrentMap(queue.current_map);
      } catch {
        // Silently fail on initial load
      }
    };
    loadQueue();
  }, []);

  const handleQueueChange = async () => {
    try {
      const queue = await getRotationQueue();
      setQueueMaps(queue.maps);
      setCurrentMode(queue.mode);
      setCurrentMap(queue.current_map);
    } catch {
      // Silently fail
    }
  };

  const handleTimerTrigger = async (event: TimerTriggerEvent) => {
    const triggerType = event.type === 'time_limit' ? 'Time limit' : 'Frag limit';
    addNotification('info', `${triggerType} reached - switching map...`);
  };

  const { handleTrigger: handleMapRotation, isSwitching, switchingTo } = useMapRotation({
    mode: currentMode,
    queueMaps,
    currentMap,
    onMapChangeStart: (mapName) => {
      addNotification('info', `Switching to ${mapName}...`);
    },
    onMapChangeComplete: (mapName) => {
      addNotification('success', `Successfully switched to ${mapName}`);
      handleQueueChange();
    },
    onMapChangeError: (error) => {
      addNotification('error', `Failed to switch map: ${error}`);
    },
    onResetTimer: () => {
      // Timer reset is handled by useRotationTimer detecting map change
    },
  });

  const onTimerTrigger = async (event: TimerTriggerEvent) => {
    await handleTimerTrigger(event);
    await handleMapRotation(event);
  };

  useRotationTimer({
    onTrigger: onTimerTrigger,
  });

  return (
    <div className="space-y-6 relative">
      <h1 className="text-2xl font-bold">Map Rotation</h1>

      {isSwitching && switchingTo && (
        <div className="bg-orange-600/20 border border-orange-500 rounded-lg p-4">
          <p className="text-orange-300">Switching to {switchingTo}...</p>
        </div>
      )}

      <Section title="Rotation Mode">
        <RotationModeToggle currentMode={currentMode} />
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
      />

      <NotificationContainer notifications={notifications} onDismiss={removeNotification} />
    </div>
  );
}
