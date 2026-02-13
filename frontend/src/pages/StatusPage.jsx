import { useState, useEffect } from 'react'
import { getStatus, getTags } from '../api'
import { IconCheckCircle, IconAlertTriangle, IconAlertCircle, IconClock, IconStatusDot } from '../Icons'

const STATUS_LABELS = {
  operational: <><IconCheckCircle size={16} style={{ marginRight: 6 }} />All Systems Operational</>,
  degraded: <><IconAlertTriangle size={16} style={{ marginRight: 6 }} />Some Systems Degraded</>,
  major_outage: <><IconAlertCircle size={16} style={{ marginRight: 6 }} />Major Outage Detected</>,
  unknown: <><IconClock size={16} style={{ marginRight: 6 }} />Checking Status...</>,
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

const STATUS_FILTERS = [
  { value: null, label: 'All' },
  { value: 'up', label: <><IconStatusDot color="#00d4aa" size={8} style={{ marginRight: 4 }} />Up</> },
  { value: 'down', label: <><IconStatusDot color="#ff4757" size={8} style={{ marginRight: 4 }} />Down</> },
  { value: 'degraded', label: <><IconStatusDot color="#ffa502" size={8} style={{ marginRight: 4 }} />Degraded</> },
  { value: 'unknown', label: <><IconStatusDot color="#747d8c" size={8} style={{ marginRight: 4 }} />Unknown</> },
];

export default function StatusPage({ onSelect }) {
  const [status, setStatus] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [statusFilter, setStatusFilter] = useState(null);
  const [allTags, setAllTags] = useState([]);
  const [tagFilter, setTagFilter] = useState(null);

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

  useEffect(() => {
    getTags().then(setAllTags).catch(() => {});
  }, [status]);

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

  const filtered = monitors.filter((m) => {
    if (statusFilter && m.current_status !== statusFilter) return false;
    if (tagFilter && !(m.tags || []).includes(tagFilter)) return false;
    if (searchQuery.trim()) {
      const q = searchQuery.trim().toLowerCase();
      if (!m.name.toLowerCase().includes(q) && !m.url.toLowerCase().includes(q)) return false;
    }
    return true;
  });

  return (
    <div>
      <div className={`status-banner ${overall}`}>
        {STATUS_LABELS[overall] || overall}
      </div>

      {monitors.length > 0 && (
        <div className="filter-bar">
          <input
            type="text"
            className="search-input"
            placeholder="Search monitors by name or URL..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
          <div className="status-chips">
            {STATUS_FILTERS.map((f) => (
              <button
                key={f.label}
                className={`chip ${statusFilter === f.value ? 'chip-active' : ''}`}
                onClick={() => setStatusFilter(statusFilter === f.value ? null : f.value)}
              >
                {f.label}
                {f.value && (
                  <span className="chip-count">
                    {monitors.filter((m) => m.current_status === f.value).length}
                  </span>
                )}
              </button>
            ))}
          </div>
          {allTags.length > 0 && (
            <div className="status-chips" style={{ marginTop: 8 }}>
              <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginRight: 4 }}>Tags:</span>
              {allTags.map((t) => (
                <button
                  key={t}
                  className={`chip chip-tag ${tagFilter === t ? 'chip-active' : ''}`}
                  onClick={() => setTagFilter(tagFilter === t ? null : t)}
                >
                  {t}
                  <span className="chip-count">
                    {monitors.filter((m) => (m.tags || []).includes(t)).length}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      {monitors.length === 0 ? (
        <div className="empty-state">
          <h3>No monitors yet</h3>
          <p>Create your first monitor to start tracking uptime.</p>
        </div>
      ) : filtered.length === 0 ? (
        <div className="empty-state">
          <h3>No matches</h3>
          <p>No monitors match your search{statusFilter ? ` and "${statusFilter}" filter` : ''}.</p>
        </div>
      ) : (
        filtered.map((m) => (
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
            {(m.tags || []).length > 0 && (
              <div className="tag-list">
                {m.tags.map((t) => (
                  <span key={t} className="tag-badge" onClick={(e) => { e.stopPropagation(); setTagFilter(tagFilter === t ? null : t); }}>
                    {t}
                  </span>
                ))}
              </div>
            )}
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
                    <IconAlertCircle size={12} style={{ color: 'var(--danger)' }} /> Active
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
