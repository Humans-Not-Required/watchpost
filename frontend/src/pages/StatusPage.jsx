import { useState, useEffect } from 'react'
import { getStatus } from '../api'

const STATUS_LABELS = {
  operational: '✅ All Systems Operational',
  degraded: '⚠️ Some Systems Degraded',
  major_outage: '🔴 Major Outage Detected',
  unknown: '⏳ Checking Status...',
};

function formatTime(ts) {
  if (!ts) return 'Never';
  const d = new Date(ts + 'Z');
  const now = new Date();
  const diff = (now - d) / 1000;
  if (diff < 60) return 'Just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return d.toLocaleDateString();
}

function formatUptime(pct) {
  return pct >= 99.99 ? '100%' : `${pct.toFixed(2)}%`;
}

function formatMs(ms) {
  if (ms == null) return '—';
  return `${Math.round(ms)}ms`;
}

export default function StatusPage({ onSelect }) {
  const [status, setStatus] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const data = await getStatus();
        if (mounted) {
          setStatus(data);
          setLoading(false);
        }
      } catch (err) {
        if (mounted) {
          setError(err.message);
          setLoading(false);
        }
      }
    };
    load();
    const interval = setInterval(load, 30000);
    return () => { mounted = false; clearInterval(interval); };
  }, []);

  if (loading) {
    return (
      <div>
        <div className="skeleton skeleton-banner" />
        {[1, 2, 3].map((i) => (
          <div key={i} className="skeleton-card">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
              <div className="skeleton skeleton-text medium" style={{ marginBottom: 0 }} />
              <div className="skeleton skeleton-badge" />
            </div>
            <div className="monitor-stats">
              {[1, 2, 3, 4].map((j) => (
                <div key={j} className="monitor-stat">
                  <div className="skeleton skeleton-text short" />
                  <div className="skeleton skeleton-stat" />
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    );
  }

  if (error) {
    return (
      <div className="empty-state">
        <h3>Failed to load status</h3>
        <p>{error}</p>
      </div>
    );
  }

  const { monitors, overall } = status;

  return (
    <div>
      <div className={`status-banner ${overall}`}>
        {STATUS_LABELS[overall] || overall}
      </div>

      {monitors.length === 0 ? (
        <div className="empty-state">
          <h3>No monitors yet</h3>
          <p>Create your first monitor to start tracking uptime.</p>
        </div>
      ) : (
        monitors.map((m) => (
          <div
            key={m.id}
            className="card card-clickable"
            onClick={() => onSelect(m.id)}
          >
            <div className="card-header">
              <span className="card-title">{m.name}</span>
              <span className={`badge ${m.current_status}`}>
                <span className="badge-dot" />
                {m.current_status}
              </span>
            </div>
            <div className="monitor-stats">
              <div className="monitor-stat">
                <span className="monitor-stat-label">Uptime (24h)</span>
                <span className="monitor-stat-value" style={{
                  color: m.uptime_24h >= 99.5 ? 'var(--success)' :
                         m.uptime_24h >= 95 ? 'var(--warning)' : 'var(--danger)'
                }}>
                  {formatUptime(m.uptime_24h)}
                </span>
              </div>
              <div className="monitor-stat">
                <span className="monitor-stat-label">Uptime (7d)</span>
                <span className="monitor-stat-value">
                  {formatUptime(m.uptime_7d)}
                </span>
              </div>
              <div className="monitor-stat">
                <span className="monitor-stat-label">Avg Response</span>
                <span className="monitor-stat-value">
                  {formatMs(m.avg_response_ms_24h)}
                </span>
              </div>
              <div className="monitor-stat">
                <span className="monitor-stat-label">Last Check</span>
                <span className="monitor-stat-value">
                  {formatTime(m.last_checked_at)}
                </span>
              </div>
              {m.active_incident && (
                <div className="monitor-stat">
                  <span className="monitor-stat-label">Incident</span>
                  <span className="monitor-stat-value" style={{ color: 'var(--danger)' }}>
                    🔴 Active
                  </span>
                </div>
              )}
            </div>
          </div>
        ))
      )}

      <div style={{ textAlign: 'center', marginTop: 32, color: 'var(--text-muted)', fontSize: '0.8rem' }}>
        Powered by <strong>Watchpost</strong> — Agent-native monitoring
      </div>
    </div>
  );
}
