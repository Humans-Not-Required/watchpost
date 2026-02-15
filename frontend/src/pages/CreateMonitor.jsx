import { useState } from 'react'
import { createMonitor } from '../api'
import { IconCheckCircle, IconLink, IconClipboard, IconKey, IconGlobe, IconLock } from '../Icons'

export default function CreateMonitor({ onCreated, onCancel }) {
  const [form, setForm] = useState({
    name: '',
    url: '',
    monitor_type: 'http',
    method: 'GET',
    interval_seconds: 600,
    timeout_ms: 10000,
    expected_status: 200,
    body_contains: '',
    is_public: false,
    confirmation_threshold: 2,
    response_time_threshold_ms: '',
    tagsInput: '',
    group_name: '',
  });
  const [copied, setCopied] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState(null);
  const [result, setResult] = useState(null);

  const update = (key, val) => setForm({ ...form, [key]: val });

  const handleSubmit = async (e) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);

    try {
      const payload = {
        name: form.name.trim(),
        url: form.url.trim(),
        monitor_type: form.monitor_type,
        interval_seconds: parseInt(form.interval_seconds, 10),
        timeout_ms: parseInt(form.timeout_ms, 10),
        is_public: form.is_public,
        confirmation_threshold: parseInt(form.confirmation_threshold, 10),
      };
      // HTTP-specific fields
      if (form.monitor_type === 'http') {
        payload.method = form.method;
        payload.expected_status = parseInt(form.expected_status, 10);
      }
      if (form.body_contains.trim() && form.monitor_type === 'http') {
        payload.body_contains = form.body_contains.trim();
      }
      if (form.response_time_threshold_ms !== '') {
        payload.response_time_threshold_ms = parseInt(form.response_time_threshold_ms, 10);
      }
      const tags = form.tagsInput.split(',').map(t => t.trim()).filter(Boolean);
      if (tags.length > 0) {
        payload.tags = tags;
      }
      if (form.group_name.trim()) {
        payload.group_name = form.group_name.trim();
      }

      const data = await createMonitor(payload);
      setResult(data);
    } catch (err) {
      setError(err.message);
    } finally {
      setSubmitting(false);
    }
  };

  if (result) {
    // Auto-save key to localStorage
    try { localStorage.setItem(`watchpost_key_${result.monitor.id}`, result.manage_key); } catch (e) { /* silent */ }

    const manageUrl = `${window.location.origin}/#/monitor/${result.monitor.id}?key=${result.manage_key}`;
    const viewUrl = `${window.location.origin}/#/monitor/${result.monitor.id}`;

    const handleCopy = () => {
      navigator.clipboard.writeText(manageUrl).then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 3000);
      });
    };

    return (
      <div style={{ marginTop: 24 }}>
        <h2 className="section-title" style={{ color: 'var(--success)' }}>
          <IconCheckCircle size={18} style={{ marginRight: 6 }} />Monitor Created!
        </h2>

        <div className="manage-key-banner">
          <div style={{ fontWeight: 700, marginBottom: 8, color: 'var(--accent)' }}>
            <IconLink size={14} style={{ marginRight: 6 }} />Bookmark this manage link — it's your key to this monitor
          </div>
          <div style={{
            display: 'flex', gap: 8, alignItems: 'center',
            background: 'var(--bg-primary)', borderRadius: 6, padding: '8px 12px',
            border: '1px solid var(--border)',
          }}>
            <code style={{
              flex: 1, fontSize: '0.8rem', wordBreak: 'break-all',
              color: 'var(--accent)', lineHeight: 1.4,
            }}>
              {manageUrl}
            </code>
            <button
              className="btn btn-primary"
              style={{ fontSize: '0.8rem', padding: '6px 14px', flexShrink: 0 }}
              onClick={handleCopy}
            >
              {copied ? <><IconCheckCircle size={12} /> Copied!</> : <><IconClipboard size={12} /> Copy</>}
            </button>
          </div>
          <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginTop: 8 }}>
            This link includes your manage key. Bookmark it or save it somewhere safe — anyone with this link can edit, pause, or delete this monitor.
            Your key is also saved in this browser automatically.
          </div>
        </div>

        <div className="card" style={{ marginTop: 16 }}>
          <div className="card-header">
            <span className="card-title">{result.monitor.name}</span>
            <span className={`badge ${result.monitor.current_status}`}>
              <span className="badge-dot" />
              {result.monitor.current_status}
            </span>
          </div>
          {(result.monitor.tags || []).length > 0 && (
            <div className="tag-list" style={{ marginBottom: 12 }}>
              {result.monitor.tags.map((t) => (
                <span key={t} className="tag-badge">{t}</span>
              ))}
            </div>
          )}
          <div className="monitor-stats">
            <div className="monitor-stat">
              <span className="monitor-stat-label">URL</span>
              <span className="monitor-stat-value" style={{ fontSize: '0.85rem' }}>
                {result.monitor.url}
              </span>
            </div>
            <div className="monitor-stat">
              <span className="monitor-stat-label">Type</span>
              <span className="monitor-stat-value">{result.monitor.monitor_type === 'tcp' ? '🔌 TCP' : '🌐 HTTP'}</span>
            </div>
            {result.monitor.monitor_type !== 'tcp' && (
              <div className="monitor-stat">
                <span className="monitor-stat-label">Method</span>
                <span className="monitor-stat-value">{result.monitor.method}</span>
              </div>
            )}
            <div className="monitor-stat">
              <span className="monitor-stat-label">Interval</span>
              <span className="monitor-stat-value">{result.monitor.interval_seconds}s</span>
            </div>
            <div className="monitor-stat">
              <span className="monitor-stat-label">Visibility</span>
              <span className="monitor-stat-value">{result.monitor.is_public ? <><IconGlobe size={14} /> Public</> : <><IconLock size={14} /> Private</>}</span>
            </div>
          </div>
        </div>

        <div style={{ marginTop: 16, display: 'flex', gap: 8 }}>
          <button className="btn btn-primary" onClick={() => {
            window.location.hash = `/monitor/${result.monitor.id}?key=${result.manage_key}`;
          }}>
            View Monitor →
          </button>
          <button className="btn btn-secondary" onClick={onCancel}>
            Back to Dashboard
          </button>
        </div>

        <div style={{ marginTop: 16, padding: 16, background: 'var(--bg-secondary)', borderRadius: 'var(--radius)', fontSize: '0.85rem' }}>
          <div style={{ color: 'var(--text-muted)', marginBottom: 8 }}>Quick Reference (API)</div>
          <div style={{ color: 'var(--text-secondary)' }}>
            <div>View: <code style={{ color: 'var(--accent)' }}>GET {result.api_base}</code></div>
            <div>Dashboard: <code style={{ color: 'var(--accent)' }}>{viewUrl}</code></div>
            <div>Manage: <code style={{ color: 'var(--accent)' }}>{manageUrl}</code></div>
          </div>
        </div>

        <details style={{ marginTop: 12, fontSize: '0.85rem' }}>
          <summary style={{ color: 'var(--text-muted)', cursor: 'pointer' }}><IconKey size={12} style={{ marginRight: 4 }} />Raw manage key</summary>
          <code style={{ display: 'block', marginTop: 8, padding: '8px 12px', background: 'var(--bg-secondary)', borderRadius: 6, wordBreak: 'break-all', color: 'var(--text-secondary)' }}>
            {result.manage_key}
          </code>
        </details>
      </div>
    );
  }

  return (
    <div style={{ marginTop: 24 }}>
      <h2 className="section-title">Create New Monitor</h2>

      <form onSubmit={handleSubmit}>
        <div className="card">
          <div className="form-group">
            <label className="form-label">Monitor Name *</label>
            <input
              className="form-input"
              type="text"
              placeholder="e.g. Production API"
              value={form.name}
              onChange={(e) => update('name', e.target.value)}
              required
            />
          </div>

          <div className="form-group">
            <label className="form-label">Monitor Type</label>
            <div style={{ display: 'flex', gap: 0, borderRadius: 'var(--radius)', overflow: 'hidden', border: '1px solid var(--border)' }}>
              {['http', 'tcp'].map(type_ => (
                <button
                  key={type_}
                  type="button"
                  onClick={() => update('monitor_type', type_)}
                  style={{
                    flex: 1, padding: '8px 16px', border: 'none', cursor: 'pointer',
                    background: form.monitor_type === type_ ? 'var(--accent)' : 'var(--bg-secondary)',
                    color: form.monitor_type === type_ ? '#fff' : 'var(--text-secondary)',
                    fontWeight: form.monitor_type === type_ ? 600 : 400,
                    fontSize: '0.9rem', transition: 'all 0.15s ease',
                  }}
                >
                  {type_ === 'http' ? '🌐 HTTP' : '🔌 TCP'}
                </button>
              ))}
            </div>
            <div className="form-help">
              {form.monitor_type === 'http'
                ? 'Monitor HTTP/HTTPS endpoints with status codes, body matching, and response times'
                : 'Monitor TCP port connectivity (databases, Redis, SMTP, custom services)'}
            </div>
          </div>

          <div className="form-group">
            <label className="form-label">{form.monitor_type === 'tcp' ? 'Host:Port *' : 'URL to Monitor *'}</label>
            <input
              className="form-input"
              type={form.monitor_type === 'tcp' ? 'text' : 'url'}
              placeholder={form.monitor_type === 'tcp' ? 'e.g. db.example.com:5432' : 'https://api.example.com/health'}
              value={form.url}
              onChange={(e) => update('url', e.target.value)}
              required
            />
            {form.monitor_type === 'tcp' && (
              <div className="form-help">Format: host:port (e.g., redis.example.com:6379, tcp://db.example.com:5432)</div>
            )}
          </div>

          {form.monitor_type === 'http' && (
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">HTTP Method</label>
                <select
                  className="form-input"
                  value={form.method}
                  onChange={(e) => update('method', e.target.value)}
                >
                  <option value="GET">GET</option>
                  <option value="HEAD">HEAD</option>
                  <option value="POST">POST</option>
                </select>
              </div>

              <div className="form-group">
                <label className="form-label">Expected Status Code</label>
                <input
                  className="form-input"
                  type="number"
                  value={form.expected_status}
                  onChange={(e) => update('expected_status', e.target.value)}
                />
              </div>
            </div>
          )}

          <div className="form-row">
            <div className="form-group">
              <label className="form-label">Check Interval (seconds)</label>
              <input
                className="form-input"
                type="number"
                min="600"
                value={form.interval_seconds}
                onChange={(e) => update('interval_seconds', e.target.value)}
              />
              <div className="form-help">Minimum: 10 minutes (600 seconds)</div>
            </div>

            <div className="form-group">
              <label className="form-label">Timeout (ms)</label>
              <input
                className="form-input"
                type="number"
                min="1000"
                max="60000"
                value={form.timeout_ms}
                onChange={(e) => update('timeout_ms', e.target.value)}
              />
            </div>
          </div>

          <div className="form-row">
            <div className="form-group">
              <label className="form-label">Confirmation Threshold</label>
              <input
                className="form-input"
                type="number"
                min="1"
                max="10"
                value={form.confirmation_threshold}
                onChange={(e) => update('confirmation_threshold', e.target.value)}
              />
              <div className="form-help">Consecutive failures before marking down</div>
            </div>

            <div className="form-group">
              <label className="form-label">Visibility</label>
              <select
                className="form-input"
                value={form.is_public ? 'public' : 'private'}
                onChange={(e) => update('is_public', e.target.value === 'public')}
              >
                <option value="public">Public (visible on status page)</option>
                <option value="private">Private (API access only)</option>
              </select>
            </div>
          </div>

          <div className="form-group">
            <label className="form-label">Response Time Alert (ms, optional)</label>
            <input
              className="form-input"
              type="number"
              min="100"
              placeholder="e.g. 2000"
              value={form.response_time_threshold_ms}
              onChange={(e) => update('response_time_threshold_ms', e.target.value)}
            />
            <div className="form-help">Mark as degraded when response time exceeds this threshold. Leave empty to disable.</div>
          </div>

          <div className="form-row">
            <div className="form-group">
              <label className="form-label">Group (optional)</label>
              <input
                className="form-input"
                type="text"
                placeholder="e.g. Infrastructure, APIs"
                value={form.group_name}
                onChange={(e) => update('group_name', e.target.value)}
              />
              <div className="form-help">Organize monitors into sections on the status page</div>
            </div>

            <div className="form-group">
              <label className="form-label">Tags (optional)</label>
              <input
                className="form-input"
                type="text"
                placeholder="e.g. prod, api, critical"
                value={form.tagsInput}
                onChange={(e) => update('tagsInput', e.target.value)}
              />
              <div className="form-help">Comma-separated tags for filtering monitors</div>
            </div>
          </div>

          {form.monitor_type === 'http' && (
            <div className="form-group">
              <label className="form-label">Body Contains (optional)</label>
              <input
                className="form-input"
                type="text"
                placeholder='e.g. "status":"ok"'
                value={form.body_contains}
                onChange={(e) => update('body_contains', e.target.value)}
              />
              <div className="form-help">Response body must contain this string to be considered up</div>
            </div>
          )}
        </div>

        {error && (
          <div style={{
            background: 'var(--danger-bg)', border: '1px solid var(--danger)',
            borderRadius: 'var(--radius)', padding: '12px 16px', marginTop: 12,
            color: 'var(--danger)', fontSize: '0.9rem',
          }}>
            {error}
          </div>
        )}

        <div style={{ display: 'flex', gap: 8, marginTop: 16 }}>
          <button className="btn btn-primary" type="submit" disabled={submitting}>
            {submitting ? 'Creating...' : 'Create Monitor'}
          </button>
          <button className="btn btn-secondary" type="button" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
}
