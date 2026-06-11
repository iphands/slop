import { createContext, useReducer, type ReactNode } from 'react';
import type { Change } from './changes.types';

interface ChangesState {
  changes: Change[];
  currentServerState: {
    dmflags: number;
    timelimit: number;
    fraglimit: number;
    currentMap: string;
  };
}

type ChangesAction =
  | { type: 'SET_SERVER_STATE'; payload: ChangesState['currentServerState'] }
  | { type: 'QUEUE_CHANGE'; payload: Omit<Change, 'id'> }
  | { type: 'REMOVE_CHANGE'; payload: string }
  | { type: 'CLEAR_QUEUE' }
  | { type: 'APPLY_CHANGES' };

const changesReducer = (state: ChangesState, action: ChangesAction): ChangesState => {
  switch (action.type) {
    case 'SET_SERVER_STATE':
      return { ...state, currentServerState: action.payload };

    case 'QUEUE_CHANGE': {
      const { type, pendingValue, description } = action.payload;
      // Check if we already have a change for this type
      const existingIndex = state.changes.findIndex((c) => c.type === type);
      
      if (pendingValue === state.currentServerState[type as keyof typeof state.currentServerState]) {
        // Reverting to server state, remove the change
        if (existingIndex >= 0) {
          return { ...state, changes: state.changes.filter((_, i) => i !== existingIndex) };
        }
        return state;
      }

      if (existingIndex >= 0) {
        // Update existing change
        const updatedChanges = [...state.changes];
        updatedChanges[existingIndex] = {
          id: updatedChanges[existingIndex].id,
          type,
          pendingValue,
          description,
        };
        return { ...state, changes: updatedChanges };
      }

      // Add new change
      return {
        ...state,
        changes: [
          ...state.changes,
          {
            id: `${type}-${Date.now()}`,
            type,
            pendingValue,
            description,
          },
        ],
      };
    }

    case 'REMOVE_CHANGE':
      return { ...state, changes: state.changes.filter((c) => c.id !== action.payload) };

    case 'CLEAR_QUEUE':
      return { ...state, changes: [] };

    case 'APPLY_CHANGES':
      // After applying, clear queue
      return {
        ...state,
        changes: [],
      };

    default:
      return state;
  }
};

interface ChangesContextValue {
  state: ChangesState;
  queueChange: (change: Omit<Change, 'id'>) => void;
  removeChange: (id: string) => void;
  clearQueue: () => void;
  applyChanges: () => void;
  getPendingValue: (type: Change['type']) => number | string | undefined;
  getServerValue: (type: Change['type']) => number | string;
  isDirty: (type: Change['type']) => boolean;
  setServerState: (state: ChangesState['currentServerState']) => void;
}

const ChangesContext = createContext<ChangesContextValue | undefined>(undefined);

export { ChangesContext };

export function ChangesProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(changesReducer, {
    changes: [],
    currentServerState: {
      dmflags: 17424,
      timelimit: 20,
      fraglimit: 25,
      currentMap: 'q2dm1',
    },
  });

  const queueChange = (change: Omit<Change, 'id'>) => {
    dispatch({ type: 'QUEUE_CHANGE', payload: change });
  };

  const removeChange = (id: string) => {
    dispatch({ type: 'REMOVE_CHANGE', payload: id });
  };

  const clearQueue = () => {
    dispatch({ type: 'CLEAR_QUEUE' });
  };

  const applyChanges = () => {
    dispatch({ type: 'APPLY_CHANGES' });
  };

  const getPendingValue = (type: Change['type']) => {
    const change = state.changes.find((c) => c.type === type);
    return change?.pendingValue;
  };

  const getServerValue = (type: Change['type']) => {
    return state.currentServerState[type as keyof typeof state.currentServerState] as number | string;
  };

  const isDirty = (type: Change['type']) => {
    return state.changes.some((c) => c.type === type);
  };

  const setServerState = (newState: ChangesState['currentServerState']) => {
    dispatch({ type: 'SET_SERVER_STATE', payload: newState });
  };

  return (
    <ChangesContext.Provider
      value={{
        state,
        queueChange,
        removeChange,
        clearQueue,
        applyChanges,
        getPendingValue,
        getServerValue,
        isDirty,
        setServerState,
      }}
    >
      {children}
    </ChangesContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components
export { useChanges } from './useChanges';
