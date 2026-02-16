import { useState, useEffect } from 'react'
import { getDashboard, getUptimeHistory } from '../api'
import { IconDashboard, IconSignal, IconArrowUp, IconZap, IconAlertOctagon, IconTrendUp, IconFlame, IconClock, IconCheckCircle, IconAlertCircle, IconStatusDot } from '../Icons'

const STATUS_COLORS = {
  up: '#00d4aa',
  down: '#ff4757',
  degraded: '#ffa502',
  unknown: '#747d8c',
  maintenance: '#3742fa',
};

const STATUS_DOT_COLORS = {
  up: '#00d4aa',
  down: '#ff4757',
  degraded: '#ffa502',
  unknown: '#747d8c',
  maintenance: '#3742fa',
};

function fillMissingDays(data, days) {
  if (!data || data.length === 0) return [];
  // Build a map of existing data by date
  const dataMap = {};
  for (const d of data) dataMap[d.date] = d;
  // Generate all dates in the range
  const filled = [];
  const now = new Date();
  for (let i = days - 1; i >= 0; i--) {
    const d = new Date(now);
    d.setUTCDate(d.getUTCDate() - i);
    const key = d.toISOString().slice(0, 10);
    if (dataMap[key]) {
      filled.push(dataMap[key]);
    } else {
      filled.push({ date: key, uptime_pct: null, total_checks: 0, up_checks: 0, down_checks: 0, avg_response_ms: null });
    }
  }
  return filled;
}

function niceStep(range) {
  // Pick a step that gives ~4-8 ticks
  if (range <= 2) return 0.5;
  if (range <= 5) return 1;
  if (range <= 10) return 2;
  if (range <= 25) return 5;
  if (range <= 50) return 10;
  return 20;
}

