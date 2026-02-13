import { useState, useRef } from 'react'
import { bulkCreateMonitors } from '../api'
import { IconCheckCircle, IconAlertTriangle, IconLink, IconClipboard, IconFolder, IconFileText, IconLock, IconGlobe } from '../Icons'

const EXAMPLE_JSON = `[
  {
    "name": "Production API",
    "url": "https://api.example.com/health",
    "method": "GET",
    "interval_seconds": 60,
    "is_public": true,
    "tags": ["prod", "api"]
  },
  {
    "name": "Staging Frontend",
    "url": "https://staging.example.com",
    "interval_seconds": 600,
    "is_public": false,
    "tags": ["staging"]
  }
]`;

function validateMonitors(monitors) {
  const errors = [];
  if (!Array.isArray(monitors)) {
    return [{ index: -1, error: 'Input must be a JSON array of monitors' }];
  }
  if (monitors.length === 0) {
    return [{ index: -1, error: 'Array is empty — add at least one monitor' }];
  }
  if (monitors.length > 50) {
    return [{ index: -1, error: `Too many monitors (${monitors.length}). Maximum is 50 per request.` }];
  }
  monitors.forEach((m, i) => {
    if (!m.name || !m.name.trim()) errors.push({ index: i, error: `Monitor ${i + 1}: name is required` });
    if (!m.url || !m.url.trim()) errors.push({ index: i, error: `Monitor ${i + 1}: url is required` });
    if (m.method && !['GET', 'HEAD', 'POST'].includes(m.method.toUpperCase())) {
      errors.push({ index: i, error: `Monitor ${i + 1}: method must be GET, HEAD, or POST` });
    }
    if (m.interval_seconds !== undefined && m.interval_seconds < 600) {
      errors.push({ index: i, error: `Monitor ${i + 1}: interval must be ≥ 600s (10 min)` });
    }
  });
  return errors;
}

