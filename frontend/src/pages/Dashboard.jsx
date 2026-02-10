import { useState, useEffect } from 'react'
import { getDashboard } from '../api'

const STATUS_COLORS = {
  up: '#00d4aa',
  down: '#ff4757',
  degraded: '#ffa502',
  unknown: '#747d8c',
  maintenance: '#3742fa',
};

const STATUS_EMOJI = {
  up: '🟢',
  down: '🔴',
  degraded: '🟡',
  unknown: '⚪',
  maintenance: '🔵',
};

function StatCard({ label, value, sub, color, icon }) {
  return (
    <div className="stat-card">
      {icon && <div className="stat-icon">{icon}</div>}
      <div className="stat-value" style={color ? { color } : {}}>
        {value}
      </div>
      <div className="stat-label">{label}</div>
      {sub && <div className="stat-sub">{sub}</div>}
    </div>
  );
}

function StatusBar({ counts }) {
  const total = counts.up + counts.down + counts.degraded + counts.unknown + counts.maintenance;
  if (total === 0) return null;
  const segments = ['up', 'down', 'degraded', 'maintenance', 'unknown'];
  return (
    <div className="status-bar-container">
      <div className="status-bar">
        {segments.map(s => {
          const pct = (counts[s] / total) * 100;
          if (pct === 0) return null;
          return (
            <div
              key={s}
              className="status-bar-segment"
              style={{ width: `${pct}%`, backgroundColor: STATUS_COLORS[s] }}
              title={`${s}: ${counts[s]} (${pct.toFixed(0)}%)`}
            />
          );
        })}
      </div>
      <div className="status-bar-legend">
        {segments.map(s => counts[s] > 0 && (
          <span key={s} className="legend-item">
            <span className="legend-dot" style={{ backgroundColor: STATUS_COLORS[s] }} />
            {s}: {counts[s]}
          </span>
        ))}
      </div>
    </div>
  );
}

function timeAgo(iso) {
  if (!iso) return '';
  const diff = Date.now() - new Date(iso + (iso.endsWith('Z') ? '' : 'Z')).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  return `${days}d ago`;
}

export default function Dashboard({ onNavigate }) {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  const loadData = () => {
    getDashboard()
      .then(d => { setData(d); setError(null); })
      .catch(e => setError(e.message))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    loadData();
    const interval = setInterval(loadData, 30000);
    return () => clearInterval(interval);
  }, []);

  if (loading && !data) {
    return (
      <div>
        <h2 style={{ marginBottom: 24 }}>📊 Dashboard</h2>
        <div className="dashboard-grid">
          {[1,2,3,4].map(i => <div key={i} className="stat-card skeleton-card"><div className="skeleton" style={{height:48,width:'60%',margin:'0 auto 8px'}}/><div className="skeleton" style={{height:16,width:'40%',margin:'0 auto'}}/></div>)}
        </div>
      </div>
    );
  }

  if (error && !data) {
    return <div className="error-box">Failed to load dashboard: {error}</div>;
  }

  if (!data) return null;

  const overallColor = data.status_counts.down > 0 ? '#ff4757'
    : data.status_counts.degraded > 0 ? '#ffa502'
    : '#00d4aa';

  const overallLabel = data.status_counts.down > 0 ? 'Outage Detected'
    : data.status_counts.degraded > 0 ? 'Degraded Performance'
    : data.total_monitors === 0 ? 'No Monitors'
    : 'All Systems Operational';

  return (
    <div className="dashboard">
      <h2 style={{ marginBottom: 8 }}>📊 Dashboard</h2>
      <div className="overall-banner" style={{ borderLeftColor: overallColor }}>
        <span className="overall-status" style={{ color: overallColor }}>{overallLabel}</span>
        {data.total_checks_24h > 0 && (
          <span className="overall-checks">{data.total_checks_24h.toLocaleString()} checks in 24h</span>
        )}
      </div>

      {/* Key Metrics */}
      <div className="dashboard-grid">
        <StatCard
          icon="📡"
          label="Monitors"
          value={data.total_monitors}
          sub={`${data.public_monitors} public · ${data.paused_monitors} paused`}
        />
        <StatCard
          icon="⬆️"
          label="Uptime (24h)"
          value={`${data.avg_uptime_24h.toFixed(2)}%`}
          color={data.avg_uptime_24h >= 99.9 ? '#00d4aa' : data.avg_uptime_24h >= 99 ? '#ffa502' : '#ff4757'}
          sub={`7d: ${data.avg_uptime_7d.toFixed(2)}%`}
        />
        <StatCard
          icon="⚡"
          label="Avg Response"
          value={data.avg_response_ms_24h != null ? `${Math.round(data.avg_response_ms_24h)}ms` : '—'}
          color={data.avg_response_ms_24h != null && data.avg_response_ms_24h > 1000 ? '#ffa502' : '#00d4aa'}
        />
        <StatCard
          icon="🚨"
          label="Active Incidents"
          value={data.active_incidents}
          color={data.active_incidents > 0 ? '#ff4757' : '#00d4aa'}
        />
      </div>

      {/* Status Distribution */}
      <div className="dashboard-section">
        <h3>Monitor Status</h3>
        <StatusBar counts={data.status_counts} />
      </div>

      {/* Two-column: Recent Incidents + Slowest Monitors */}
      <div className="dashboard-columns">
        <div className="dashboard-section">
          <h3>🔥 Recent Incidents</h3>
          {data.recent_incidents.length === 0 ? (
            <p className="empty-state">No incidents recorded</p>
          ) : (
            <div className="incident-list">
              {data.recent_incidents.map(inc => (
                <div key={inc.id} className="incident-row" onClick={() => onNavigate && onNavigate(`/monitor/${inc.monitor_id}`)}>
                  <div className="incident-row-header">
                    <span className={`incident-badge ${inc.resolved_at ? 'resolved' : 'active'}`}>
                      {inc.resolved_at ? '✅' : '🔴'}
                    </span>
                    <span className="incident-monitor">{inc.monitor_name}</span>
                    <span className="incident-time">{timeAgo(inc.started_at)}</span>
                  </div>
                  <div className="incident-cause">{inc.cause}</div>
                </div>
              ))}
            </div>
          )}
        </div>

        <div className="dashboard-section">
          <h3>🐌 Slowest Monitors (24h)</h3>
          {data.slowest_monitors.length === 0 ? (
            <p className="empty-state">No data yet</p>
          ) : (
            <div className="slow-list">
              {data.slowest_monitors.map((m, i) => (
                <div key={m.id} className="slow-row" onClick={() => onNavigate && onNavigate(`/monitor/${m.id}`)}>
                  <span className="slow-rank">#{i + 1}</span>
                  <span className="slow-name">
                    {STATUS_EMOJI[m.current_status] || '⚪'} {m.name}
                  </span>
                  <span className="slow-ms">{Math.round(m.avg_response_ms)}ms</span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