function UptimeHistoryChart({ data, days }) {
  if (!data || data.length === 0) {
    return <p className="empty-state">No uptime history data yet</p>;
  }

  const filled = fillMissingDays(data, days || 30);
  // Points with actual data (for line/area — skip nulls)
  const hasData = filled.filter(d => d.uptime_pct !== null);

  if (hasData.length === 0) {
    return <p className="empty-state">No uptime history data yet</p>;
  }

  const W = 700, H = 200, PAD_L = 48, PAD_R = 12, PAD_T = 16, PAD_B = 36;
  const chartW = W - PAD_L - PAD_R;
  const chartH = H - PAD_T - PAD_B;

  // Y range: 0-100% always when data dips below 90%, otherwise zoom into 90-100%
  const minUp = Math.min(...hasData.map(d => d.uptime_pct));
  const yMin = minUp >= 90 ? 90 : 0;
  const yMax = 100;
  const yRange = yMax - yMin || 1;

  const toX = (i) => PAD_L + (i / Math.max(filled.length - 1, 1)) * chartW;
  const toY = (pct) => PAD_T + chartH - ((pct - yMin) / yRange) * chartH;

  // Build line/area path only through data points (skip null gaps)
  const segments = []; // array of arrays of {idx, d}
  let current = [];
  for (let i = 0; i < filled.length; i++) {
    if (filled[i].uptime_pct !== null) {
      current.push({ idx: i, d: filled[i] });
    } else {
      if (current.length > 0) { segments.push(current); current = []; }
    }
  }
  if (current.length > 0) segments.push(current);

  // Y-axis ticks — use nice step to keep ~4-8 labels
  const step = niceStep(yRange);
  const yTicks = [];
  const firstTick = Math.ceil(yMin / step) * step;
  for (let v = firstTick; v <= yMax; v += step) {
    yTicks.push(Math.round(v * 10) / 10);
  }
  if (yTicks.length > 0 && yTicks[yTicks.length - 1] !== yMax) yTicks.push(yMax);
  if (yTicks.length > 0 && yTicks[0] !== yMin && yMin === 0) yTicks.unshift(0);

  // X-axis labels (show ~5-7 dates)
  const labelInterval = Math.max(1, Math.floor(filled.length / 6));

  // Color gradient based on min uptime
  const lineColor = minUp >= 99.9 ? '#00d4aa' : minUp >= 99 ? '#ffa502' : '#ff4757';
  const fillColor = minUp >= 99.9 ? 'rgba(0,212,170,0.15)' : minUp >= 99 ? 'rgba(255,165,2,0.15)' : 'rgba(255,71,87,0.15)';

  const [tooltip, setTooltip] = useState(null);

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="uptime-history-chart" style={{ width: '100%', maxWidth: W }}>
      {/* Grid lines */}
      {yTicks.map(v => (
        <g key={v}>
          <line x1={PAD_L} y1={toY(v)} x2={W - PAD_R} y2={toY(v)}
            stroke="var(--chart-grid)" strokeWidth="1" />
          <text x={PAD_L - 6} y={toY(v) + 4} textAnchor="end"
            fill="var(--chart-text)" fontSize="10">
            {v % 1 === 0 ? `${v}%` : `${v.toFixed(1)}%`}
          </text>
        </g>
      ))}

      {/* No-data zone markers */}
      {filled.map((d, i) => {
        if (d.uptime_pct !== null) return null;
        const bw = Math.max(chartW / filled.length, 2);
        return (
          <rect key={`nodata-${i}`} x={toX(i) - bw/2} y={PAD_T} width={bw} height={chartH}
            fill="var(--chart-fill)" />
        );
      })}

      {/* Area fill + Line — per segment (skip gaps) */}
      {segments.map((seg, si) => {
        if (seg.length === 0) return null;
        const pts = seg.map(s => `${toX(s.idx).toFixed(1)},${toY(s.d.uptime_pct).toFixed(1)}`);
        const linePath = `M${pts.join('L')}`;
        const lastX = toX(seg[seg.length - 1].idx).toFixed(1);
        const firstX = toX(seg[0].idx).toFixed(1);
        const baseY = (PAD_T + chartH).toFixed(1);
        const areaPath = `${linePath}L${lastX},${baseY}L${firstX},${baseY}Z`;
        return (
          <g key={`seg-${si}`}>
            <path d={areaPath} fill={fillColor} />
            <path d={linePath} fill="none" stroke={lineColor} strokeWidth="2" strokeLinejoin="round" />
          </g>
        );
      })}

      {/* Data points + hover targets */}
      {filled.map((d, i) => {
        if (d.uptime_pct === null) return (
          <rect key={i} x={toX(i) - 10} y={PAD_T} width="20" height={chartH}
            fill="transparent"
            onMouseEnter={() => setTooltip({ i, d, x: toX(i), y: PAD_T + chartH / 2, noData: true })}
            onMouseLeave={() => setTooltip(null)} />
        );
        return (
          <g key={i}>
            <circle cx={toX(i)} cy={toY(d.uptime_pct)} r="3"
              fill={d.uptime_pct >= 99.9 ? '#00d4aa' : d.uptime_pct >= 99 ? '#ffa502' : '#ff4757'}
              stroke="var(--chart-bar-stroke)" strokeWidth="1" />
            <rect x={toX(i) - 10} y={PAD_T} width="20" height={chartH}
              fill="transparent"
              onMouseEnter={() => setTooltip({ i, d, x: toX(i), y: toY(d.uptime_pct) })}
              onMouseLeave={() => setTooltip(null)} />
          </g>
        );
      })}

      {/* X-axis labels */}
      {filled.map((d, i) => {
        if (i % labelInterval !== 0 && i !== filled.length - 1) return null;
        const label = d.date.slice(5); // MM-DD
        return (
          <text key={i} x={toX(i)} y={H - 6} textAnchor="middle"
            fill="var(--chart-text)" fontSize="10">
            {label}
          </text>
        );
      })}

      {/* Tooltip */}
      {tooltip && (
        <g>
          <line x1={tooltip.x} y1={PAD_T} x2={tooltip.x} y2={PAD_T + chartH}
            stroke="var(--chart-avg-stroke)" strokeWidth="1" strokeDasharray="3,3" />
          <rect x={Math.min(tooltip.x - 60, W - PAD_R - 120)} y={Math.max(tooltip.y - 42, PAD_T)} width="120" height="36"
            rx="4" fill="var(--chart-tooltip-bg)" stroke="var(--chart-tooltip-border)" />
          {tooltip.noData ? (
            <>
              <text x={Math.min(tooltip.x, W - PAD_R - 60)} y={Math.max(tooltip.y - 22, PAD_T + 20)} textAnchor="middle"
                fill="var(--chart-tooltip-text)" fontSize="11">
                No data
              </text>
              <text x={Math.min(tooltip.x, W - PAD_R - 60)} y={Math.max(tooltip.y - 9, PAD_T + 33)} textAnchor="middle"
                fill="var(--chart-text-dim)" fontSize="10">
                {tooltip.d.date}
              </text>
            </>
          ) : (
            <>
              <text x={Math.min(tooltip.x, W - PAD_R - 60)} y={Math.max(tooltip.y - 26, PAD_T + 16)} textAnchor="middle"
                fill="#fff" fontSize="11" fontWeight="bold">
                {tooltip.d.uptime_pct.toFixed(2)}% uptime
              </text>
              <text x={Math.min(tooltip.x, W - PAD_R - 60)} y={Math.max(tooltip.y - 13, PAD_T + 29)} textAnchor="middle"
                fill="var(--chart-text-strong)" fontSize="10">
                {tooltip.d.date} · {tooltip.d.total_checks} checks
              </text>
            </>
          )}
        </g>
      )}
    </svg>
  );
}

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
  const [history, setHistory] = useState(null);
  const [historyDays, setHistoryDays] = useState(30);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  const loadData = () => {
    getDashboard()
      .then(d => { setData(d); setError(null); })
      .catch(e => setError(e.message))
      .finally(() => setLoading(false));
  };

  const loadHistory = () => {
    getUptimeHistory(historyDays)
      .then(h => setHistory(h))
      .catch(() => {});
  };

  useEffect(() => {
    loadData();
    loadHistory();
    const interval = setInterval(loadData, 30000);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    loadHistory();
  }, [historyDays]);

  if (loading && !data) {
    return (
      <div>
        <h2 style={{ marginBottom: 24 }}><IconDashboard size={20} style={{ marginRight: 8 }} />Dashboard</h2>
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
      <h2 style={{ marginBottom: 8 }}><IconDashboard size={20} style={{ marginRight: 8 }} />Dashboard</h2>
      <div className="overall-banner" style={{ borderLeftColor: overallColor }}>
        <span className="overall-status" style={{ color: overallColor }}>{overallLabel}</span>
        {data.total_checks_24h > 0 && (
          <span className="overall-checks">{data.total_checks_24h.toLocaleString()} checks in 24h</span>
        )}
      </div>

      {/* Key Metrics */}
      <div className="dashboard-grid">
        <StatCard
          icon={<IconSignal size={20} />}
          label="Monitors"
          value={data.total_monitors}
          sub={`${data.public_monitors} public · ${data.paused_monitors} paused`}
        />
        <StatCard
          icon={<IconArrowUp size={20} />}
          label="Uptime (24h)"
          value={`${data.avg_uptime_24h.toFixed(2)}%`}
          color={data.avg_uptime_24h >= 99.9 ? '#00d4aa' : data.avg_uptime_24h >= 99 ? '#ffa502' : '#ff4757'}
          sub={`7d: ${data.avg_uptime_7d.toFixed(2)}%`}
        />
        <StatCard
          icon={<IconZap size={20} />}
          label="Avg Response"
          value={data.avg_response_ms_24h != null ? `${Math.round(data.avg_response_ms_24h)}ms` : '—'}
          color={data.avg_response_ms_24h != null && data.avg_response_ms_24h > 1000 ? '#ffa502' : '#00d4aa'}
        />
        <StatCard
          icon={<IconAlertOctagon size={20} />}
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

      {/* Uptime History Chart */}
      <div className="dashboard-section">
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
          <h3><IconTrendUp size={16} style={{ marginRight: 6 }} />Uptime History</h3>
          <div className="history-range-selector">
            {[7, 14, 30, 90].map(d => (
              <button key={d}
                className={`range-btn ${historyDays === d ? 'active' : ''}`}
                onClick={() => setHistoryDays(d)}>
                {d}d
              </button>
            ))}
          </div>
        </div>
        <UptimeHistoryChart data={history} days={historyDays} />
      </div>

      {/* Two-column: Recent Incidents + Slowest Monitors */}
      <div className="dashboard-columns">
        <div className="dashboard-section">
          <h3><IconFlame size={16} style={{ marginRight: 6 }} />Recent Incidents</h3>
          {data.recent_incidents.length === 0 ? (
            <p className="empty-state">No incidents recorded</p>
          ) : (
            <div className="incident-list">
              {data.recent_incidents.map(inc => (
                <div key={inc.id} className="incident-row" onClick={() => onNavigate && onNavigate(`/monitor/${inc.monitor_id}`)}>
                  <div className="incident-row-header">
                    <span className={`incident-badge ${inc.resolved_at ? 'resolved' : 'active'}`}>
                      {inc.resolved_at ? <IconCheckCircle size={14} style={{ color: '#00d4aa' }} /> : <IconAlertCircle size={14} style={{ color: '#ff4757' }} />}
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
          <h3><IconClock size={16} style={{ marginRight: 6 }} />Slowest Monitors (24h)</h3>
          {data.slowest_monitors.length === 0 ? (
            <p className="empty-state">No data yet</p>
          ) : (
            <div className="slow-list">
              {data.slowest_monitors.map((m, i) => (
                <div key={m.id} className="slow-row" onClick={() => onNavigate && onNavigate(`/monitor/${m.id}`)}>
                  <span className="slow-rank">#{i + 1}</span>
                  <span className="slow-name">
                    <IconStatusDot color={STATUS_DOT_COLORS[m.current_status] || '#747d8c'} size={8} style={{ marginRight: 6 }} />{m.name}
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
