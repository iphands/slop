import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Dashboard } from './pages/Dashboard';

// refetchIntervalInBackground defaults to false, so a hidden tab stops polling.
// Leave it that way.
const qc = new QueryClient({ defaultOptions: { queries: { retry: 2 } } });

export default function App() {
  return (
    <QueryClientProvider client={qc}>
      <Dashboard />
    </QueryClientProvider>
  );
}
