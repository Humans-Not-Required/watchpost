import { useState, useEffect } from 'react'
import { getMonitor, getHeartbeats, getUptime, getIncidents, getMonitorSla, pauseMonitor, resumeMonitor, deleteMonitor, updateMonitor, acknowledgeIncident, getNotifications, createNotification, deleteNotification, updateNotification, getMaintenanceWindows, createMaintenanceWindow, deleteMaintenanceWindow } from '../api'
import { IconTrendUp, IconCheckCircle, IconAlertCircle, IconBell, IconMail, IconPause, IconPlay, IconX, IconWrench, IconCalendar, IconKey, IconEdit, IconSave, IconLink, IconLock, IconGlobe, IconClipboard, IconTrash, IconDashboard, IconHeart, IconZap, IconTag } from '../Icons'

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

function ResponseTimeChart({ heartbeats }) {
  // SVG line chart of response times - no external dependencies
  const data = heartbeats
    .filter(hb => hb.response_time_ms != null && hb.response_time_ms > 0)
    .slice(0, 100)
    .reverse(); // oldest first for left-to-right

  if (data.length < 2) {
    return <div style={{ color: 'var(--text-muted)', fontSize: '0.85rem', padding: '16px 0' }}>Not enough data for response time chart</div>;
  }

  const W = 800, H = 200;
  const PAD = { top: 20, right: 20, bottom: 40, left: 55 };
  const plotW = W - PAD.left - PAD.right;
  const plotH = H - PAD.top - PAD.bottom;

  const times = data.map(hb => hb.response_time_ms);
  const minT = 0;
  const maxT = Math.max(...times) * 1.15; // 15% headroom
  const dates = data.map(hb => new Date(hb.checked_at + 'Z').getTime());
  const minD = dates[0];
  const maxD = dates[dates.length - 1];
  const rangeD = maxD - minD || 1;

  const x = (i) => PAD.left + (dates[i] - minD) / rangeD * plotW;
  const y = (ms) => PAD.top + plotH - (ms / maxT) * plotH;

  // Build SVG path
  const pathD = data.map((hb, i) => `${i === 0 ? 'M' : 'L'}${x(i).toFixed(1)},${y(hb.response_time_ms).toFixed(1)}`).join(' ');

  // Area fill path (path + close to bottom)
  const areaD = pathD + ` L${x(data.length - 1).toFixed(1)},${(PAD.top + plotH).toFixed(1)} L${x(0).toFixed(1)},${(PAD.top + plotH).toFixed(1)} Z`;

  // Y-axis ticks (4-5 ticks)
  const yTicks = [];
  const niceStep = niceNum(maxT / 4);
  for (let v = 0; v <= maxT; v += niceStep) {
    if (v > maxT) break;
    yTicks.push(v);
  }

  // X-axis labels (3-5 labels)
  const xLabels = [];
  const labelCount = Math.min(5, data.length);
  for (let i = 0; i < labelCount; i++) {
    const idx = Math.round(i * (data.length - 1) / (labelCount - 1));
    const d = new Date(data[idx].checked_at + 'Z');
    xLabels.push({ x: x(idx), label: d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) });
  }

  // Avg line
  const avg = times.reduce((a, b) => a + b, 0) / times.length;
  const avgY = y(avg);

  // Tooltip state is handled by CSS :hover on dots

  return (
    <div className="card" style={{ marginTop: 16, padding: '16px 12px' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <h4 style={{ fontSize: '0.9rem', fontWeight: 600 }}><IconTrendUp size={14} style={{ marginRight: 6 }} />Response Time</h4>
        <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)' }}>
          avg {Math.round(avg)}ms · last {data.length} checks
        </span>
      </div>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        style={{ width: '100%', height: 'auto', display: 'block' }}
      >
        {/* Grid lines */}
        {yTicks.map((v, i) => (
          <g key={i}>
            <line
              x1={PAD.left} y1={y(v)} x2={W - PAD.right} y2={y(v)}
              stroke="var(--border)" strokeWidth="1" strokeDasharray="4,4"
            />
            <text
              x={PAD.left - 8} y={y(v) + 4}
              textAnchor="end" fontSize="11" fill="var(--text-muted)"
            >
              {v >= 1000 ? `${(v / 1000).toFixed(1)}s` : `${Math.round(v)}ms`}
            </text>
          </g>
        ))}

        {/* X-axis labels */}
        {xLabels.map((lbl, i) => (
          <text
            key={i}
            x={lbl.x} y={H - 8}
            textAnchor="middle" fontSize="11" fill="var(--text-muted)"
          >
            {lbl.label}
          </text>
        ))}

        {/* Avg line */}
        <line
          x1={PAD.left} y1={avgY} x2={W - PAD.right} y2={avgY}
          stroke="var(--accent)" strokeWidth="1" strokeDasharray="6,4" opacity="0.5"
        />

        {/* Area fill */}
        <path d={areaD} fill="var(--accent)" opacity="0.08" />

        {/* Line */}
        <path d={pathD} fill="none" stroke="var(--accent)" strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" />

        {/* Data points */}
        {data.map((hb, i) => (
          <g key={i}>
            <circle
              cx={x(i)} cy={y(hb.response_time_ms)} r="3.5"
              fill={hb.status === 'up' ? 'var(--accent)' : 'var(--danger)'}
              stroke="var(--bg-card)" strokeWidth="1.5"
              style={{ cursor: 'pointer' }}
            >
              <title>{`${hb.response_time_ms}ms — ${new Date(hb.checked_at + 'Z').toLocaleString()}`}</title>
            </circle>
          </g>
        ))}
      </svg>
    </div>
  );
}

