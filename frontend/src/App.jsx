import { useState, useEffect, useCallback, useRef } from 'react'
import { IconDashboard, IconPackage, IconGlobe } from './Icons'
import Dashboard from './pages/Dashboard'
import StatusPage from './pages/StatusPage'
import MonitorDetail from './pages/MonitorDetail'
import CreateMonitor from './pages/CreateMonitor'
import BulkImport from './pages/BulkImport'
import Locations from './pages/Locations'

function parseRoute() {
  const hash = window.location.hash.slice(1) || '/';
  // Support key in hash query: #/monitor/<id>?key=<key>
  const qIdx = hash.indexOf('?');
  const path = qIdx >= 0 ? hash.slice(0, qIdx) : hash;
  const hashParams = qIdx >= 0 ? new URLSearchParams(hash.slice(qIdx + 1)) : new URLSearchParams();
  // Backward compat: also check main URL query params
  const mainParams = new URLSearchParams(window.location.search);
  const key = hashParams.get('key') || mainParams.get('key') || '';

  if (path.startsWith('/monitor/')) {
    return { page: 'detail', id: path.slice(9), key };
  }
  if (path === '/new') return { page: 'create', key };
  if (path === '/import') return { page: 'import', key };
  if (path === '/status') return { page: 'status', key };
  if (path === '/locations') return { page: 'locations', key };
  return { page: 'dashboard', key };
}

function navigate(path) {
  window.location.hash = path;
}

export default function App() {
  const [route, setRoute] = useState(parseRoute);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef(null);

  useEffect(() => {
    const onHash = () => {
      setRoute(parseRoute());
      setMenuOpen(false);
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  // Close menu when clicking outside
  useEffect(() => {
    if (!menuOpen) return;
    const onClick = (e) => {
      if (menuRef.current && !menuRef.current.contains(e.target)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener('click', onClick);
    return () => document.removeEventListener('click', onClick);
  }, [menuOpen]);

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
          <button
            className={`hamburger ${menuOpen ? 'open' : ''}`}
            onClick={(e) => { e.stopPropagation(); setMenuOpen(!menuOpen); }}
            aria-label="Toggle navigation"
          >
            <span /><span /><span />
          </button>
          <nav ref={menuRef} className={`header-nav ${menuOpen ? 'open' : ''}`}>
            <button
              className={`nav-btn ${route.page === 'dashboard' ? 'active' : ''}`}
              onClick={() => navigate('/')}
            >
              <IconDashboard size={14} /> Dashboard
            </button>
            <button
              className={`nav-btn ${route.page === 'status' ? 'active' : ''}`}
              onClick={() => navigate('/status')}
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
              <IconPackage size={14} /> Bulk Import
            </button>
            <button
              className={`nav-btn ${route.page === 'locations' ? 'active' : ''}`}
              onClick={() => navigate('/locations')}
            >
              <IconGlobe size={14} /> Locations
            </button>
          </nav>
        </div>
      </header>

      <main className="container" style={{ paddingBottom: 40 }}>
        {route.page === 'dashboard' && (
          <Dashboard onNavigate={(path) => navigate(path)} />
        )}
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
        {route.page === 'locations' && <Locations />}
      </main>
      <footer style={{ textAlign: 'center', padding: '12px 16px', fontSize: '0.7rem', color: '#475569' }}>
        Made for AI, by AI.{' '}
        <a href="https://github.com/Humans-Not-Required" target="_blank" rel="noopener noreferrer" style={{ color: '#6366f1', textDecoration: 'none' }}>Humans not required</a>.
      </footer>
    </div>
  );
}
