import { useContext } from 'react';
import { ChangesContext } from './ChangesContext';

export function useChanges() {
  const context = useContext(ChangesContext);
  if (!context) {
    throw new Error('useChanges must be used within ChangesProvider');
  }
  return context;
}