function niceNum(range) {
  // Find a "nice" step size for axis ticks
  const exp = Math.floor(Math.log10(range));
  const frac = range / Math.pow(10, exp);
  let nice;
  if (frac <= 1.5) nice = 1;
  else if (frac <= 3) nice = 2;
  else if (frac <= 7) nice = 5;
  else nice = 10;
  return nice * Math.pow(10, exp);
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
    <div className="uptime-stats-grid">
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
            {inc.resolved_at ? <><IconCheckCircle size={14} style={{ color: 'var(--success)' }} /> Resolved</> : <><IconAlertCircle size={14} style={{ color: 'var(--danger)' }} /> Active</>}
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

function NotificationManager({ monitorId, manageKey }) {
  const [channels, setChannels] = useState([]);
  const [loading, setLoading] = useState(true);
  const [showAdd, setShowAdd] = useState(false);
  const [newName, setNewName] = useState('');
  const [newType, setNewType] = useState('webhook');
  const [newUrl, setNewUrl] = useState('');
  const [adding, setAdding] = useState(false);
  const [deleting, setDeleting] = useState(null);
  const [toggling, setToggling] = useState(null);
  const [error, setError] = useState(null);

  const loadChannels = async () => {
    try {
      const data = await getNotifications(monitorId, manageKey);
      setChannels(data);
    } catch (err) {
      // silent
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { loadChannels(); }, [monitorId]);

  const handleAdd = async () => {
    if (!newName.trim() || !newUrl.trim()) {
      setError(newType === 'email' ? 'Name and email address are required' : 'Name and URL are required');
      return;
    }
    setAdding(true);
    setError(null);
    try {
      const config = newType === 'email'
        ? { address: newUrl.trim() }
        : { url: newUrl.trim() };
      await createNotification(monitorId, {
        name: newName.trim(),
        channel_type: newType,
        config,
      }, manageKey);
      setNewName('');
      setNewUrl('');
      setShowAdd(false);
      await loadChannels();
    } catch (err) {
      setError(err.message);
    } finally {
      setAdding(false);
    }
  };

  const handleToggle = async (channelId, currentEnabled) => {
    setToggling(channelId);
    try {
      await updateNotification(channelId, { is_enabled: !currentEnabled }, manageKey);
      await loadChannels();
    } catch (err) {
      alert(`Failed to toggle: ${err.message}`);
    } finally {
      setToggling(null);
    }
  };

  const handleDelete = async (id) => {
    setDeleting(id);
    try {
      await deleteNotification(id, manageKey);
      await loadChannels();
    } catch (err) {
      alert(`Failed to delete: ${err.message}`);
    } finally {
      setDeleting(null);
    }
  };

  if (loading) {
    return <div style={{ color: 'var(--text-muted)', padding: '16px 0' }}>Loading notifications...</div>;
  }

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <div style={{ fontSize: '0.9rem', color: 'var(--text-secondary)' }}>
          {channels.length === 0 ? 'No notification channels configured' : `${channels.length} channel${channels.length > 1 ? 's' : ''}`}
        </div>
        {!showAdd && (
          <button className="btn btn-primary" style={{ fontSize: '0.8rem', padding: '6px 14px' }} onClick={() => setShowAdd(true)}>
            + Add Channel
          </button>
        )}
      </div>

      {channels.map((ch) => (
        <div key={ch.id} className="card" style={{ padding: 14, marginBottom: 8 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <div>
              <div style={{ fontWeight: 600, fontSize: '0.9rem' }}>
                {ch.channel_type === 'webhook' ? <IconBell size={14} style={{ marginRight: 4 }} /> : <IconMail size={14} style={{ marginRight: 4 }} />}{ch.name}
              </div>
              <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginTop: 2 }}>
                {ch.channel_type} — {ch.channel_type === 'email' ? (ch.config?.address || 'No address') : (ch.config?.url || 'No URL')}
              </div>
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <button
                className={`btn ${ch.is_enabled ? 'btn-secondary' : 'btn-primary'}`}
                style={{ fontSize: '0.7rem', padding: '4px 10px' }}
                disabled={toggling === ch.id}
                onClick={() => handleToggle(ch.id, ch.is_enabled)}
                title={ch.is_enabled ? 'Disable notifications' : 'Enable notifications'}
              >
                {toggling === ch.id ? '...' : ch.is_enabled ? <><IconPause size={12} /> Disable</> : <><IconPlay size={12} /> Enable</>}
              </button>
              <button
                className="btn btn-danger"
                style={{ fontSize: '0.75rem', padding: '4px 10px' }}
                disabled={deleting === ch.id}
                onClick={() => handleDelete(ch.id)}
              >
                {deleting === ch.id ? '...' : <IconX size={12} />}
              </button>
            </div>
          </div>
        </div>
      ))}

      {showAdd && (
        <div className="card" style={{ borderColor: 'var(--accent)', marginTop: 8 }}>
          <h4 style={{ fontSize: '0.9rem', fontWeight: 600, marginBottom: 12 }}>Add Notification Channel</h4>

          {error && (
            <div style={{ background: 'var(--danger-bg)', border: '1px solid var(--danger)', borderRadius: 'var(--radius)', padding: '8px 12px', marginBottom: 12, fontSize: '0.85rem', color: 'var(--danger)' }}>
              {error}
            </div>
          )}

          <div className="form-row">
            <div className="form-group">
              <label className="form-label">Name</label>
              <input className="form-input" value={newName} onChange={e => setNewName(e.target.value)} placeholder="e.g. Slack Alerts" />
            </div>
            <div className="form-group">
              <label className="form-label">Type</label>
              <select className="form-input" value={newType} onChange={e => setNewType(e.target.value)}>
                <option value="webhook">Webhook</option>
                <option value="email">Email</option>
              </select>
            </div>
          </div>

          <div className="form-group">
            <label className="form-label">{newType === 'webhook' ? 'Webhook URL' : 'Email Address'}</label>
            <input
              className="form-input"
              value={newUrl}
              onChange={e => setNewUrl(e.target.value)}
              placeholder={newType === 'webhook' ? 'https://hooks.slack.com/...' : 'alerts@example.com'}
            />
            {newType === 'email' && (
              <div style={{ fontSize: '0.75rem', color: 'var(--text-muted)', marginTop: 4 }}>
                Requires SMTP configuration (SMTP_HOST, SMTP_USERNAME, SMTP_PASSWORD env vars)
              </div>
            )}
          </div>

          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button className="btn btn-secondary" style={{ fontSize: '0.8rem', padding: '6px 12px' }} onClick={() => { setShowAdd(false); setError(null); }}>Cancel</button>
            <button className="btn btn-primary" style={{ fontSize: '0.8rem', padding: '6px 12px' }} disabled={adding} onClick={handleAdd}>
              {adding ? 'Adding...' : 'Add Channel'}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function MaintenanceManager({ monitorId, manageKey }) {
  const [windows, setWindows] = useState([]);
  const [loading, setLoading] = useState(true);
  const [showAdd, setShowAdd] = useState(false);
  const [newTitle, setNewTitle] = useState('');
  const [newStartsAt, setNewStartsAt] = useState('');
  const [newEndsAt, setNewEndsAt] = useState('');
  const [adding, setAdding] = useState(false);
  const [deleting, setDeleting] = useState(null);
  const [error, setError] = useState(null);

  const loadWindows = async () => {
    try {
      const data = await getMaintenanceWindows(monitorId);
      setWindows(data);
    } catch (err) {
      // silent
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { loadWindows(); }, [monitorId]);

  // Convert local datetime-local input to ISO-8601 UTC string
  const toUTC = (localStr) => {
    if (!localStr) return '';
    const d = new Date(localStr);
    return d.toISOString().replace(/\.\d{3}Z$/, 'Z');
  };

  const handleAdd = async () => {
    if (!newTitle.trim()) {
      setError('Title is required');
      return;
    }
    if (!newStartsAt || !newEndsAt) {
      setError('Start and end times are required');
      return;
    }
    const startsUTC = toUTC(newStartsAt);
    const endsUTC = toUTC(newEndsAt);
    if (endsUTC <= startsUTC) {
      setError('End time must be after start time');
      return;
    }
    setAdding(true);
    setError(null);
    try {
      await createMaintenanceWindow(monitorId, {
        title: newTitle.trim(),
        starts_at: startsUTC,
        ends_at: endsUTC,
      }, manageKey);
      setNewTitle('');
      setNewStartsAt('');
      setNewEndsAt('');
      setShowAdd(false);
      await loadWindows();
    } catch (err) {
      setError(err.message);
    } finally {
      setAdding(false);
    }
  };

  const handleDelete = async (id) => {
    setDeleting(id);
    try {
      await deleteMaintenanceWindow(id, manageKey);
      await loadWindows();
    } catch (err) {
      alert(`Failed to delete: ${err.message}`);
    } finally {
      setDeleting(null);
    }
  };

  const categorize = (w) => {
    if (w.active) return 'active';
    const now = new Date();
    const starts = new Date(w.starts_at.endsWith('Z') ? w.starts_at : w.starts_at + 'Z');
    return starts > now ? 'upcoming' : 'past';
  };

  if (loading) {
    return <div style={{ color: 'var(--text-muted)', padding: '16px 0' }}>Loading maintenance windows...</div>;
  }

  const active = windows.filter(w => categorize(w) === 'active');
  const upcoming = windows.filter(w => categorize(w) === 'upcoming');
  const past = windows.filter(w => categorize(w) === 'past');

  const renderWindow = (w) => {
    const cat = categorize(w);
    const badgeColor = cat === 'active' ? 'var(--warning)' : cat === 'upcoming' ? 'var(--accent)' : 'var(--text-muted)';
    const badgeLabel = cat === 'active' ? <><IconWrench size={12} /> Active</> : cat === 'upcoming' ? <><IconCalendar size={12} /> Upcoming</> : <><IconCheckCircle size={12} /> Completed</>;
    return (
      <div key={w.id} className="card" style={{ padding: 14, marginBottom: 8 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'start' }}>
          <div style={{ flex: 1 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span style={{ fontWeight: 600, fontSize: '0.9rem' }}>{w.title}</span>
              <span style={{
                fontSize: '0.7rem',
                padding: '2px 8px',
                borderRadius: 12,
                background: `${badgeColor}20`,
                color: badgeColor,
                fontWeight: 600,
              }}>
                {badgeLabel}
              </span>
            </div>
            <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginTop: 4 }}>
              {formatTime(w.starts_at.replace('Z', ''))} → {formatTime(w.ends_at.replace('Z', ''))}
            </div>
          </div>
          {manageKey && (
            <button
              className="btn btn-danger"
              style={{ fontSize: '0.75rem', padding: '4px 10px', flexShrink: 0 }}
              disabled={deleting === w.id}
              onClick={() => handleDelete(w.id)}
            >
              {deleting === w.id ? '...' : <IconX size={12} />}
            </button>
          )}
        </div>
      </div>
    );
  };

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <div style={{ fontSize: '0.9rem', color: 'var(--text-secondary)' }}>
          {windows.length === 0 ? 'No maintenance windows scheduled' : `${windows.length} window${windows.length > 1 ? 's' : ''}`}
        </div>
        {manageKey && !showAdd && (
          <button className="btn btn-primary" style={{ fontSize: '0.8rem', padding: '6px 14px' }} onClick={() => setShowAdd(true)}>
            + Schedule Maintenance
          </button>
        )}
      </div>

      {showAdd && manageKey && (
        <div className="card" style={{ borderColor: 'var(--accent)', marginBottom: 16 }}>
          <h4 style={{ fontSize: '0.9rem', fontWeight: 600, marginBottom: 12 }}>Schedule Maintenance Window</h4>

          {error && (
            <div style={{ background: 'var(--danger-bg)', border: '1px solid var(--danger)', borderRadius: 'var(--radius)', padding: '8px 12px', marginBottom: 12, fontSize: '0.85rem', color: 'var(--danger)' }}>
              {error}
            </div>
          )}

          <div className="form-group">
            <label className="form-label">Title</label>
            <input className="form-input" value={newTitle} onChange={e => setNewTitle(e.target.value)} placeholder="e.g. Server migration" />
          </div>

          <div className="form-row">
            <div className="form-group">
              <label className="form-label">Starts At</label>
              <input className="form-input" type="datetime-local" value={newStartsAt} onChange={e => setNewStartsAt(e.target.value)} />
              <div className="form-help">Local time (converted to UTC)</div>
            </div>
            <div className="form-group">
              <label className="form-label">Ends At</label>
              <input className="form-input" type="datetime-local" value={newEndsAt} onChange={e => setNewEndsAt(e.target.value)} />
              <div className="form-help">Local time (converted to UTC)</div>
            </div>
          </div>

          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button className="btn btn-secondary" style={{ fontSize: '0.8rem', padding: '6px 12px' }} onClick={() => { setShowAdd(false); setError(null); }}>Cancel</button>
            <button className="btn btn-primary" style={{ fontSize: '0.8rem', padding: '6px 12px' }} disabled={adding} onClick={handleAdd}>
              {adding ? 'Scheduling...' : <><IconCalendar size={12} /> Schedule</>}
            </button>
          </div>
        </div>
      )}

      {active.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: '0.8rem', fontWeight: 600, color: 'var(--warning)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: '0.05em' }}>Active Now</div>
          {active.map(renderWindow)}
        </div>
      )}

      {upcoming.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: '0.8rem', fontWeight: 600, color: 'var(--accent)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: '0.05em' }}>Upcoming</div>
          {upcoming.map(renderWindow)}
        </div>
      )}

      {past.length > 0 && (
        <div>
          <div style={{ fontSize: '0.8rem', fontWeight: 600, color: 'var(--text-muted)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: '0.05em' }}>Completed</div>
          {past.map(renderWindow)}
        </div>
      )}
    </div>
  );
}

const HTTP_METHODS = ['GET', 'POST', 'HEAD', 'PUT', 'DELETE', 'PATCH'];

function EditMonitorForm({ monitor, manageKey, onSaved, onCancel }) {
  const [form, setForm] = useState({
    name: monitor.name || '',
    url: monitor.url || '',
    method: monitor.method || 'GET',
    interval_seconds: monitor.interval_seconds || 600,
    timeout_ms: monitor.timeout_ms || 10000,
    expected_status: monitor.expected_status || 200,
    confirmation_threshold: monitor.confirmation_threshold || 3,
    response_time_threshold_ms: monitor.response_time_threshold_ms ?? '',
    body_contains: monitor.body_contains || '',
    is_public: monitor.is_public ?? true,
    tagsInput: (monitor.tags || []).join(', '),
    group_name: monitor.group_name || '',
    dns_record_type: monitor.dns_record_type || 'A',
    dns_expected: monitor.dns_expected || '',
    sla_target: monitor.sla_target ?? '',
    sla_period_days: monitor.sla_period_days ?? 30,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(null);

  const set = (key, value) => setForm(f => ({ ...f, [key]: value }));

  const handleSave = async () => {
    if (!form.name.trim() || !form.url.trim()) {
      setError('Name and URL are required');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      // Only send fields that changed
      const patch = {};
      if (form.name !== monitor.name) patch.name = form.name.trim();
      if (form.url !== monitor.url) patch.url = form.url.trim();
      if (form.method !== monitor.method) patch.method = form.method;
      if (form.interval_seconds !== monitor.interval_seconds) patch.interval_seconds = Number(form.interval_seconds);
      if (form.timeout_ms !== monitor.timeout_ms) patch.timeout_ms = Number(form.timeout_ms);
      if (form.expected_status !== monitor.expected_status) patch.expected_status = Number(form.expected_status);
      if (form.confirmation_threshold !== monitor.confirmation_threshold) patch.confirmation_threshold = Number(form.confirmation_threshold);
      // Handle response_time_threshold_ms: '' means clear (null), number means set
      const newRtThreshold = form.response_time_threshold_ms === '' ? null : Number(form.response_time_threshold_ms);
      const oldRtThreshold = monitor.response_time_threshold_ms ?? null;
      if (newRtThreshold !== oldRtThreshold) patch.response_time_threshold_ms = newRtThreshold;
      if ((form.body_contains || '') !== (monitor.body_contains || '')) patch.body_contains = form.body_contains || null;
      if (form.is_public !== monitor.is_public) patch.is_public = form.is_public;
      const newTags = form.tagsInput.split(',').map(t => t.trim()).filter(Boolean);
      const oldTags = (monitor.tags || []).join(',');
      if (newTags.join(',') !== oldTags) patch.tags = newTags;
      const newGroup = form.group_name.trim();
      const oldGroup = monitor.group_name || '';
      if (newGroup !== oldGroup) patch.group_name = newGroup || '';
      // DNS fields
      if (monitor.monitor_type === 'dns') {
        if ((form.dns_record_type || 'A') !== (monitor.dns_record_type || 'A')) patch.dns_record_type = form.dns_record_type;
        const newExpected = form.dns_expected?.trim() || null;
        const oldExpected = monitor.dns_expected || null;
        if (newExpected !== oldExpected) patch.dns_expected = newExpected;
      }
      // SLA fields
      const newSlaTarget = form.sla_target === '' ? null : Number(form.sla_target);
      const oldSlaTarget = monitor.sla_target ?? null;
      if (newSlaTarget !== oldSlaTarget) patch.sla_target = newSlaTarget;
      const newSlaPeriod = form.sla_target === '' ? null : Number(form.sla_period_days);
      const oldSlaPeriod = monitor.sla_period_days ?? null;
      if (newSlaPeriod !== oldSlaPeriod) patch.sla_period_days = newSlaPeriod;

      if (Object.keys(patch).length === 0) {
        onCancel();
        return;
      }
      await updateMonitor(monitor.id, patch, manageKey);
      onSaved();
    } catch (err) {
      setError(err.message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="card" style={{ marginTop: 12, borderColor: 'var(--accent)', borderWidth: 2 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <h3 style={{ fontSize: '1rem', fontWeight: 600 }}><IconEdit size={16} style={{ marginRight: 6 }} />Edit Monitor</h3>
        <button className="btn btn-secondary" style={{ fontSize: '0.8rem', padding: '6px 12px' }} onClick={onCancel}>Cancel</button>
      </div>

      {error && (
        <div style={{ background: 'var(--danger-bg)', border: '1px solid var(--danger)', borderRadius: 'var(--radius)', padding: '10px 14px', marginBottom: 16, fontSize: '0.85rem', color: 'var(--danger)' }}>
          {error}
        </div>
      )}

      <div className="form-group">
        <label className="form-label">Name</label>
        <input className="form-input" value={form.name} onChange={e => set('name', e.target.value)} />
      </div>

      <div className="form-group">
        <label className="form-label">{monitor.monitor_type === 'tcp' ? 'Host:Port' : monitor.monitor_type === 'dns' ? 'Hostname' : 'URL'}</label>
        <input className="form-input" value={form.url} onChange={e => set('url', e.target.value)} placeholder={monitor.monitor_type === 'tcp' ? 'host:port' : monitor.monitor_type === 'dns' ? 'example.com' : 'https://example.com'} />
        {monitor.monitor_type === 'tcp' && <div className="form-help">Type: 🔌 TCP — connectivity check</div>}
        {monitor.monitor_type === 'dns' && <div className="form-help">Type: 🔍 DNS — record resolution check</div>}
      </div>

      {monitor.monitor_type === 'dns' && (
        <div className="form-row">
          <div className="form-group">
            <label className="form-label">Record Type</label>
            <select className="form-input" value={form.dns_record_type || 'A'} onChange={e => set('dns_record_type', e.target.value)}>
              <option value="A">A (IPv4)</option>
              <option value="AAAA">AAAA (IPv6)</option>
              <option value="CNAME">CNAME</option>
              <option value="MX">MX (mail)</option>
              <option value="TXT">TXT</option>
              <option value="NS">NS</option>
              <option value="SOA">SOA</option>
              <option value="PTR">PTR</option>
              <option value="SRV">SRV</option>
              <option value="CAA">CAA</option>
            </select>
          </div>
          <div className="form-group">
            <label className="form-label">Expected Value</label>
            <input className="form-input" value={form.dns_expected || ''} onChange={e => set('dns_expected', e.target.value)} placeholder="Optional — leave empty to just verify resolution" />
          </div>
        </div>
      )}

      {monitor.monitor_type === 'http' && (
        <div className="form-row">
          <div className="form-group">
            <label className="form-label">Method</label>
            <select className="form-input" value={form.method} onChange={e => set('method', e.target.value)}>
              {HTTP_METHODS.map(m => <option key={m} value={m}>{m}</option>)}
            </select>
          </div>
          <div className="form-group">
            <label className="form-label">Expected Status</label>
            <input className="form-input" type="number" value={form.expected_status} onChange={e => set('expected_status', Number(e.target.value))} />
          </div>
        </div>
      )}

      <div className="form-row">
        <div className="form-group">
          <label className="form-label">Check Interval (seconds)</label>
          <input className="form-input" type="number" min="600" value={form.interval_seconds} onChange={e => set('interval_seconds', Number(e.target.value))} />
          <div className="form-help">Minimum 10 minutes (600 seconds)</div>
        </div>
        <div className="form-group">
          <label className="form-label">Timeout (ms)</label>
          <input className="form-input" type="number" min="1000" value={form.timeout_ms} onChange={e => set('timeout_ms', Number(e.target.value))} />
        </div>
      </div>

      <div className="form-row">
        <div className="form-group">
          <label className="form-label">Confirmation Threshold</label>
          <input className="form-input" type="number" min="1" max="10" value={form.confirmation_threshold} onChange={e => set('confirmation_threshold', Number(e.target.value))} />
          <div className="form-help">Consecutive failures before incident</div>
        </div>
        <div className="form-group">
          <label className="form-label">Response Time Alert (ms)</label>
          <input className="form-input" type="number" min="100" placeholder="Disabled" value={form.response_time_threshold_ms} onChange={e => set('response_time_threshold_ms', e.target.value)} />
          <div className="form-help">Mark as degraded above this threshold. Empty = disabled.</div>
        </div>
      </div>

      <div className="form-row">
        <div className="form-group">
          <label className="form-label">SLA Target %</label>
          <input className="form-input" type="number" min="0" max="100" step="0.01" placeholder="e.g. 99.9" value={form.sla_target} onChange={e => set('sla_target', e.target.value)} />
          <div className="form-help">Uptime target for error budget tracking. Empty = disabled.</div>
        </div>
        <div className="form-group">
          <label className="form-label">SLA Period (days)</label>
          <input className="form-input" type="number" min="1" max="365" value={form.sla_period_days} onChange={e => set('sla_period_days', e.target.value)} disabled={form.sla_target === ''} />
          <div className="form-help">Rolling window for SLA calculation</div>
        </div>
      </div>

      {monitor.monitor_type === 'http' && (
        <div className="form-group">
          <label className="form-label">Body Contains</label>
          <input className="form-input" value={form.body_contains} onChange={e => set('body_contains', e.target.value)} placeholder="Optional text to match in response" />
        </div>
      )}

      <div className="form-row">
        <div className="form-group">
          <label className="form-label">Group</label>
          <input className="form-input" value={form.group_name} onChange={e => set('group_name', e.target.value)} placeholder="e.g. Infrastructure" />
          <div className="form-help">Section name on status page</div>
        </div>
        <div className="form-group">
          <label className="form-label">Tags</label>
          <input className="form-input" value={form.tagsInput} onChange={e => set('tagsInput', e.target.value)} placeholder="prod, api, critical" />
          <div className="form-help">Comma-separated tags for filtering</div>
        </div>
      </div>

      <div className="form-group" style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <input type="checkbox" id="edit-public" checked={form.is_public} onChange={e => set('is_public', e.target.checked)} style={{ width: 18, height: 18, accentColor: 'var(--accent)' }} />
        <label htmlFor="edit-public" className="form-label" style={{ marginBottom: 0 }}>Public (visible on status page without manage key)</label>
      </div>

      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', marginTop: 8 }}>
        <button className="btn btn-secondary" onClick={onCancel} disabled={saving}>Cancel</button>
        <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
          {saving ? 'Saving...' : <><IconSave size={14} /> Save Changes</>}
        </button>
      </div>
    </div>
  );
}

function ManageKeyInput({ monitorId, onKeySet }) {
  const [showInput, setShowInput] = useState(false);
  const [keyValue, setKeyValue] = useState('');
  const [validating, setValidating] = useState(false);
  const [error, setError] = useState(null);

  const handleSubmit = async () => {
    const key = keyValue.trim();
    if (!key) return;
    setValidating(true);
    setError(null);
    try {
      // Validate the key by trying to fetch notifications (requires auth)
      await getNotifications(monitorId, key);
      // Key is valid — persist and activate
      try { localStorage.setItem(`watchpost_key_${monitorId}`, key); } catch (e) { /* silent */ }
      onKeySet(key);
    } catch (err) {
      // Try a PATCH with empty body as fallback validation
      try {
        await updateMonitor(monitorId, {}, key);
        try { localStorage.setItem(`watchpost_key_${monitorId}`, key); } catch (e) { /* silent */ }
        onKeySet(key);
      } catch (err2) {
        setError('Invalid manage key');
      }
    } finally {
      setValidating(false);
    }
  };

  if (!showInput) {
    return (
      <div style={{ marginTop: 12 }}>
        <button
          className="btn btn-secondary"
          style={{ fontSize: '0.85rem', padding: '8px 16px' }}
          onClick={() => setShowInput(true)}
        >
          <IconKey size={14} /> Enter Manage Key
        </button>
        <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginLeft: 10 }}>
          Unlock editing, pause/resume, and delete
        </span>
      </div>
    );
  }

  return (
    <div className="card" style={{ marginTop: 12, borderColor: 'var(--accent)', padding: 16 }}>
      <div style={{ fontSize: '0.9rem', fontWeight: 600, marginBottom: 10 }}><IconKey size={14} style={{ marginRight: 6 }} />Enter Manage Key</div>
      <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginBottom: 12 }}>
        Paste the manage key you received when creating this monitor.
      </div>
      {error && (
        <div style={{ background: 'var(--danger-bg)', border: '1px solid var(--danger)', borderRadius: 'var(--radius)', padding: '8px 12px', marginBottom: 12, fontSize: '0.85rem', color: 'var(--danger)' }}>
          {error}
        </div>
      )}
      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <input
          className="form-input"
          type="password"
          style={{ flex: 1, fontFamily: 'monospace', fontSize: '0.85rem' }}
          placeholder="wp_..."
          value={keyValue}
          onChange={(e) => setKeyValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSubmit()}
          autoFocus
        />
        <button
          className="btn btn-primary"
          style={{ fontSize: '0.85rem', padding: '8px 16px' }}
          disabled={validating || !keyValue.trim()}
          onClick={handleSubmit}
        >
          {validating ? 'Checking...' : 'Unlock'}
        </button>
        <button
          className="btn btn-secondary"
          style={{ fontSize: '0.85rem', padding: '8px 12px' }}
          onClick={() => { setShowInput(false); setError(null); setKeyValue(''); }}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

// ── SLA Card ──

function SlaCard({ sla }) {
  const statusColor = sla.status === 'met' ? 'var(--success)' :
    sla.status === 'at_risk' ? 'var(--warning, #f59e0b)' : 'var(--danger)';
  const statusLabel = sla.status === 'met' ? 'SLA Met' :
    sla.status === 'at_risk' ? 'At Risk' : 'SLA Breached';
  const budgetPct = Math.min(sla.budget_used_pct, 100);

  const formatSeconds = (s) => {
    if (s < 0) return `-${formatSeconds(-s)}`;
    if (s < 60) return `${Math.round(s)}s`;
    if (s < 3600) return `${Math.round(s / 60)}m`;
    if (s < 86400) return `${(s / 3600).toFixed(1)}h`;
    return `${(s / 86400).toFixed(1)}d`;
  };

  return (
    <div style={{
      background: 'var(--card-bg)',
      borderRadius: 8,
      padding: 16,
      marginTop: 16,
      border: `1px solid ${sla.status === 'breached' ? 'var(--danger)' : sla.status === 'at_risk' ? 'var(--warning, #f59e0b)' : 'var(--border)'}`,
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <IconTrendUp size={16} />
          <span style={{ fontWeight: 600, fontSize: '0.95rem' }}>SLA Tracking</span>
          <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)' }}>
            ({sla.period_days}d rolling)
          </span>
        </div>
        <span style={{
          padding: '2px 10px',
          borderRadius: 12,
          fontSize: '0.8rem',
          fontWeight: 600,
          background: statusColor + '22',
          color: statusColor,
        }}>
          {statusLabel}
        </span>
      </div>

      <div style={{ display: 'flex', gap: 24, flexWrap: 'wrap', marginBottom: 12 }}>
        <div>
          <div style={{ fontSize: '0.75rem', color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Target</div>
          <div style={{ fontSize: '1.1rem', fontWeight: 600 }}>{sla.target_pct}%</div>
        </div>
        <div>
          <div style={{ fontSize: '0.75rem', color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Current</div>
          <div style={{ fontSize: '1.1rem', fontWeight: 600, color: sla.current_pct >= sla.target_pct ? 'var(--success)' : 'var(--danger)' }}>
            {sla.current_pct.toFixed(3)}%
          </div>
        </div>
        <div>
          <div style={{ fontSize: '0.75rem', color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Checks</div>
          <div style={{ fontSize: '1.1rem', fontWeight: 600 }}>{sla.successful_checks}/{sla.total_checks}</div>
        </div>
        <div>
          <div style={{ fontSize: '0.75rem', color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Budget Left</div>
          <div style={{ fontSize: '1.1rem', fontWeight: 600, color: sla.budget_remaining_seconds < 0 ? 'var(--danger)' : 'var(--text-primary)' }}>
            {formatSeconds(sla.budget_remaining_seconds)}
          </div>
        </div>
      </div>

      {/* Error Budget Bar */}
      <div style={{ marginTop: 4 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '0.75rem', color: 'var(--text-muted)', marginBottom: 4 }}>
          <span>Error Budget</span>
          <span>{budgetPct.toFixed(1)}% used of {formatSeconds(sla.budget_total_seconds)}</span>
        </div>
        <div style={{
          height: 8,
          borderRadius: 4,
          background: 'var(--border)',
          overflow: 'hidden',
        }}>
          <div style={{
            height: '100%',
            width: `${budgetPct}%`,
            borderRadius: 4,
            background: budgetPct > 75 ? 'var(--danger)' : budgetPct > 50 ? 'var(--warning, #f59e0b)' : 'var(--success)',
            transition: 'width 0.3s ease',
          }} />
        </div>
      </div>
    </div>
  );
}

export default function MonitorDetail({ id, manageKey: urlKey, onBack }) {
  const [monitor, setMonitor] = useState(null);
  const [heartbeats, setHeartbeats] = useState([]);
  const [uptime, setUptime] = useState(null);
  const [incidents, setIncidents] = useState([]);
  const [sla, setSla] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [tab, setTab] = useState('overview');
  const [actionLoading, setActionLoading] = useState(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [editing, setEditing] = useState(false);
  const [enteredKey, setEnteredKey] = useState(null);
  const [linkCopied, setLinkCopied] = useState(false);

  // Effective manage key: URL param > entered key > localStorage
  const manageKey = urlKey || enteredKey || (() => {
    try { return localStorage.getItem(`watchpost_key_${id}`) || ''; } catch (e) { return ''; }
  })();

  // Auto-save key to localStorage when loaded via URL
  useEffect(() => {
    if (urlKey) {
      try { localStorage.setItem(`watchpost_key_${id}`, urlKey); } catch (e) { /* silent */ }
    }
  }, [id, urlKey]);

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const [m, hb, u, inc, slaData] = await Promise.all([
          getMonitor(id),
          getHeartbeats(id, 100),
          getUptime(id),
          getIncidents(id),
          getMonitorSla(id).catch(() => null),
        ]);
        if (mounted) {
          setMonitor(m);
          setHeartbeats(hb);
          setUptime(u);
          setIncidents(inc);
          setSla(slaData);
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
    return (
      <div>
        <div style={{ padding: '16px 0' }}>
          <div className="skeleton skeleton-text short" />
        </div>
        <div className="skeleton-card">
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
            <div>
              <div className="skeleton skeleton-text wide" style={{ height: 20, width: 200 }} />
              <div className="skeleton skeleton-text medium" style={{ marginTop: 8, width: 280 }} />
            </div>
            <div className="skeleton skeleton-badge" />
          </div>
          <div className="monitor-stats">
            {[1, 2, 3, 4, 5].map((j) => (
              <div key={j} className="monitor-stat">
                <div className="skeleton skeleton-text short" />
                <div className="skeleton skeleton-stat" />
              </div>
            ))}
          </div>
        </div>
        {/* Uptime bar skeleton */}
        <div style={{ display: 'flex', gap: 2, margin: '16px 0' }}>
          {Array.from({ length: 40 }, (_, i) => (
            <div key={i} className="skeleton skeleton-bar-segment" />
          ))}
        </div>
        {/* Uptime stats skeleton */}
        <div className="skeleton-uptime-grid">
          {[1, 2, 3, 4].map((i) => (
            <div key={i} className="skeleton-uptime-cell">
              <div className="skeleton skeleton-uptime-value" />
              <div className="skeleton skeleton-uptime-label" />
            </div>
          ))}
        </div>
      </div>
    );
  }

  const reload = async () => {
    try {
      const [m, hb, u, inc] = await Promise.all([
        getMonitor(id),
        getHeartbeats(id, 100),
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
              {monitor.monitor_type === 'tcp' ? '🔌 TCP' : monitor.monitor_type === 'dns' ? `🔍 DNS ${monitor.dns_record_type || 'A'}` : monitor.method} {monitor.url}
            </div>
          </div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            {monitor.is_paused && <span className="badge paused"><IconPause size={12} /> Paused</span>}
            <span className={`badge ${monitor.current_status}`}>
              <span className="badge-dot" />
              {monitor.current_status}
            </span>
          </div>
        </div>

        {(monitor.group_name || (monitor.tags || []).length > 0) && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginBottom: 12, alignItems: 'center' }}>
            {monitor.group_name && (
              <span className="tag-badge" style={{ background: 'var(--accent)', color: '#000', fontWeight: 600 }}>
                {monitor.group_name}
              </span>
            )}
            {(monitor.tags || []).map((t) => (
              <span key={t} className="tag-badge">{t}</span>
            ))}
          </div>
        )}

        <div className="monitor-stats">
          <div className="monitor-stat">
            <span className="monitor-stat-label">Interval</span>
            <span className="monitor-stat-value">{monitor.interval_seconds}s</span>
          </div>
          <div className="monitor-stat">
            <span className="monitor-stat-label">Timeout</span>
            <span className="monitor-stat-value">{monitor.timeout_ms}ms</span>
          </div>
          {monitor.monitor_type === 'tcp' ? (
            <div className="monitor-stat">
              <span className="monitor-stat-label">Type</span>
              <span className="monitor-stat-value">🔌 TCP</span>
            </div>
          ) : monitor.monitor_type === 'dns' ? (
            <>
              <div className="monitor-stat">
                <span className="monitor-stat-label">Type</span>
                <span className="monitor-stat-value">🔍 DNS</span>
              </div>
              <div className="monitor-stat">
                <span className="monitor-stat-label">Record</span>
                <span className="monitor-stat-value">{monitor.dns_record_type || 'A'}</span>
              </div>
              {monitor.dns_expected && (
                <div className="monitor-stat">
                  <span className="monitor-stat-label">Expected</span>
                  <span className="monitor-stat-value" style={{ fontSize: '0.8rem', maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis' }}>{monitor.dns_expected}</span>
                </div>
              )}
            </>
          ) : (
            <div className="monitor-stat">
              <span className="monitor-stat-label">Expected</span>
              <span className="monitor-stat-value">HTTP {monitor.expected_status}</span>
            </div>
          )}
          <div className="monitor-stat">
            <span className="monitor-stat-label">Confirm</span>
            <span className="monitor-stat-value">{monitor.confirmation_threshold}x</span>
          </div>
          <div className="monitor-stat">
            <span className="monitor-stat-label">RT Alert</span>
            <span className="monitor-stat-value">{monitor.response_time_threshold_ms ? `${monitor.response_time_threshold_ms}ms` : 'Off'}</span>
          </div>
          {monitor.sla_target != null && (
            <div className="monitor-stat">
              <span className="monitor-stat-label">SLA</span>
              <span className="monitor-stat-value">{monitor.sla_target}% / {monitor.sla_period_days || 30}d</span>
            </div>
          )}
          <div className="monitor-stat">
            <span className="monitor-stat-label">Last Check</span>
            <span className="monitor-stat-value">{relativeTime(monitor.last_checked_at)}</span>
          </div>
        </div>

        {manageKey ? (
          <div className="manage-panel">
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginRight: 8 }}><IconKey size={12} style={{ marginRight: 4 }} />Manage:</span>
              <button
                className="btn btn-secondary"
                style={{ fontSize: '0.8rem', padding: '6px 14px' }}
                onClick={() => { setEditing(true); setConfirmDelete(false); }}
                disabled={editing}
              >
                <IconEdit size={12} /> Edit
              </button>
              <button
                className="btn btn-secondary"
                style={{ fontSize: '0.8rem', padding: '6px 14px' }}
                disabled={actionLoading === 'pause' || actionLoading === 'resume'}
                onClick={handlePauseResume}
              >
                {actionLoading === 'pause' || actionLoading === 'resume'
                  ? '...'
                  : monitor.is_paused ? <><IconPlay size={12} /> Resume</> : <><IconPause size={12} /> Pause</>}
              </button>
              <button
                className="btn btn-secondary"
                style={{ fontSize: '0.8rem', padding: '6px 14px' }}
                onClick={() => {
                  const link = `${window.location.origin}/#/monitor/${id}?key=${manageKey}`;
                  navigator.clipboard.writeText(link).then(() => {
                    setLinkCopied(true);
                    setTimeout(() => setLinkCopied(false), 3000);
                  });
                }}
              >
                {linkCopied ? <><IconCheckCircle size={12} /> Copied!</> : <><IconLink size={12} /> Copy Link</>}
              </button>
              {!confirmDelete ? (
                <button
                  className="btn btn-danger"
                  style={{ fontSize: '0.8rem', padding: '6px 14px' }}
                  onClick={() => setConfirmDelete(true)}
                >
                  <IconTrash size={12} /> Delete
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
              {!urlKey && (
                <button
                  className="btn btn-secondary"
                  style={{ fontSize: '0.75rem', padding: '4px 10px', marginLeft: 'auto' }}
                  onClick={() => {
                    setEnteredKey(null);
                    try { localStorage.removeItem(`watchpost_key_${id}`); } catch (e) { /* silent */ }
                  }}
                  title="Remove saved manage key"
                >
                  <IconLock size={12} /> Lock
                </button>
              )}
            </div>
          </div>
        ) : (
          <ManageKeyInput monitorId={id} onKeySet={(key) => setEnteredKey(key)} />
        )}

        {editing && manageKey && (
          <EditMonitorForm
            monitor={monitor}
            manageKey={manageKey}
            onSaved={async () => { setEditing(false); await reload(); }}
            onCancel={() => setEditing(false)}
          />
        )}
      </div>

      <UptimeBar heartbeats={heartbeats} />

      {/* Tabs */}
      <div style={{ display: 'flex', gap: 4, margin: '24px 0 16px', borderBottom: '1px solid var(--border)', paddingBottom: 8, flexWrap: 'wrap' }}>
        {['overview', 'heartbeats', 'incidents', 'maintenance', 'badges', ...(manageKey ? ['notifications'] : [])].map((t) => (
          <button
            key={t}
            className={`nav-btn ${tab === t ? 'active' : ''}`}
            onClick={() => setTab(t)}
          >
            {t === 'overview' ? <><IconDashboard size={13} /> Overview</> : t === 'heartbeats' ? <><IconHeart size={13} /> Heartbeats</> : t === 'incidents' ? <><IconZap size={13} /> Incidents ({incidents.length})</> : t === 'maintenance' ? <><IconWrench size={13} /> Maintenance</> : t === 'badges' ? <><IconTag size={13} /> Badges</> : <><IconBell size={13} /> Notifications</>}
          </button>
        ))}
      </div>

      {tab === 'overview' && (
        <div>
          <UptimeStats stats={uptime} />
          {sla && <SlaCard sla={sla} />}
          <ResponseTimeChart heartbeats={heartbeats} />
          {uptime?.avg_response_ms_24h != null && (
            <div style={{ textAlign: 'center', marginTop: 12, color: 'var(--text-muted)', fontSize: '0.85rem' }}>
              Avg response time (24h): <strong style={{ color: 'var(--text-primary)' }}>{Math.round(uptime.avg_response_ms_24h)}ms</strong>
            </div>
          )}
        </div>
      )}

      {tab === 'heartbeats' && <HeartbeatTable heartbeats={heartbeats} />}

      {tab === 'incidents' && <IncidentList incidents={incidents} manageKey={manageKey} onAck={reload} />}

      {tab === 'maintenance' && <MaintenanceManager monitorId={id} manageKey={manageKey} />}

      {tab === 'badges' && <BadgeEmbed monitorId={id} />}

      {tab === 'notifications' && manageKey && <NotificationManager monitorId={id} manageKey={manageKey} />}
    </div>
  );
}

function BadgeEmbed({ monitorId }) {
  const [period, setPeriod] = useState('24h');
  const [copied, setCopied] = useState(null);
  const base = window.location.origin;

  const uptimeUrl = `${base}/api/v1/monitors/${monitorId}/badge/uptime?period=${period}`;
  const statusUrl = `${base}/api/v1/monitors/${monitorId}/badge/status`;

  const uptimeMd = `![uptime](${uptimeUrl})`;
  const statusMd = `![status](${statusUrl})`;
  const uptimeHtml = `<img src="${uptimeUrl}" alt="uptime">`;
  const statusHtml = `<img src="${statusUrl}" alt="status">`;

  const copy = (text, key) => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(key);
      setTimeout(() => setCopied(null), 2000);
    });
  };

  const codeStyle = {
    background: 'var(--bg-secondary)',
    padding: '8px 12px',
    borderRadius: 6,
    fontSize: '0.85rem',
    fontFamily: 'monospace',
    wordBreak: 'break-all',
    display: 'block',
    marginBottom: 8,
    color: 'var(--text-primary)',
    border: '1px solid var(--border)',
  };

  const btnStyle = {
    fontSize: '0.8rem',
    padding: '4px 10px',
    cursor: 'pointer',
    background: 'var(--bg-secondary)',
    color: 'var(--text-primary)',
    border: '1px solid var(--border)',
    borderRadius: 4,
  };

  return (
    <div>
      <h3 style={{ margin: '0 0 16px' }}>Embeddable Badges</h3>
      <p style={{ color: 'var(--text-muted)', fontSize: '0.9rem', marginBottom: 20 }}>
        Add these badges to your README, status page, or dashboard.
      </p>

      <div style={{ marginBottom: 24 }}>
        <h4 style={{ margin: '0 0 8px' }}>Uptime Badge</h4>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 12 }}>
          <span style={{ color: 'var(--text-muted)', fontSize: '0.85rem' }}>Period:</span>
          {['24h', '7d', '30d', '90d'].map(p => (
            <button
              key={p}
              onClick={() => setPeriod(p)}
              style={{
                ...btnStyle,
                background: period === p ? 'var(--accent)' : 'var(--bg-secondary)',
                color: period === p ? '#fff' : 'var(--text-primary)',
              }}
            >{p}</button>
          ))}
        </div>
        <div style={{ marginBottom: 12 }}>
          <img src={uptimeUrl} alt="uptime badge" style={{ height: 20 }} />
        </div>
        <div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)' }}>Markdown</span>
            <button style={btnStyle} onClick={() => copy(uptimeMd, 'uptimeMd')}>
              {copied === 'uptimeMd' ? <><IconCheckCircle size={12} /> Copied</> : <><IconClipboard size={12} /> Copy</>}
            </button>
          </div>
          <code style={codeStyle}>{uptimeMd}</code>
        </div>
        <div style={{ marginTop: 8 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)' }}>HTML</span>
            <button style={btnStyle} onClick={() => copy(uptimeHtml, 'uptimeHtml')}>
              {copied === 'uptimeHtml' ? <><IconCheckCircle size={12} /> Copied</> : <><IconClipboard size={12} /> Copy</>}
            </button>
          </div>
          <code style={codeStyle}>{uptimeHtml}</code>
        </div>
      </div>

      <div>
        <h4 style={{ margin: '0 0 12px' }}>Status Badge</h4>
        <div style={{ marginBottom: 12 }}>
          <img src={statusUrl} alt="status badge" style={{ height: 20 }} />
        </div>
        <div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)' }}>Markdown</span>
            <button style={btnStyle} onClick={() => copy(statusMd, 'statusMd')}>
              {copied === 'statusMd' ? <><IconCheckCircle size={12} /> Copied</> : <><IconClipboard size={12} /> Copy</>}
            </button>
          </div>
          <code style={codeStyle}>{statusMd}</code>
        </div>
        <div style={{ marginTop: 8 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span style={{ fontSize: '0.8rem', color: 'var(--text-muted)' }}>HTML</span>
            <button style={btnStyle} onClick={() => copy(statusHtml, 'statusHtml')}>
              {copied === 'statusHtml' ? <><IconCheckCircle size={12} /> Copied</> : <><IconClipboard size={12} /> Copy</>}
            </button>
          </div>
          <code style={codeStyle}>{statusHtml}</code>
        </div>
      </div>
    </div>
  );
}
