import { Link } from 'react-router-dom';

export function NotFound() {
  return (
    <div className="space-y-6">
      <div className="text-center py-12">
        <div className="text-6xl font-bold text-red-500 mb-4">404</div>
        <h1 className="text-2xl font-bold text-white mb-2">Page Not Found</h1>
        <p className="text-gray-400 mb-8">
          The page you're looking for doesn't exist or has been moved.
        </p>
        
        <div className="flex flex-wrap justify-center gap-4">
          <Link
            to="/"
            className="px-6 py-2 bg-blue-600 hover:bg-blue-700 rounded font-medium transition-colors"
          >
            Go to Dashboard
          </Link>
          <Link
            to="/maps"
            className="px-6 py-2 bg-gray-700 hover:bg-gray-600 rounded font-medium transition-colors"
          >
            View Maps
          </Link>
          <Link
            to="/deathmatch"
            className="px-6 py-2 bg-gray-700 hover:bg-gray-600 rounded font-medium transition-colors"
          >
            Deathmatch Settings
          </Link>
        </div>
      </div>

      <div className="bg-gray-800 rounded p-4 border border-gray-700">
        <h3 className="text-lg font-semibold text-gray-300 mb-3">Available Pages:</h3>
        <div className="grid grid-cols-2 md:grid-cols-3 gap-2 text-sm">
          <Link
            to="/"
            className="text-blue-400 hover:text-blue-300 transition-colors"
          >
            → Dashboard
          </Link>
          <Link
            to="/deathmatch"
            className="text-blue-400 hover:text-blue-300 transition-colors"
          >
            → Deathmatch
          </Link>
          <Link
            to="/maps"
            className="text-blue-400 hover:text-blue-300 transition-colors"
          >
            → Maps
          </Link>
          <Link
            to="/players"
            className="text-blue-400 hover:text-blue-300 transition-colors"
          >
            → Players
          </Link>
          <Link
            to="/logs"
            className="text-blue-400 hover:text-blue-300 transition-colors"
          >
            → Logs
          </Link>
          <Link
            to="/settings"
            className="text-blue-400 hover:text-blue-300 transition-colors"
          >
            → Settings
          </Link>
        </div>
      </div>
    </div>
  );
}
