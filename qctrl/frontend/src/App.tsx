import { useState } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ChangesProvider } from './contexts/ChangesContext';
import { ChangesQueueUI } from './components/ChangesQueueUI';
import { Layout } from './components/Layout';
import { Deathmatch } from './pages/Deathmatch';
import { Maps } from './pages/Maps';
import { Players } from './pages/Players';
import { Logs } from './pages/Logs';
import { Dashboard } from './pages/Dashboard';
import { Settings } from './pages/Settings';

const queryClient = new QueryClient();

type Page = 'dashboard' | 'deathmatch' | 'maps' | 'players' | 'logs' | 'settings';

function App() {
  const [currentPage, setCurrentPage] = useState<Page>('dashboard');

  return (
    <QueryClientProvider client={queryClient}>
      <ChangesProvider>
        <Layout>
          <nav className="flex gap-4 mb-6 border-b border-gray-700 pb-4 flex-wrap">
            <button
              onClick={() => setCurrentPage('dashboard')}
              className={`px-4 py-2 rounded ${
                currentPage === 'dashboard'
                  ? 'bg-blue-600'
                  : 'bg-gray-700 hover:bg-gray-600'
              }`}
            >
              Dashboard
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
            <button
              onClick={() => setCurrentPage('settings')}
              className={`px-4 py-2 rounded ${
                currentPage === 'settings'
                  ? 'bg-blue-600'
                  : 'bg-gray-700 hover:bg-gray-600'
              }`}
            >
              Settings
            </button>
          </nav>

          <div className="space-y-6">
            {currentPage === 'dashboard' && <Dashboard />}
            {currentPage === 'deathmatch' && <Deathmatch />}
            {currentPage === 'maps' && <Maps />}
            {currentPage === 'players' && <Players />}
            {currentPage === 'logs' && <Logs />}
            {currentPage === 'settings' && <Settings />}
          </div>

          <ChangesQueueUI />
        </Layout>
      </ChangesProvider>
    </QueryClientProvider>
  );
}

export default App;
