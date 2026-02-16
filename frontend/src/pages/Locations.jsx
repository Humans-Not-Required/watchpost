import { useState, useEffect } from 'react'
import { getLocations, createLocation, deleteLocation } from '../api'
import { IconGlobe, IconPlus, IconTrash, IconKey, IconCheckCircle, IconClock, IconClipboard, IconAlertCircle } from '../Icons'

function relativeTime(ts) {
  if (!ts) return 'Never';
  const d = new Date(ts.endsWith('Z') ? ts : ts + 'Z');
  const now = new Date();
  const diff = (now - d) / 1000;
  if (diff < 60) return 'Just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function formatTime(ts) {
  if (!ts) return 'Never';
  const d = new Date(ts.endsWith('Z') ? ts : ts + 'Z');
  return d.toLocaleString();
}

export default function Locations() {
  const [locations, setLocations] = useState([]);
  const [loading, setLoading] = useState(true);
  const [showAdd, setShowAdd] = useState(false);
  const [adminKey, setAdminKey] = useState(() => {
    try { return localStorage.getItem('watchpost_admin_key') || ''; } catch { return ''; }
  });
  const [showKeyInput, setShowKeyInput] = useState(false);
  const [newName, setNewName] = useState('');
  const [newRegion, setNewRegion] = useState('');
  const [adding, setAdding] = useState(false);
  const [deleting, setDeleting] = useState(null);
  const [error, setError] = useState(null);
  const [createdProbeKey, setCreatedProbeKey] = useState(null);
  const [copied, setCopied] = useState(null);

  const loadLocations = async () => {
    try {
      const data = await getLocations();
      setLocations(data);
    } catch (err) {
      // silent
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { loadLocations(); }, []);

  const handleSaveAdminKey = () => {
    try { localStorage.setItem('watchpost_admin_key', adminKey); } catch { /* silent */ }
    setShowKeyInput(false);
  };

  const handleAdd = async () => {
    if (!newName.trim()) {
      setError('Location name is required');
      return;
    }
    if (!adminKey.trim()) {
      setError('Admin key is required to manage locations');
      return;
    }
    setAdding(true);
    setError(null);
    try {
      const data = { name: newName.trim() };
      if (newRegion.trim()) data.region = newRegion.trim();
      const result = await createLocation(data, adminKey);
      setCreatedProbeKey(result.probe_key);
      setNewName('');
      setNewRegion('');
      await loadLocations();
    } catch (err) {
      setError(err.message);
    } finally {
      setAdding(false);
    }
  };

  const handleDelete = async (id, name) => {
    if (!adminKey.trim()) {
      alert('Admin key required to delete locations');
      return;
    }
    if (!confirm(`Delete location "${name}"? This cannot be undone.`)) return;
    setDeleting(id);
    try {
      await deleteLocation(id, adminKey);
      await loadLocations();
    } catch (err) {
      alert(`Failed to delete: ${err.message}`);
    } finally {
      setDeleting(null);
    }
  };

  const copy = (text, key) => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(key);
      setTimeout(() => setCopied(null), 2000);
    });
  };

  if (loading) {
    return (
      <div style={{ padding: '24px 0' }}>
        <h2 style={{ fontSize: '1.3rem', marginBottom: 16 }}><IconGlobe size={18} style={{ marginRight: 8 }} />Check Locations</h2>
        <div style={{ color: 'var(--text-muted)' }}>Loading...</div>
      </div>
    );
  }

  return (
    <div style={{ padding: '24px 0' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 20, flexWrap: 'wrap', gap: 12 }}>
        <div>
          <h2 style={{ fontSize: '1.3rem', margin: 0 }}><IconGlobe size={18} style={{ marginRight: 8 }} />Check Locations</h2>
          <p style={{ color: 'var(--text-muted)', fontSize: '0.85rem', margin: '4px 0 0' }}>
            Manage remote probe locations for multi-region monitoring
          </p>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <button
            className="btn btn-secondary"
            style={{ fontSize: '0.85rem', padding: '8px 14px' }}
            onClick={() => setShowKeyInput(!showKeyInput)}
          >
            <IconKey size={14} /> {adminKey ? 'Change Key' : 'Set Admin Key'}
          </button>
          {adminKey && (
            <button
              className="btn btn-primary"
              style={{ fontSize: '0.85rem', padding: '8px 14px' }}
              onClick={() => { setShowAdd(true); setCreatedProbeKey(null); setError(null); }}
            >
              <IconPlus size={14} /> Add Location
            </button>
          )}
        </div>
      </div>

      {/* Admin key input */}
      {showKeyInput && (
        <div className="card" style={{ borderColor: 'var(--accent)', marginBottom: 16 }}>
          <div style={{ fontSize: '0.9rem', fontWeight: 600, marginBottom: 8 }}><IconKey size={14} style={{ marginRight: 6 }} />Admin Key</div>
          <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginBottom: 12 }}>
            The admin key was printed when the server first started. It's required to add/remove check locations.
          </div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <input
              className="form-input"
              type="password"
              style={{ flex: 1, fontFamily: 'monospace', fontSize: '0.85rem' }}
              placeholder="Admin key..."
              value={adminKey}
              onChange={(e) => setAdminKey(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSaveAdminKey()}
            />
            <button className="btn btn-primary" style={{ fontSize: '0.85rem', padding: '8px 14px' }} onClick={handleSaveAdminKey}>
              Save
            </button>
            <button className="btn btn-secondary" style={{ fontSize: '0.85rem', padding: '8px 14px' }} onClick={() => setShowKeyInput(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Add location form */}
      {showAdd && (
        <div className="card" style={{ borderColor: 'var(--accent)', marginBottom: 16 }}>
          <h3 style={{ fontSize: '1rem', fontWeight: 600, marginBottom: 12 }}>
            <IconPlus size={14} style={{ marginRight: 6 }} />Register Check Location
          </h3>

          {error && (
            <div style={{ background: 'var(--danger-bg)', border: '1px solid var(--danger)', borderRadius: 'var(--radius)', padding: '8px 12px', marginBottom: 12, fontSize: '0.85rem', color: 'var(--danger)' }}>
              {error}
            </div>
          )}

          {createdProbeKey ? (
            <div>
              <div style={{
                background: 'rgba(0,212,170,0.08)',
                border: '1px solid var(--success)',
                borderRadius: 'var(--radius)',
                padding: 16,
                marginBottom: 16,
              }}>
                <div style={{ fontWeight: 600, marginBottom: 8, color: 'var(--success)' }}>
                  <IconCheckCircle size={14} /> Location created!
                </div>
                <div style={{ fontSize: '0.85rem', color: 'var(--text-secondary)', marginBottom: 12 }}>
                  Save this probe key — it's shown only once. Remote agents use it to submit check results.
                </div>
                <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                  <code style={{
                    flex: 1,
                    background: 'var(--bg-secondary)',
                    padding: '10px 14px',
                    borderRadius: 6,
                    fontFamily: 'monospace',
                    fontSize: '0.85rem',
                    wordBreak: 'break-all',
                    border: '1px solid var(--border)',
                  }}>
                    {createdProbeKey}
                  </code>
                  <button
                    className="btn btn-secondary"
                    style={{ fontSize: '0.85rem', padding: '8px 14px', flexShrink: 0 }}
                    onClick={() => copy(createdProbeKey, 'probe')}
                  >
                    {copied === 'probe' ? <><IconCheckCircle size={12} /> Copied</> : <><IconClipboard size={12} /> Copy</>}
                  </button>
                </div>
                <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginTop: 12 }}>
                  <strong>Usage:</strong> POST /api/v1/probe with <code>Authorization: Bearer &lt;probe_key&gt;</code>
                </div>
              </div>
              <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
                <button className="btn btn-secondary" onClick={() => { setShowAdd(false); setCreatedProbeKey(null); }}>
                  Done
                </button>
                <button className="btn btn-primary" onClick={() => { setCreatedProbeKey(null); setError(null); }}>
                  Add Another
                </button>
              </div>
            </div>
          ) : (
            <>
              <div className="form-row">
                <div className="form-group">
                  <label className="form-label">Name *</label>
                  <input
                    className="form-input"
                    value={newName}
                    onChange={(e) => setNewName(e.target.value)}
                    placeholder="e.g. US East, EU West, Asia Pacific"
                    autoFocus
                  />
                </div>
                <div className="form-group">
                  <label className="form-label">Region</label>
                  <input
                    className="form-input"
                    value={newRegion}
                    onChange={(e) => setNewRegion(e.target.value)}
                    placeholder="e.g. us-east-1, eu-west-1"
                  />
                  <div className="form-help">Optional — helps identify geographic location</div>
                </div>
              </div>

              <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
                <button className="btn btn-secondary" onClick={() => { setShowAdd(false); setError(null); }}>
                  Cancel
                </button>
                <button
                  className="btn btn-primary"
                  disabled={adding || !newName.trim()}
                  onClick={handleAdd}
                >
                  {adding ? 'Creating...' : <><IconGlobe size={14} /> Create Location</>}
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {/* Location list */}
      {locations.length === 0 ? (
        <div className="empty-state" style={{ marginTop: 32 }}>
          <IconGlobe size={48} style={{ color: 'var(--text-muted)', marginBottom: 12 }} />
          <h3 style={{ color: 'var(--text-secondary)' }}>No check locations</h3>
          <p style={{ color: 'var(--text-muted)', fontSize: '0.9rem', maxWidth: 420 }}>
            Check locations allow remote agents to submit probe results for distributed monitoring.
            Add your first location to get started.
          </p>
        </div>
      ) : (
        <div>
          {locations.map((loc) => (
            <div key={loc.id} className="card" style={{ padding: 16, marginBottom: 8 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <span style={{ fontWeight: 600, fontSize: '0.95rem' }}>
                      <IconGlobe size={14} style={{ marginRight: 4 }} />{loc.name}
                    </span>
                    {loc.region && (
                      <span style={{
                        fontSize: '0.75rem',
                        padding: '2px 8px',
                        borderRadius: 12,
                        background: 'var(--accent-bg, rgba(0,212,170,0.1))',
                        color: 'var(--accent)',
                        fontWeight: 500,
                      }}>
                        {loc.region}
                      </span>
                    )}
                    {(() => {
                      const healthColors = {
                        healthy: { bg: 'rgba(0,212,170,0.1)', fg: '#00d4aa', label: '● Healthy' },
                        new: { bg: 'rgba(96,165,250,0.1)', fg: '#60a5fa', label: '◌ New' },
                        stale: { bg: 'rgba(251,191,36,0.1)', fg: '#fbbf24', label: '⚠ Stale' },
                        disabled: { bg: 'rgba(239,68,68,0.1)', fg: '#ef4444', label: '✕ Disabled' },
                      };
                      const h = healthColors[loc.health_status] || healthColors.disabled;
                      return (
                        <span style={{
                          fontSize: '0.75rem',
                          padding: '2px 8px',
                          borderRadius: 12,
                          background: h.bg,
                          color: h.fg,
                          fontWeight: 500,
                        }}>
                          {h.label}
                        </span>
                      );
                    })()}
                  </div>
                  <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)', marginTop: 6, display: 'flex', gap: 16, flexWrap: 'wrap' }}>
                    <span title={formatTime(loc.last_seen_at)}>
                      <IconClock size={11} style={{ marginRight: 3 }} />
                      Last seen: {loc.last_seen_at ? relativeTime(loc.last_seen_at) : 'Never'}
                    </span>
                    <span title={formatTime(loc.created_at)}>
                      Created: {relativeTime(loc.created_at)}
                    </span>
                    <span style={{ fontFamily: 'monospace', fontSize: '0.75rem', color: 'var(--text-muted)' }}>
                      {loc.id.slice(0, 8)}…
                    </span>
                  </div>
                </div>
                {adminKey && (
                  <button
                    className="btn btn-danger"
                    style={{ fontSize: '0.8rem', padding: '6px 12px', flexShrink: 0, marginLeft: 12 }}
                    disabled={deleting === loc.id}
                    onClick={() => handleDelete(loc.id, loc.name)}
                  >
                    {deleting === loc.id ? '...' : <><IconTrash size={12} /> Delete</>}
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Help section */}
      <div className="card" style={{ marginTop: 24, background: 'var(--subtle-bg)' }}>
        <h4 style={{ fontSize: '0.9rem', fontWeight: 600, marginBottom: 10 }}>How Multi-Region Monitoring Works</h4>
        <ol style={{ color: 'var(--text-secondary)', fontSize: '0.85rem', lineHeight: 1.7, paddingLeft: 20, margin: 0 }}>
          <li>Register check locations here (each gets a unique probe key)</li>
          <li>Deploy remote probe agents that run checks from each location</li>
          <li>Agents submit results via <code>POST /api/v1/probe</code> with their probe key</li>
          <li>View per-location status on monitor detail pages</li>
          <li>Enable <strong>consensus threshold</strong> on monitors to require N locations to agree before declaring down</li>
        </ol>
      </div>
    </div>
  );
}
