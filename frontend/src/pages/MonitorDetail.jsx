import { useState, useEffect } from 'react'
import { getMonitor, getHeartbeats, getUptime, getIncidents, pauseMonitor, resumeMonitor, deleteMonitor, acknowledgeIncident } from '../api'

function formatTime(ts) {
  if (!ts) return 'Never';
  const d = new Date(ts + 'Z');
  return d.toLocaleString();
}

function relativeTime(ts) {
  if (!ts) return 'Never';
  const d = new Date(ts + 'Z');
  const now = new Date();
  const diff = (now - d) / 1000;
  if (diff < 60) return 'Just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function formatUptime(pct) {
  return pct >= 99.99 ? '100%' : `${pct.toFixed(2)}%`;
}

function UptimeBar({ heartbeats }) {
  // Show last 60 heartbeats as colored segments
  const segments = heartbeats.slice(0, 60).reverse();
  if (segments.length === 0) {
    return <div style={{ color: 'var(--text-muted)', fontSize: '0.85rem' }}>No check data yet</div>;
  }

  return (
    <div className="uptime-bar-container">
      <div className="uptime-bar">
        {segments.map((hb, i) => (
          <div
            key={i}
            className={`uptime-bar-segment ${hb.status}`}
            title={`${hb.status} — ${hb.response_time_ms}ms — ${formatTime(hb.checked_at)}`}
          />
        ))}
      </div>
      <div className="uptime-bar-labels">
        <span>{segments.length > 0 ? relativeTime(segments[0].checked_at) : ''}</span>
        <span>Now</span>
      </div>
    </div>
  );
}

function UptimeStats({ stats }) {
  if (!stats) return null;

  const items = [
    { label: '24h', value: stats.uptime_24h, checks: stats.total_checks_24h },
    { label: '7d', value: stats.uptime_7d, checks: stats.total_checks_7d },
    { label: '30d', value: stats.uptime_30d, checks: stats.total_checks_30d },
    { label: '90d', value: stats.uptime_90d, checks: stats.total_checks_90d },
  ];

  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginTop: 16 }}>
      {items.map(({ label, value, checks }) => (
        <div key={label} className="card" style={{ textAlign: 'center', padding: 16, marginBottom: 0 }}>
          <div style={{
            fontSize: '1.5rem',
            fontWeight: 700,
            color: value >= 99.5 ? 'var(--success)' : value >= 95 ? 'var(--warning)' : 'var(--danger)',
          }}>
            {formatUptime(value)}
          </div>
          <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginTop: 4 }}>
            {label} ({checks} checks)
          </div>
        </div>
      ))}
    </div>
  );
}

function IncidentList({ incidents, manageKey, onAck }) {
  if (!incidents || incidents.length === 0) {
    return <div style={{ color: 'var(--text-muted)', padding: '16px 0' }}>No incidents recorded</div>;
  }

  return (
    <div>
      {incidents.map((inc) => (
        <IncidentCard key={inc.id} incident={inc} manageKey={manageKey} onAck={onAck} />
      ))}
    </div>
  );
}