export default function BulkImport({ onDone, onCancel }) {
  const [jsonText, setJsonText] = useState('');
  const [parsed, setParsed] = useState(null);
  const [parseError, setParseError] = useState(null);
  const [validationErrors, setValidationErrors] = useState([]);
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState(null);
  const [submitError, setSubmitError] = useState(null);
  const fileRef = useRef(null);

  const handleParse = (text) => {
    setJsonText(text);
    setParsed(null);
    setParseError(null);
    setValidationErrors([]);
    setResult(null);
    setSubmitError(null);

    if (!text.trim()) return;

    try {
      let data = JSON.parse(text);
      // Accept { monitors: [...] } or [...]
      if (data && !Array.isArray(data) && Array.isArray(data.monitors)) {
        data = data.monitors;
      }
      const errs = validateMonitors(data);
      if (errs.length > 0) {
        setValidationErrors(errs);
        setParsed(data);
      } else {
        setParsed(data);
      }
    } catch (e) {
      setParseError(`Invalid JSON: ${e.message}`);
    }
  };

  const handleFileUpload = (e) => {
    const file = e.target.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => handleParse(ev.target.result);
    reader.readAsText(file);
  };

  const handleSubmit = async () => {
    if (!parsed || validationErrors.length > 0) return;
    setSubmitting(true);
    setSubmitError(null);

    try {
      const res = await bulkCreateMonitors(parsed);
      setResult(res);
    } catch (err) {
      setSubmitError(err.message);
    } finally {
      setSubmitting(false);
    }
  };

  const loadExample = () => {
    handleParse(EXAMPLE_JSON);
  };

  // Result view
  if (result) {
    return (
      <div style={{ marginTop: 24 }}>
        <h2 className="section-title">
          {result.failed === 0
            ? <span style={{ color: 'var(--success)' }}><IconCheckCircle size={16} style={{ marginRight: 6 }} />All {result.succeeded} Monitors Created!</span>
            : <span style={{ color: 'var(--warning)' }}><IconAlertTriangle size={16} style={{ marginRight: 6 }} />{result.succeeded}/{result.total} Created ({result.failed} failed)</span>
          }
        </h2>

        {result.created.length > 0 && (() => {
          // Auto-save all keys to localStorage
          result.created.forEach(item => {
            try { localStorage.setItem(`watchpost_key_${item.monitor.id}`, item.manage_key); } catch (e) { /* silent */ }
          });

          return (
            <div className="card" style={{ marginTop: 16 }}>
              <div className="card-header">
                <span className="card-title">Created Monitors</span>
              </div>
              <div className="manage-key-banner" style={{ marginBottom: 16 }}>
                <div style={{ fontWeight: 700, marginBottom: 8, color: 'var(--accent)' }}>
                  <IconLink size={14} style={{ marginRight: 6 }} />Bookmark these manage links — they include your keys
                </div>
                <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)' }}>
                  Keys are also saved in this browser automatically.
                </div>
              </div>
              <div style={{ overflowX: 'auto' }}>
                <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '0.85rem' }}>
                  <thead>
                    <tr style={{ borderBottom: '1px solid var(--border)' }}>
                      <th style={thStyle}>Name</th>
                      <th style={thStyle}>Target URL</th>
                      <th style={thStyle}>Manage Link</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.created.map((item) => {
                      const manageLink = `${window.location.origin}/#/monitor/${item.monitor.id}?key=${item.manage_key}`;
                      return (
                        <tr key={item.monitor.id} style={{ borderBottom: '1px solid var(--border)' }}>
                          <td style={tdStyle}>{item.monitor.name}</td>
                          <td style={{ ...tdStyle, maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                            {item.monitor.url}
                          </td>
                          <td style={tdStyle}>
                            <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                              <a href={`#/monitor/${item.monitor.id}?key=${item.manage_key}`} style={{ color: 'var(--accent)', fontSize: '0.8rem' }}>
                                Open →
                              </a>
                              <button
                                className="btn btn-secondary"
                                style={{ fontSize: '0.7rem', padding: '2px 8px' }}
                                onClick={() => navigator.clipboard.writeText(manageLink)}
                              >
                                <IconClipboard size={12} />
                              </button>
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>

              <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
                <button
                  className="btn btn-secondary"
                  style={{ fontSize: '0.8rem' }}
                  onClick={() => {
                    const data = result.created.map(c => ({
                      name: c.monitor.name,
                      id: c.monitor.id,
                      manage_link: `${window.location.origin}/#/monitor/${c.monitor.id}?key=${c.manage_key}`,
                      manage_key: c.manage_key,
                    }));
                    navigator.clipboard.writeText(JSON.stringify(data, null, 2));
                  }}
                >
                  <IconClipboard size={14} style={{ marginRight: 4 }} />Copy All as JSON
                </button>
              </div>
            </div>
          );
        })()}

        {result.errors.length > 0 && (
          <div className="card" style={{ marginTop: 16, borderColor: 'var(--danger)' }}>
            <div className="card-header">
              <span className="card-title" style={{ color: 'var(--danger)' }}>Failed</span>
            </div>
            {result.errors.map((err, i) => (
              <div key={i} style={{ padding: '8px 0', borderBottom: '1px solid var(--border)', fontSize: '0.85rem' }}>
                <span style={{ color: 'var(--text-muted)' }}>Monitor #{err.index + 1}:</span>{' '}
                <span style={{ color: 'var(--danger)' }}>{err.error}</span>
              </div>
            ))}
          </div>
        )}

        <div style={{ display: 'flex', gap: 8, marginTop: 16 }}>
          <button className="btn btn-primary" onClick={() => onDone ? onDone() : onCancel()}>
            Back to Status
          </button>
          <button className="btn btn-secondary" onClick={() => { setResult(null); setJsonText(''); setParsed(null); }}>
            Import More
          </button>
        </div>
      </div>
    );
  }

  return (
    <div style={{ marginTop: 24 }}>
      <h2 className="section-title">Bulk Import Monitors</h2>
      <p style={{ color: 'var(--text-muted)', marginBottom: 16, fontSize: '0.9rem' }}>
        Create up to 50 monitors at once by pasting JSON or uploading a file. 
        Use the <strong>export</strong> API endpoint to get configs from existing monitors.
      </p>

      <div className="card">
        <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
          <button
            className="btn btn-secondary"
            style={{ fontSize: '0.8rem' }}
            onClick={() => fileRef.current?.click()}
          >
            <IconFolder size={14} style={{ marginRight: 4 }} />Upload JSON File
          </button>
          <button
            className="btn btn-secondary"
            style={{ fontSize: '0.8rem' }}
            onClick={loadExample}
          >
            <IconFileText size={14} style={{ marginRight: 4 }} />Load Example
          </button>
          <input
            ref={fileRef}
            type="file"
            accept=".json,application/json"
            style={{ display: 'none' }}
            onChange={handleFileUpload}
          />
        </div>

        <div className="form-group">
          <label className="form-label">Monitor Config (JSON array)</label>
          <textarea
            className="form-input"
            style={{
              minHeight: 240,
              fontFamily: 'monospace',
              fontSize: '0.8rem',
              lineHeight: 1.5,
              resize: 'vertical',
            }}
            placeholder={`Paste a JSON array of monitors, or click "Load Example" to see the format...`}
            value={jsonText}
            onChange={(e) => handleParse(e.target.value)}
          />
        </div>

        {parseError && (
          <div style={{
            background: 'var(--danger-bg)', border: '1px solid var(--danger)',
            borderRadius: 'var(--radius)', padding: '10px 14px', marginTop: 8,
            color: 'var(--danger)', fontSize: '0.85rem',
          }}>
            {parseError}
          </div>
        )}

        {validationErrors.length > 0 && (
          <div style={{
            background: 'var(--danger-bg)', border: '1px solid var(--danger)',
            borderRadius: 'var(--radius)', padding: '10px 14px', marginTop: 8,
            color: 'var(--danger)', fontSize: '0.85rem',
          }}>
            {validationErrors.map((err, i) => (
              <div key={i}>{err.error}</div>
            ))}
          </div>
        )}

        {parsed && validationErrors.length === 0 && (
          <div style={{
            background: 'var(--bg-secondary)', border: '1px solid var(--border)',
            borderRadius: 'var(--radius)', padding: '12px 16px', marginTop: 12,
          }}>
            <div style={{ fontWeight: 600, marginBottom: 8, fontSize: '0.9rem' }}>
              <IconCheckCircle size={14} style={{ marginRight: 6 }} />{parsed.length} monitor{parsed.length !== 1 ? 's' : ''} ready to import
            </div>
            <div style={{ overflowX: 'auto' }}>
              <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '0.8rem' }}>
                <thead>
                  <tr style={{ borderBottom: '1px solid var(--border)' }}>
                    <th style={thStyle}>#</th>
                    <th style={thStyle}>Name</th>
                    <th style={thStyle}>URL</th>
                    <th style={thStyle}>Method</th>
                    <th style={thStyle}>Interval</th>
                    <th style={thStyle}>Public</th>
                    <th style={thStyle}>Tags</th>
                  </tr>
                </thead>
                <tbody>
                  {parsed.map((m, i) => (
                    <tr key={i} style={{ borderBottom: '1px solid var(--border)' }}>
                      <td style={tdStyle}>{i + 1}</td>
                      <td style={tdStyle}>{m.name || '—'}</td>
                      <td style={{ ...tdStyle, maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {m.url || '—'}
                      </td>
                      <td style={tdStyle}>{(m.method || 'GET').toUpperCase()}</td>
                      <td style={tdStyle}>{m.interval_seconds || 600}s</td>
                      <td style={tdStyle}>{m.is_public === false ? <IconLock size={14} /> : <IconGlobe size={14} />}</td>
                      <td style={tdStyle}>
                        {(m.tags || []).length > 0
                          ? m.tags.map(t => <span key={t} className="tag-badge" style={{ fontSize: '0.7rem', marginRight: 4 }}>{t}</span>)
                          : '—'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </div>

      {submitError && (
        <div style={{
          background: 'var(--danger-bg)', border: '1px solid var(--danger)',
          borderRadius: 'var(--radius)', padding: '12px 16px', marginTop: 12,
          color: 'var(--danger)', fontSize: '0.9rem',
        }}>
          {submitError}
        </div>
      )}

      <div style={{ display: 'flex', gap: 8, marginTop: 16 }}>
        <button
          className="btn btn-primary"
          onClick={handleSubmit}
          disabled={!parsed || validationErrors.length > 0 || submitting}
        >
          {submitting ? 'Importing...' : `Import ${parsed ? parsed.length : 0} Monitor${parsed?.length !== 1 ? 's' : ''}`}
        </button>
        <button className="btn btn-secondary" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

const thStyle = { textAlign: 'left', padding: '6px 10px', color: 'var(--text-muted)', fontWeight: 600 };
const tdStyle = { padding: '6px 10px', color: 'var(--text-secondary)' };
