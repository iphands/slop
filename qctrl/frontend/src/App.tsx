import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Layout } from './components/Layout';
import { ServerStatus } from './components/ServerStatus';
import { MapList } from './components/MapList';

const queryClient = new QueryClient();

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Layout>
        <div className="space-y-6">
          <section className="p-4 bg-gray-800 rounded-lg">
            <h2 className="text-lg font-semibold mb-2">Server Status</h2>
            <ServerStatus />
          </section>

          <section className="p-4 bg-gray-800 rounded-lg">
            <h2 className="text-lg font-semibold mb-4">Available Maps</h2>
            <MapList />
          </section>
        </div>
      </Layout>
    </QueryClientProvider>
  );
}

export default App;
