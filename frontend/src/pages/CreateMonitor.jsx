import { useState } from 'react'
import { createMonitor } from '../api'

export default function CreateMonitor({ onCreated, onCancel }) {
  const [form, setForm] = useState({
    name: '',
    url: '',
    method: 'GET',
    interval_seconds: 300,
    timeout_ms: 10000,
    expected_status: 200,
    body_contains: '',
    is_public: true,
    confirmation_threshold: 2,
  });
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
        method: form.method,
        interval_seconds: parseInt(form.interval_seconds, 10),
        timeout_ms: parseInt(form.timeout_ms, 10),
        expected_status: parseInt(form.expected_status, 10),
        is_public: form.is_public,
        confirmation_threshold: parseInt(form.confirmation_threshold, 10),
      };
      if (form.body_contains.trim()) {
        payload.body_contains = form.body_contains.trim();
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
    return (
      <div style={{ marginTop: 24 }}>
        <h2 className="section-title" style={{ color: 'var(--success)' }}>
          ✅ Monitor Created!
        </h2>

        <div className="manage-key-banner">
          <div style={{ fontWeight: 700, marginBottom: 8, color: 'var(--warning)' }}>
            ⚠️ Save your manage key — it's only shown once!
          </div>
          <code>{result.manage_key}</code>
          <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginTop: 8 }}>
            Use this key to update, delete, pause, or manage notifications for this monitor.
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
          <div className="monitor-stats">
            <div className="monitor-stat">
              <span className="monitor-stat-label">URL</span>
              <span className="monitor-stat-value" style={{ fontSize: '0.85rem' }}>
                {result.monitor.url}
              </span>
            </div>
            <div className="monitor-stat">
              <span className="monitor-stat-label">Method</span>
              <span className="monitor-stat-value">{result.monitor.method}</span>
            </div>
            <div className="monitor-stat">
              <span className="monitor-stat-label">Interval</span>
              <span className="monitor-stat-value">{result.monitor.interval_seconds}s</span>
            </div>
          </div>
        </div>

        <div style={{ marginTop: 16, display: 'flex', gap: 8 }}>
          <button className="btn btn-primary" onClick={() => onCreated(result.monitor.id)}>
            View Monitor →
          </button>
          <button className="btn btn-secondary" onClick={onCancel}>
            Back to Status
          </button>
        </div>

        <div style={{ marginTop: 16, padding: 16, background: 'var(--bg-secondary)', borderRadius: 'var(--radius)', fontSize: '0.85rem' }}>
          <div style={{ color: 'var(--text-muted)', marginBottom: 8 }}>Quick Reference (API)</div>
          <div style={{ color: 'var(--text-secondary)' }}>
            <div>View: <code style={{ color: 'var(--accent)' }}>GET {result.api_base}</code></div>
            <div>Dashboard: <code style={{ color: 'var(--accent)' }}>{result.view_url}</code></div>
            <div>Manage: <code style={{ color: 'var(--accent)' }}>{result.manage_url}</code></div>
          </div>
        </div>
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
            <label className="form-label">URL to Monitor *</label>
            <input
              className="form-input"
              type="url"
              placeholder="https://api.example.com/health"
              value={form.url}
              onChange={(e) => update('url', e.target.value)}
              required
            />
          </div>

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

          <div className="form-row">
            <div className="form-group">
              <label className="form-label">Check Interval (seconds)</label>
              <input
                className="form-input"
                type="number"
                min="30"
                value={form.interval_seconds}
                onChange={(e) => update('interval_seconds', e.target.value)}
              />
              <div className="form-help">Minimum: 30 seconds</div>
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
