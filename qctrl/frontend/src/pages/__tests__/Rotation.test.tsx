import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ChangesProvider } from '../../contexts/ChangesContext';
import { NotificationsProvider } from '../../hooks/useNotifications';
import { Rotation } from '../Rotation';

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <NotificationsProvider>
        <ChangesProvider>
          {children}
        </ChangesProvider>
      </NotificationsProvider>
    </QueryClientProvider>
  );
}

describe('Rotation Page', () => {
  it('renders without crashing', () => {
    render(<Rotation />, { wrapper: createWrapper() });
    
    expect(screen.getByText('Map Rotation')).toBeInTheDocument();
  });

  it('displays the Map Rotation heading', () => {
    render(<Rotation />, { wrapper: createWrapper() });
    
    const heading = screen.getByRole('heading', { level: 1 });
    expect(heading).toHaveTextContent('Map Rotation');
  });

  it('shows queue management section', () => {
    render(<Rotation />, { wrapper: createWrapper() });
    
    expect(screen.getByText('Queue Management')).toBeInTheDocument();
  });
});
