import { useState, useEffect, useCallback } from 'react'
import StatusPage from './pages/StatusPage'
import MonitorDetail from './pages/MonitorDetail'
import CreateMonitor from './pages/CreateMonitor'
import BulkImport from './pages/BulkImport'

function parseRoute() {
  const hash = window.location.hash.slice(1) || '/';
  const params = new URLSearchParams(window.location.search);
  const key = params.get('key') || '';
  if (hash.startsWith('/monitor/')) {
    return { page: 'detail', id: hash.slice(9), key };
  }
  if (hash === '/new') return { page: 'create', key };
  if (hash === '/import') return { page: 'import', key };
  return { page: 'status', key };
}

function navigate(path) {
  window.location.hash = path;
}

export default function App() {
  const [route, setRoute] = useState(parseRoute);

  useEffect(() => {
    const onHash = () => setRoute(parseRoute());
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  return (
    <div>
      <header className="header">
        <div className="container header-inner">
          <div className="header-logo" onClick={() => navigate('/')}>
            <svg viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
              <circle cx="32" cy="32" r="30" fill="#1a1a2e" stroke="#00d4aa" strokeWidth="3"/>
              <circle cx="32" cy="28" r="8" fill="none" stroke="#00d4aa" strokeWidth="2.5"/>
              <line x1="37" y1="34" x2="46" y2="46" stroke="#00d4aa" strokeWidth="2.5" strokeLinecap="round"/>
              <circle cx="32" cy="28" r="2" fill="#00d4aa"/>
            </svg>
            Watchpost
          </div>
          <nav className="header-nav">
            <button
              className={`nav-btn ${route.page === 'status' ? 'active' : ''}`}
              onClick={() => navigate('/')}
            >
              Status
            </button>
            <button
              className={`nav-btn ${route.page === 'create' ? 'active' : ''}`}
              onClick={() => navigate('/new')}
            >
              + New Monitor
            </button>
            <button
              className={`nav-btn ${route.page === 'import' ? 'active' : ''}`}
              onClick={() => navigate('/import')}
            >
              📦 Bulk Import
            </button>
          </nav>
        </div>
      </header>

      <main className="container" style={{ paddingBottom: 40 }}>
        {route.page === 'status' && (
          <StatusPage onSelect={(id) => navigate(`/monitor/${id}`)} />
        )}
        {route.page === 'detail' && (
          <MonitorDetail
            id={route.id}
            manageKey={route.key}
            onBack={() => navigate('/')}
          />
        )}
        {route.page === 'create' && (
          <CreateMonitor
            onCreated={(id) => navigate(`/monitor/${id}`)}
            onCancel={() => navigate('/')}
          />
        )}
        {route.page === 'import' && (
          <BulkImport
            onDone={() => navigate('/')}
            onCancel={() => navigate('/')}
          />
        )}
      </main>
    </div>
  );
}
