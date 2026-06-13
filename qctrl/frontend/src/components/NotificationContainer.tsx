interface Notification {
  id: string;
  type: 'info' | 'success' | 'error';
  message: string;
}

interface NotificationContainerProps {
  notifications: Notification[];
  onDismiss: (id: string) => void;
}

export function NotificationContainer({ notifications, onDismiss }: NotificationContainerProps) {
  if (notifications.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 space-y-2 z-50">
      {notifications.map((notification) => (
        <div
          key={notification.id}
          className={`
            flex items-center justify-between gap-4 px-4 py-3 rounded-lg shadow-lg min-w-64
            ${notification.type === 'info' ? 'bg-blue-600' : ''}
            ${notification.type === 'success' ? 'bg-green-600' : ''}
            ${notification.type === 'error' ? 'bg-red-600' : ''}
          `}
        >
          <span className="text-white text-sm">{notification.message}</span>
          <button
            type="button"
            onClick={() => onDismiss(notification.id)}
            className="text-white hover:text-gray-200 text-sm font-medium"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
