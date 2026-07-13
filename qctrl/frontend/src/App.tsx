import { Link, useLocation, Routes, Route } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ChangesProvider } from './contexts/ChangesContext';
import { ChangesQueueUI } from './components/ChangesQueueUI';
import { ServerStatusSync } from './components/ServerStatusSync';
import { NotificationsProvider } from './hooks/useNotifications';
import { NotificationContainer } from './components/NotificationContainer';
import { MapChangeNotifier } from './components/MapChangeNotifier';
import { Layout } from './components/Layout';
import { Deathmatch } from './pages/Deathmatch';
import { Maps } from './pages/Maps';
import { Players } from './pages/Players';
import { Logs } from './pages/Logs';
import { Dashboard } from './pages/Dashboard';
import { Rotation } from './pages/Rotation';
import { Settings } from './pages/Settings';
import { NotFound } from './pages/NotFound';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Keep polling in a background tab, so a tab you come back to shows the
      // current map and clock rather than whatever was true when you left.
      //
      // This used to be load-bearing for correctness, not just for display: the
      // rotation trigger rode on the ['status'] query, so a backgrounded tab
      // meant a rotation that was due simply never fired. Rotation is the
      // backend's now, and nothing here can stall it.
      refetchIntervalInBackground: true,
    },
  },
});

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
        <NotificationsProvider>
          <ServerStatusSync />
          {/* Announces map changes on any page. Rotation itself is the backend's
              job now — see components/MapChangeNotifier.tsx. */}
          <MapChangeNotifier />
          <Layout>
          <nav className="flex gap-4 mb-6 border-b border-gray-700 pb-4 flex-wrap">
            <NavLink to="/">Dashboard</NavLink>
            <NavLink to="/deathmatch">Deathmatch</NavLink>
            <NavLink to="/maps">Maps</NavLink>
            <NavLink to="/rotation">Rotation</NavLink>
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
              <Route path="/rotation" element={<Rotation />} />
              <Route path="/players" element={<Players />} />
              <Route path="/logs" element={<Logs />} />
              <Route path="/settings" element={<Settings />} />
              <Route path="*" element={<NotFound />} />
            </Routes>
          </div>
        </Layout>
        {/* Global notification stack — single instance for the whole app. */}
        <NotificationContainer />
        </NotificationsProvider>
      </ChangesProvider>
    </QueryClientProvider>
  );
}

export default App;
