import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { DndContext, type DragEndEvent, closestCenter, PointerSensor, useSensor, useSensors } from '@dnd-kit/core';
import {
  arrayMove,
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { getRotationQueue, removeMapFromQueue, updateRotationQueue } from '../lib/api';

interface QueueListProps {
  onQueueChange?: () => void;
}

interface SortableMapItemProps {
  mapName: string;
  index: number;
  onRemove: (mapName: string) => void;
}

function SortableMapItem({ mapName, index, onRemove }: SortableMapItemProps) {
  const { attributes, listeners, setNodeRef, transform, transition } = useSortable({ id: mapName });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className="p-3 bg-gray-800 rounded border border-gray-700 hover:border-gray-600 flex items-center justify-between group"
    >
      <div className="flex items-center gap-3 flex-1">
        <button
          type="button"
          {...attributes}
          {...listeners}
          className="text-gray-500 hover:text-gray-300 cursor-grab active:cursor-grabbing p-1"
          title="Drag to reorder"
        >
          ⇄
        </button>
        <span className="text-xs font-mono text-gray-500 w-6">{index + 1}.</span>
        <span className="font-medium text-gray-200">{mapName}</span>
      </div>
      <button
        type="button"
        onClick={() => onRemove(mapName)}
        className="text-gray-400 hover:text-red-400 transition-colors px-2 py-1 rounded hover:bg-gray-700"
        title="Remove from queue"
      >
        ×
      </button>
    </div>
  );
}

export function QueueList({ onQueueChange }: QueueListProps) {
  const queryClient = useQueryClient();
  
  const { data: queueData, isLoading } = useQuery({
    queryKey: ['rotationQueue'],
    queryFn: getRotationQueue,
  });

  const removeMutation = useMutation({
    mutationFn: removeMapFromQueue,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rotationQueue'] });
      onQueueChange?.();
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({ maps, mode }: { maps: string[]; mode: 'Sequential' | 'Random' }) =>
      updateRotationQueue(maps, mode),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rotationQueue'] });
      onQueueChange?.();
    },
  });

  const handleRemove = async (mapName: string) => {
    try {
      await removeMutation.mutateAsync(mapName);
    } catch (error) {
      console.error('Failed to remove map from queue:', error);
    }
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;

    if (over && active.id !== over.id) {
      const maps = queueData?.maps || [];
      const oldIndex = maps.indexOf(active.id as string);
      const newIndex = maps.indexOf(over.id as string);

      if (oldIndex !== -1 && newIndex !== -1) {
        const newMaps = arrayMove(maps, oldIndex, newIndex);
        const mode = queueData?.mode || 'Sequential';
        
        updateMutation.mutate({ maps: newMaps, mode });
      }
    }
  };

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8,
      },
    })
  );

  if (isLoading) {
    return <div className="text-gray-400">Loading queue...</div>;
  }

  const maps = queueData?.maps || [];
  const mode = queueData?.mode || 'Sequential';

  if (maps.length === 0) {
    return (
      <div className="p-4 bg-gray-800 rounded border border-gray-700 text-center">
        <p className="text-gray-400">No maps in queue</p>
        <p className="text-xs text-gray-500 mt-1">Add maps to start the rotation</p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm font-medium text-gray-300">
          Queue ({maps.length} {maps.length === 1 ? 'map' : 'maps'})
        </span>
        <span className="text-xs px-2 py-1 bg-gray-700 rounded text-gray-400">
          {mode}
        </span>
      </div>
      
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragEnd={handleDragEnd}
      >
        <SortableContext items={maps} strategy={verticalListSortingStrategy}>
          <div className="space-y-1">
            {maps.map((mapName, index) => (
              <SortableMapItem
                key={mapName}
                mapName={mapName}
                index={index}
                onRemove={handleRemove}
              />
            ))}
          </div>
        </SortableContext>
      </DndContext>
    </div>
  );
}
