import { useState } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Layout } from './components/Layout';
import { ServerStatus } from './components/ServerStatus';
import { MapList } from './components/MapList';
import { Deathmatch } from './pages/Deathmatch';
import { Maps } from './pages/Maps';
import { Players } from './pages/Players';
import { Logs } from './pages/Logs';

const queryClient = new QueryClient();

type Page = 'home' | 'deathmatch' | 'maps' | 'players' | 'logs';

function App() {
  const [currentPage, setCurrentPage] = useState<Page>('home');

  return (
    <QueryClientProvider client={queryClient}>
      <Layout>
        <nav className="flex gap-4 mb-6 border-b border-gray-700 pb-4 flex-wrap">
          <button
            onClick={() => setCurrentPage('home')}
            className={`px-4 py-2 rounded ${
              currentPage === 'home'
                ? 'bg-blue-600'
                : 'bg-gray-700 hover:bg-gray-600'
            }`}
          >
            Home
          </button>
          <button
            onClick={() => setCurrentPage('deathmatch')}
            className={`px-4 py-2 rounded ${
              currentPage === 'deathmatch'
                ? 'bg-blue-600'
                : 'bg-gray-700 hover:bg-gray-600'
            }`}
          >
            Deathmatch
          </button>
          <button
            onClick={() => setCurrentPage('maps')}
            className={`px-4 py-2 rounded ${
              currentPage === 'maps'
                ? 'bg-blue-600'
                : 'bg-gray-700 hover:bg-gray-600'
            }`}
          >
            Maps
          </button>
          <button
            onClick={() => setCurrentPage('players')}
            className={`px-4 py-2 rounded ${
              currentPage === 'players'
                ? 'bg-blue-600'
                : 'bg-gray-700 hover:bg-gray-600'
            }`}
          >
            Players
          </button>
          <button
            onClick={() => setCurrentPage('logs')}
            className={`px-4 py-2 rounded ${
              currentPage === 'logs'
                ? 'bg-blue-600'
                : 'bg-gray-700 hover:bg-gray-600'
            }`}
          >
            Logs
          </button>
        </nav>

        <div className="space-y-6">
          {currentPage === 'home' && (
            <>
              <section className="p-4 bg-gray-800 rounded-lg">
                <h2 className="text-lg font-semibold mb-2">Server Status</h2>
                <ServerStatus />
              </section>

              <section className="p-4 bg-gray-800 rounded-lg">
                <h2 className="text-lg font-semibold mb-4">Available Maps</h2>
                <MapList />
              </section>
            </>
          )}

          {currentPage === 'deathmatch' && <Deathmatch />}
          {currentPage === 'maps' && <Maps />}
          {currentPage === 'players' && <Players />}
          {currentPage === 'logs' && <Logs />}
        </div>
      </Layout>
    </QueryClientProvider>
  );
}

export default App;
