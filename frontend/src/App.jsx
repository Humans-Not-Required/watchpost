import { useState, useEffect, useRef } from 'react'
import { verifyAdmin } from './api'
import { IconDashboard, IconPackage, IconGlobe, IconSun, IconMoon, IconLock } from './Icons'
import Dashboard from './pages/Dashboard'
import StatusPage from './pages/StatusPage'
import MonitorDetail from './pages/MonitorDetail'
import CreateMonitor from './pages/CreateMonitor'
import BulkImport from './pages/BulkImport'
import Locations from './pages/Locations'
import StatusPages from './pages/StatusPages'

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
  if (path === '/dashboard') return { page: 'dashboard', key };
  if (path === '/locations') return { page: 'locations', key };
  if (path === '/pages/new') return { page: 'status-pages', subpage: 'create', key };
  if (path.startsWith('/pages/')) return { page: 'status-pages', subpage: 'view', slug: path.slice(7), key };
  if (path === '/pages') return { page: 'status-pages', subpage: 'list', key };
  // Default: status page (public-safe landing)
  return { page: 'status', key };
}

function navigate(path) {
  window.location.hash = path;
}

function getInitialTheme() {
  const stored = localStorage.getItem('watchpost-theme');
  if (stored === 'light' || stored === 'dark') return stored;
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

export default function App() {
  const [route, setRoute] = useState(parseRoute);
  const [menuOpen, setMenuOpen] = useState(false);
  const [theme, setTheme] = useState(getInitialTheme);
  const [adminKey, setAdminKey] = useState(() => localStorage.getItem('watchpost-admin-key') || '');
  const [isAdmin, setIsAdmin] = useState(false);
  const [adminChecked, setAdminChecked] = useState(false);
  const menuRef = useRef(null);

  // Verify admin key on mount and when it changes
  useEffect(() => {
    if (!adminKey) {
      setIsAdmin(false);
      setAdminChecked(true);
      return;
    }
    let mounted = true;
    verifyAdmin(adminKey).then(res => {
      if (mounted) {
        setIsAdmin(res.valid === true);
        setAdminChecked(true);
        if (res.valid) {
          localStorage.setItem('watchpost-admin-key', adminKey);
        } else {
          localStorage.removeItem('watchpost-admin-key');
        }
      }
    }).catch(() => {
      if (mounted) {
        setIsAdmin(false);
        setAdminChecked(true);
      }
    });
    return () => { mounted = false; };
  }, [adminKey]);

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem('watchpost-theme', theme);
  }, [theme]);

  const toggleTheme = () => setTheme(t => t === 'dark' ? 'light' : 'dark');

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

  const handleAdminLogin = (key) => {
    setAdminKey(key);
  };

  const handleAdminLogout = () => {
    setAdminKey('');
    setIsAdmin(false);
    localStorage.removeItem('watchpost-admin-key');
    navigate('/');
  };

  return (
    <div>
      <header className="header">
        <div className="container header-inner">
          <div className="header-logo" onClick={() => navigate('/')}>
            <svg viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
              <circle cx="32" cy="32" r="30" fill="var(--logo-fill)" stroke="var(--accent)" strokeWidth="3"/>
              <circle cx="32" cy="28" r="8" fill="none" stroke="var(--accent)" strokeWidth="2.5"/>
              <line x1="37" y1="34" x2="46" y2="46" stroke="var(--accent)" strokeWidth="2.5" strokeLinecap="round"/>
              <circle cx="32" cy="28" r="2" fill="var(--accent)"/>
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
              className={`nav-btn ${route.page === 'status' ? 'active' : ''}`}
              onClick={() => navigate('/')}
            >
              Status
            </button>
            {isAdmin && (
              <>
                <button
                  className={`nav-btn ${route.page === 'dashboard' ? 'active' : ''}`}
                  onClick={() => navigate('/dashboard')}
                >
                  <IconDashboard size={14} /> Dashboard
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
                <button
                  className={`nav-btn ${route.page === 'status-pages' ? 'active' : ''}`}
                  onClick={() => navigate('/pages')}
                >
                  Pages
                </button>
              </>
            )}
            {!isAdmin && adminChecked && (
              <button
                className={`nav-btn ${route.page === 'dashboard' ? 'active' : ''}`}
                onClick={() => navigate('/dashboard')}
              >
                <IconLock size={14} /> Admin
              </button>
            )}
            {isAdmin && (
              <button
                className="nav-btn"
                onClick={handleAdminLogout}
                title="Sign out of admin"
              >
                Sign Out
              </button>
            )}
          </nav>
          <button
            className="theme-toggle"
            onClick={toggleTheme}
            aria-label={`Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`}
            title={`Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`}
          >
            {theme === 'dark' ? <IconSun size={18} /> : <IconMoon size={18} />}
          </button>
        </div>
      </header>

      <main className="container" style={{ paddingBottom: 40 }}>
        {route.page === 'dashboard' && (
          <Dashboard
            onNavigate={(path) => navigate(path)}
            adminKey={adminKey}
            isAdmin={isAdmin}
            onAdminLogin={handleAdminLogin}
          />
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
          isAdmin ? (
            <CreateMonitor
              onCreated={(id) => navigate(`/monitor/${id}`)}
              onCancel={() => navigate('/')}
            />
          ) : (
            <AdminRequired onLogin={handleAdminLogin} />
          )
        )}
        {route.page === 'import' && (
          isAdmin ? (
            <BulkImport
              onDone={() => navigate('/')}
              onCancel={() => navigate('/')}
            />
          ) : (
            <AdminRequired onLogin={handleAdminLogin} />
          )
        )}
        {route.page === 'locations' && (
          isAdmin ? <Locations /> : <AdminRequired onLogin={handleAdminLogin} />
        )}
        {route.page === 'status-pages' && (
          isAdmin ? (
            <StatusPages route={route} onNavigate={(path) => navigate(path)} />
          ) : (
            <AdminRequired onLogin={handleAdminLogin} />
          )
        )}
      </main>
      <footer style={{ textAlign: 'center', padding: '12px 16px', fontSize: '0.7rem', color: 'var(--text-muted)' }}>
        Made for AI, by AI.{' '}
        <a href="https://github.com/Humans-Not-Required" target="_blank" rel="noopener noreferrer" style={{ color: 'var(--accent)', textDecoration: 'none' }}>Humans not required</a>.
      </footer>
    </div>
  );
}

