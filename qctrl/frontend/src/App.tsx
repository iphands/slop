import { Link, useLocation, Routes, Route } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ChangesProvider } from './contexts/ChangesContext';
import { ChangesQueueUI } from './components/ChangesQueueUI';
import { ServerStatusSync } from './components/ServerStatusSync';
import { Layout } from './components/Layout';
import { Deathmatch } from './pages/Deathmatch';
import { Maps } from './pages/Maps';
import { Players } from './pages/Players';
import { Logs } from './pages/Logs';
import { Dashboard } from './pages/Dashboard';
import { Settings } from './pages/Settings';

const queryClient = new QueryClient();

function NavLink({ to, children }: { to: string; children: React.ReactNode }) {
  const location = useLocation();
  const isActive = location.pathname === to;
  
  return (
    <Link
      to={to}
      className={`px-4 py-2 rounded transition-colors ${
        isActive
          ? 'bg-blue-600'
          : 'bg-gray-700 hover:bg-gray-600'
      }`}
    >
      {children}
    </Link>
  );
}

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ChangesProvider>
        <ServerStatusSync />
        <Layout>
          <nav className="flex gap-4 mb-6 border-b border-gray-700 pb-4 flex-wrap">
            <NavLink to="/">Dashboard</NavLink>
            <NavLink to="/deathmatch">Deathmatch</NavLink>
            <NavLink to="/maps">Maps</NavLink>
            <NavLink to="/players">Players</NavLink>
            <NavLink to="/logs">Logs</NavLink>
            <NavLink to="/settings">Settings</NavLink>
          </nav>

          <ChangesQueueUI />

          <div className="space-y-6">
            <Routes>
              <Route path="/" element={<Dashboard />} />
              <Route path="/deathmatch" element={<Deathmatch />} />
              <Route path="/maps" element={<Maps />} />
              <Route path="/players" element={<Players />} />
              <Route path="/logs" element={<Logs />} />
              <Route path="/settings" element={<Settings />} />
            </Routes>
          </div>
        </Layout>
      </ChangesProvider>
    </QueryClientProvider>
  );
}

export default App;
