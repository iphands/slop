export interface Change {
  id: string;
  type: 'dmflags' | 'timelimit' | 'fraglimit' | 'map';
  pendingValue: number | string;
  description: string;
}

export interface ChangesContextValue {
  state: {
    changes: Change[];
    currentServerState: {
      dmflags: number;
      timelimit: number;
      fraglimit: number;
      currentMap: string;
    };
  };
  queueChange: (change: Omit<Change, 'id'>) => void;
  removeChange: (id: string) => void;
  clearQueue: () => void;
  applyChanges: () => void;
  getPendingValue: (type: Change['type']) => number | string | undefined;
  getServerValue: (type: Change['type']) => number | string;
  isDirty: (type: Change['type']) => boolean;
  setServerState: (state: ChangesContextValue['state']['currentServerState']) => void;
}