function AdminRequired({ onLogin }) {
  const [key, setKey] = useState('');
  const [error, setError] = useState('');

  const handleSubmit = async (e) => {
    e.preventDefault();
    if (!key.trim()) return;
    try {
      const res = await verifyAdmin(key.trim());
      if (res.valid) {
        onLogin(key.trim());
      } else {
        setError('Invalid admin key');
      }
    } catch {
      setError('Failed to verify key');
    }
  };

  return (
    <div style={{ maxWidth: 400, margin: '80px auto', textAlign: 'center' }}>
      <IconLock size={48} style={{ color: 'var(--text-muted)', marginBottom: 16 }} />
      <h2 style={{ color: 'var(--text-primary)', marginBottom: 8 }}>Admin Access Required</h2>
      <p style={{ color: 'var(--text-muted)', marginBottom: 24 }}>
        Enter your admin key to access this page.
      </p>
      <form onSubmit={handleSubmit}>
        <input
          type="password"
          value={key}
          onChange={(e) => { setKey(e.target.value); setError(''); }}
          placeholder="Admin key"
          style={{
            width: '100%',
            padding: '10px 14px',
            fontSize: '1rem',
            background: 'var(--card-bg)',
            border: '1px solid var(--border)',
            borderRadius: 8,
            color: 'var(--text-primary)',
            marginBottom: 12,
            boxSizing: 'border-box',
          }}
        />
        {error && <p style={{ color: '#ff4757', fontSize: '0.85rem', marginBottom: 12 }}>{error}</p>}
        <button
          type="submit"
          style={{
            width: '100%',
            padding: '10px',
            background: 'var(--accent)',
            color: '#fff',
            border: 'none',
            borderRadius: 8,
            fontSize: '0.95rem',
            fontWeight: 600,
            cursor: 'pointer',
          }}
        >
          Unlock
        </button>
      </form>
    </div>
  );
}