function IncidentCard({ incident: inc, manageKey, onAck }) {
  const [ackNote, setAckNote] = useState('');
  const [acking, setAcking] = useState(false);
  const [showAckForm, setShowAckForm] = useState(false);

  const handleAck = async () => {
    if (!ackNote.trim()) return;
    setAcking(true);
    try {
      await acknowledgeIncident(inc.id, ackNote.trim(), 'via UI', manageKey);
      setShowAckForm(false);
      setAckNote('');
      onAck?.();
    } catch (err) {
      alert(`Failed to acknowledge: ${err.message}`);
    } finally {
      setAcking(false);
    }
  };

  return (
    <div className={`incident-card ${inc.resolved_at ? 'resolved' : 'active'}`}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'start' }}>
        <div>
          <div style={{ fontWeight: 600, fontSize: '0.9rem' }}>
            {inc.resolved_at ? '✅ Resolved' : '🔴 Active'}
          </div>
          <div style={{ fontSize: '0.85rem', color: 'var(--text-secondary)', marginTop: 4 }}>
            {inc.cause}
          </div>
        </div>
        <div style={{ textAlign: 'right', fontSize: '0.8rem', color: 'var(--text-muted)' }}>
          <div>Started: {relativeTime(inc.started_at)}</div>
          {inc.resolved_at && <div>Resolved: {relativeTime(inc.resolved_at)}</div>}
        </div>
      </div>
      {inc.acknowledgement && (
        <div style={{
          marginTop: 8,
          padding: '8px 12px',
          background: 'rgba(0,212,170,0.05)',
          borderRadius: 6,
          fontSize: '0.85rem',
          color: 'var(--text-secondary)',
        }}>
          <strong>Ack by {inc.acknowledged_by || 'unknown'}:</strong> {inc.acknowledgement}
        </div>
      )}
      {manageKey && !inc.acknowledgement && !inc.resolved_at && (
        <div style={{ marginTop: 8 }}>
          {!showAckForm ? (
            <button
              className="btn btn-secondary"
              style={{ fontSize: '0.8rem', padding: '6px 12px' }}
              onClick={() => setShowAckForm(true)}
            >
              Acknowledge
            </button>
          ) : (
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <input
                className="form-input"
                style={{ flex: 1, padding: '6px 10px', fontSize: '0.85rem' }}
                placeholder="Acknowledgement note..."
                value={ackNote}
                onChange={(e) => setAckNote(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleAck()}
                autoFocus
              />
              <button
                className="btn btn-primary"
                style={{ fontSize: '0.8rem', padding: '6px 12px' }}
                disabled={acking || !ackNote.trim()}
                onClick={handleAck}
              >
                {acking ? '...' : 'Send'}
              </button>
              <button
                className="btn btn-secondary"
                style={{ fontSize: '0.8rem', padding: '6px 12px' }}
                onClick={() => { setShowAckForm(false); setAckNote(''); }}
              >
                Cancel
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function HeartbeatTable({ heartbeats }) {
  if (!heartbeats || heartbeats.length === 0) return null;

  return (
    <div className="card" style={{ overflow: 'auto' }}>
      <table className="data-table">
        <thead>
          <tr>
            <th>Status</th>
            <th>Response</th>
            <th>HTTP Code</th>
            <th>Time</th>
            <th>Error</th>
          </tr>
        </thead>
        <tbody>
          {heartbeats.slice(0, 20).map((hb) => (
            <tr key={hb.id}>
              <td>
                <span className={`badge ${hb.status}`}>
                  <span className="badge-dot" />
                  {hb.status}
                </span>
              </td>
              <td>{hb.response_time_ms}ms</td>
              <td style={{ color: 'var(--text-secondary)' }}>{hb.status_code || '—'}</td>
              <td style={{ color: 'var(--text-muted)', fontSize: '0.8rem' }}>{relativeTime(hb.checked_at)}</td>
              <td style={{ color: 'var(--danger)', fontSize: '0.8rem', maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {hb.error_message || ''}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default function MonitorDetail({ id, manageKey, onBack }) {
  const [monitor, setMonitor] = useState(null);
  const [heartbeats, setHeartbeats] = useState([]);
  const [uptime, setUptime] = useState(null);
  const [incidents, setIncidents] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [tab, setTab] = useState('overview');
  const [actionLoading, setActionLoading] = useState(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const [m, hb, u, inc] = await Promise.all([
          getMonitor(id),
          getHeartbeats(id, 60),
          getUptime(id),
          getIncidents(id),
        ]);
        if (mounted) {
          setMonitor(m);
          setHeartbeats(hb);
          setUptime(u);
          setIncidents(inc);
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
  }, [id]);

  if (loading) {
    return <div className="loading"><div className="spinner" /> Loading monitor...</div>;
  }

  const reload = async () => {
    try {
      const [m, hb, u, inc] = await Promise.all([
        getMonitor(id),
        getHeartbeats(id, 60),
        getUptime(id),
        getIncidents(id),
      ]);
      setMonitor(m);
      setHeartbeats(hb);
      setUptime(u);
      setIncidents(inc);
    } catch (err) {
      // silent reload failure
    }
  };

  const handlePauseResume = async () => {
    const action = monitor.is_paused ? 'resume' : 'pause';
    setActionLoading(action);
    try {
      if (monitor.is_paused) {
        await resumeMonitor(id, manageKey);
      } else {
        await pauseMonitor(id, manageKey);
      }
      await reload();
    } catch (err) {
      alert(`Failed to ${action}: ${err.message}`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleDelete = async () => {
    setActionLoading('delete');
    try {
      await deleteMonitor(id, manageKey);
      onBack();
    } catch (err) {
      alert(`Failed to delete: ${err.message}`);
      setConfirmDelete(false);
    } finally {
      setActionLoading(null);
    }
  };

  if (error) {
    return (
      <div className="empty-state">
        <h3>Monitor not found</h3>
        <p>{error}</p>
        <button className="btn btn-secondary" style={{ marginTop: 16 }} onClick={onBack}>← Back</button>
      </div>
    );
  }

  return (
    <div>
      <button
        onClick={onBack}
        style={{
          background: 'none', border: 'none', color: 'var(--accent)',
          fontSize: '0.9rem', padding: '16px 0', cursor: 'pointer',
        }}
      >
        ← Back to Status
      </button>

      <div className="card" style={{ marginBottom: 20 }}>
        <div className="card-header">
          <div>
            <h2 className="card-title" style={{ fontSize: '1.3rem' }}>{monitor.name}</h2>
            <div style={{ fontSize: '0.85rem', color: 'var(--text-muted)', marginTop: 4 }}>
              {monitor.method} {monitor.url}
            </div>
          </div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            {monitor.is_paused && <span className="badge paused">⏸ Paused</span>}
            <span className={`badge ${monitor.current_status}`}>
              <span className="badge-dot" />
              {monitor.current_status}
            </span>
          </div>
        </div>

        <div className="monitor-stats">
          <div className="monitor-stat">
            <span className="monitor-stat-label">Interval</span>
            <span className="monitor-stat-value">{monitor.interval_seconds}s</span>
          </div>
          <div className="monitor-stat">
            <span className="monitor-stat-label">Timeout</span>
            <span className="monitor-stat-value">{monitor.timeout_ms}ms</span>
          </div>
          <div className="monitor-stat">
            <span className="monitor-stat-label">Expected</span>
            <span className="monitor-stat-value">HTTP {monitor.expected_status}</span>
          </div>
          <div className="monitor-stat">
            <span className="monitor-stat-label">Confirm</span>
            <span className="monitor-stat-value">{monitor.confirmation_threshold}x</span>
          </div>
          <div className="monitor-stat">
            <span className="monitor-stat-label">Last Check</span>
            <span className="monitor-stat-value">{relativeTime(monitor.last_checked_at)}</span>
          </div>
        </div>

        {manageKey && (
          <div className="manage-panel">
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginRight: 8 }}>🔑 Manage:</span>
              <button
                className="btn btn-secondary"
                style={{ fontSize: '0.8rem', padding: '6px 14px' }}
                disabled={actionLoading === 'pause' || actionLoading === 'resume'}
                onClick={handlePauseResume}
              >
                {actionLoading === 'pause' || actionLoading === 'resume'
                  ? '...'
                  : monitor.is_paused ? '▶ Resume' : '⏸ Pause'}
              </button>
              {!confirmDelete ? (
                <button
                  className="btn btn-danger"
                  style={{ fontSize: '0.8rem', padding: '6px 14px' }}
                  onClick={() => setConfirmDelete(true)}
                >
                  🗑 Delete
                </button>
              ) : (
                <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                  <span style={{ fontSize: '0.8rem', color: 'var(--danger)' }}>Delete this monitor?</span>
                  <button
                    className="btn btn-danger"
                    style={{ fontSize: '0.8rem', padding: '6px 12px' }}
                    disabled={actionLoading === 'delete'}
                    onClick={handleDelete}
                  >
                    {actionLoading === 'delete' ? '...' : 'Confirm'}
                  </button>
                  <button
                    className="btn btn-secondary"
                    style={{ fontSize: '0.8rem', padding: '6px 12px' }}
                    onClick={() => setConfirmDelete(false)}
                  >
                    Cancel
                  </button>
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      <UptimeBar heartbeats={heartbeats} />

      {/* Tabs */}
      <div style={{ display: 'flex', gap: 4, margin: '24px 0 16px', borderBottom: '1px solid var(--border)', paddingBottom: 8 }}>
        {['overview', 'heartbeats', 'incidents'].map((t) => (
          <button
            key={t}
            className={`nav-btn ${tab === t ? 'active' : ''}`}
            onClick={() => setTab(t)}
          >
            {t === 'overview' ? '📊 Overview' : t === 'heartbeats' ? '💓 Heartbeats' : `⚡ Incidents (${incidents.length})`}
          </button>
        ))}
      </div>

      {tab === 'overview' && (
        <div>
          <UptimeStats stats={uptime} />
          {uptime?.avg_response_ms_24h != null && (
            <div style={{ textAlign: 'center', marginTop: 12, color: 'var(--text-muted)', fontSize: '0.85rem' }}>
              Avg response time (24h): <strong style={{ color: 'var(--text-primary)' }}>{Math.round(uptime.avg_response_ms_24h)}ms</strong>
            </div>
          )}
        </div>
      )}

      {tab === 'heartbeats' && <HeartbeatTable heartbeats={heartbeats} />}

      {tab === 'incidents' && <IncidentList incidents={incidents} manageKey={manageKey} onAck={reload} />}
    </div>
  );
}
