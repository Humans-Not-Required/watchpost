import { useState, useEffect, useCallback } from 'react';
import { getStatusPages, getStatusPageDetail, createStatusPageApi, updateStatusPage, deleteStatusPage, addPageMonitors, removePageMonitor, getMonitors } from '../api';
import { IconGlobe, IconWrench, IconTrash, IconEdit, IconKey, IconPlus, IconX } from '../Icons';

const inputStyle = { width: '100%', padding: '8px 12px', background: '#0f172a', border: '1px solid #334155', borderRadius: 6, color: '#e2e8f0', fontSize: '0.9rem', boxSizing: 'border-box' };
const labelStyle = { display: 'block', fontSize: '0.8rem', color: '#94a3b8', marginBottom: 4 };
const fieldStyle = { marginBottom: 12 };

// ── Status Page List View ──
function StatusPageList({ onSelect, onCreateNew }) {
  const [pages, setPages] = useState([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getStatusPages()
      .then(setPages)
      .catch(() => setPages([]))
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return <div style={{ padding: '40px 0', textAlign: 'center', color: '#94a3b8' }}>Loading status pages...</div>;
  }

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 24 }}>
        <h2 style={{ margin: 0, fontSize: '1.2rem' }}>Status Pages</h2>
        <button className="btn btn-primary" onClick={onCreateNew}>+ New Status Page</button>
      </div>

      {pages.length === 0 ? (
        <div style={{ textAlign: 'center', padding: '60px 20px', color: '#64748b' }}>
          <IconGlobe size={40} />
          <p style={{ marginTop: 16, fontSize: '0.95rem' }}>No status pages yet</p>
          <p style={{ fontSize: '0.8rem' }}>Create a status page to group monitors and share a branded view.</p>
          <button className="btn btn-primary" onClick={onCreateNew} style={{ marginTop: 16 }}>Create First Status Page</button>
        </div>
      ) : (
        <div style={{ display: 'grid', gap: 12 }}>
          {pages.map(page => (
            <div
              key={page.id}
              className="card"
              style={{ padding: '16px 20px', cursor: 'pointer' }}
              onClick={() => onSelect(page.slug)}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <div>
                  <div style={{ fontWeight: 600, fontSize: '1rem' }}>
                    {page.logo_url && <img src={page.logo_url} alt="" style={{ width: 20, height: 20, borderRadius: 4, marginRight: 8, verticalAlign: 'middle' }} />}
                    {page.title}
                  </div>
                  {page.description && (
                    <div style={{ fontSize: '0.8rem', color: '#94a3b8', marginTop: 4 }}>{page.description}</div>
                  )}
                  <div style={{ fontSize: '0.75rem', color: '#64748b', marginTop: 6 }}>
                    /{page.slug} · {page.monitor_count} monitor{page.monitor_count !== 1 ? 's' : ''}
                    {page.custom_domain && <span> · {page.custom_domain}</span>}
                  </div>
                </div>
                <div style={{ fontSize: '0.7rem', color: '#475569' }}>→</div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Create Status Page Form ──
function CreateStatusPage({ onCreated, onCancel }) {
  const [slug, setSlug] = useState('');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [logoUrl, setLogoUrl] = useState('');
  const [customDomain, setCustomDomain] = useState('');
  const [isPublic, setIsPublic] = useState(true);
  const [error, setError] = useState(null);
  const [result, setResult] = useState(null);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (e) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const data = {
        slug: slug.trim(),
        title: title.trim(),
        is_public: isPublic,
      };
      if (description.trim()) data.description = description.trim();
      if (logoUrl.trim()) data.logo_url = logoUrl.trim();
      if (customDomain.trim()) data.custom_domain = customDomain.trim();

      const res = await createStatusPageApi(data);
      // Auto-save manage key to localStorage
      if (res.manage_key && res.status_page?.slug) {
        try { localStorage.setItem(`watchpost_page_key_${res.status_page.slug}`, res.manage_key); } catch (e) { /* silent */ }
      }
      setResult(res);
    } catch (err) {
      setError(err.message);
    } finally {
      setSubmitting(false);
    }
  };

  if (result) {
    return (
      <div className="card" style={{ padding: 24 }}>
        <h3 style={{ color: '#00d4aa', marginTop: 0 }}>✅ Status Page Created</h3>
        <p><strong>Title:</strong> {result.status_page.title}</p>
        <p><strong>Slug:</strong> /{result.status_page.slug}</p>
        <div style={{ background: '#1a1a2e', padding: '12px 16px', borderRadius: 8, marginTop: 12, marginBottom: 12 }}>
          <div style={{ fontSize: '0.75rem', color: '#94a3b8', marginBottom: 4 }}>Manage Key (save this — shown once)</div>
          <code style={{ fontSize: '0.85rem', color: '#fbbf24', wordBreak: 'break-all' }}>{result.manage_key}</code>
          <button
            className="btn"
            style={{ marginLeft: 8, fontSize: '0.7rem', padding: '2px 8px' }}
            onClick={() => navigator.clipboard.writeText(result.manage_key)}
          >Copy</button>
        </div>
        <p style={{ fontSize: '0.75rem', color: '#64748b' }}>Key saved to browser storage for this page.</p>
        <div style={{ display: 'flex', gap: 8 }}>
          <button className="btn btn-primary" onClick={() => onCreated(result.status_page.slug)}>View Page</button>
          <button className="btn" onClick={onCancel}>Done</button>
        </div>
      </div>
    );
  }

  return (
    <div>
      <h2 style={{ marginBottom: 16 }}>Create Status Page</h2>
      <form onSubmit={handleSubmit} className="card" style={{ padding: 20 }}>
        <StatusPageForm
          slug={slug} setSlug={setSlug}
          title={title} setTitle={setTitle}
          description={description} setDescription={setDescription}
          logoUrl={logoUrl} setLogoUrl={setLogoUrl}
          customDomain={customDomain} setCustomDomain={setCustomDomain}
          isPublic={isPublic} setIsPublic={setIsPublic}
          showSlug={true}
        />

        {error && <div style={{ color: '#f87171', fontSize: '0.85rem', marginBottom: 12 }}>{error}</div>}

        <div style={{ display: 'flex', gap: 8 }}>
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting ? 'Creating...' : 'Create Status Page'}
          </button>
          <button type="button" className="btn" onClick={onCancel}>Cancel</button>
        </div>
      </form>
    </div>
  );
}

// ── Shared Form Fields ──
function StatusPageForm({ slug, setSlug, title, setTitle, description, setDescription, logoUrl, setLogoUrl, customDomain, setCustomDomain, isPublic, setIsPublic, showSlug }) {
  return (
    <>
      {showSlug && (
        <div style={fieldStyle}>
          <label style={labelStyle}>Slug *</label>
          <input
            type="text"
            value={slug}
            onChange={(e) => setSlug(e.target.value.toLowerCase().replace(/[^a-z0-9_-]/g, ''))}
            placeholder="e.g., production"
            required
            style={inputStyle}
          />
          <div style={{ fontSize: '0.7rem', color: '#64748b', marginTop: 2 }}>URL-safe identifier (a-z, 0-9, hyphens, underscores)</div>
        </div>
      )}

      <div style={fieldStyle}>
        <label style={labelStyle}>Title *</label>
        <input
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="e.g., Production Status"
          required
          style={inputStyle}
        />
      </div>

      <div style={fieldStyle}>
        <label style={labelStyle}>Description</label>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Brief description of this status page"
          rows={2}
          style={{ ...inputStyle, resize: 'vertical' }}
        />
      </div>

      <div style={fieldStyle}>
        <label style={labelStyle}>Logo URL</label>
        <input
          type="url"
          value={logoUrl}
          onChange={(e) => setLogoUrl(e.target.value)}
          placeholder="https://example.com/logo.png"
          style={inputStyle}
        />
      </div>

      <div style={fieldStyle}>
        <label style={labelStyle}>Custom Domain</label>
        <input
          type="text"
          value={customDomain}
          onChange={(e) => setCustomDomain(e.target.value)}
          placeholder="status.example.com"
          style={inputStyle}
        />
        <div style={{ fontSize: '0.7rem', color: '#64748b', marginTop: 2 }}>Optional: point a CNAME to this service</div>
      </div>

      <div style={{ marginBottom: 16 }}>
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}>
          <input
            type="checkbox"
            checked={isPublic}
            onChange={(e) => setIsPublic(e.target.checked)}
          />
          <span style={{ fontSize: '0.85rem' }}>Public (listed on status pages index)</span>
        </label>
      </div>
    </>
  );
}

// ── Manage Key Input ──
function ManageKeyInput({ slug, manageKey, setManageKey }) {
  const [keyInput, setKeyInput] = useState('');
  const [saved, setSaved] = useState(false);

  const handleSave = () => {
    const k = keyInput.trim();
    if (!k) return;
    setManageKey(k);
    try { localStorage.setItem(`watchpost_page_key_${slug}`, k); } catch (e) { /* silent */ }
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const handleClear = () => {
    setManageKey('');
    setKeyInput('');
    try { localStorage.removeItem(`watchpost_page_key_${slug}`); } catch (e) { /* silent */ }
  };

  if (manageKey) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: '0.8rem' }}>
        <IconKey size={14} />
        <span style={{ color: '#00d4aa' }}>Manage key active</span>
        <button className="btn" style={{ fontSize: '0.7rem', padding: '2px 8px' }} onClick={handleClear}>Clear Key</button>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      <IconKey size={14} style={{ color: '#94a3b8', flexShrink: 0 }} />
      <input
        type="password"
        value={keyInput}
        onChange={(e) => setKeyInput(e.target.value)}
        placeholder="Enter manage key to edit..."
        onKeyDown={(e) => e.key === 'Enter' && handleSave()}
        style={{ ...inputStyle, width: 220, fontSize: '0.8rem', padding: '4px 8px' }}
      />
      <button className="btn" style={{ fontSize: '0.7rem', padding: '4px 10px' }} onClick={handleSave}>
        {saved ? '✓' : 'Unlock'}
      </button>
    </div>
  );
}

// ── Edit Status Page Panel ──
function EditStatusPage({ page, slug, manageKey, onUpdated, onCancel }) {
  const [title, setTitle] = useState(page.title || '');
  const [description, setDescription] = useState(page.description || '');
  const [logoUrl, setLogoUrl] = useState(page.logo_url || '');
  const [customDomain, setCustomDomain] = useState(page.custom_domain || '');
  const [isPublic, setIsPublic] = useState(page.is_public !== false);
  const [error, setError] = useState(null);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (e) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const data = { title: title.trim(), is_public: isPublic };
      // Only send changed fields; send null to clear optional fields
      data.description = description.trim() || null;
      data.logo_url = logoUrl.trim() || null;
      data.custom_domain = customDomain.trim() || null;

      await updateStatusPage(slug, data, manageKey);
      onUpdated();
    } catch (err) {
      setError(err.message);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 16 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <h3 style={{ margin: 0, fontSize: '1rem' }}><IconEdit size={16} style={{ marginRight: 6, verticalAlign: 'middle' }} />Edit Status Page</h3>
        <button className="btn" style={{ fontSize: '0.7rem', padding: '2px 8px' }} onClick={onCancel}><IconX size={14} /></button>
      </div>
      <form onSubmit={handleSubmit}>
        <StatusPageForm
          title={title} setTitle={setTitle}
          description={description} setDescription={setDescription}
          logoUrl={logoUrl} setLogoUrl={setLogoUrl}
          customDomain={customDomain} setCustomDomain={setCustomDomain}
          isPublic={isPublic} setIsPublic={setIsPublic}
          showSlug={false}
        />

        {error && <div style={{ color: '#f87171', fontSize: '0.85rem', marginBottom: 12 }}>{error}</div>}

        <div style={{ display: 'flex', gap: 8 }}>
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting ? 'Saving...' : 'Save Changes'}
          </button>
          <button type="button" className="btn" onClick={onCancel}>Cancel</button>
        </div>
      </form>
    </div>
  );
}

// ── Monitor Management Panel ──
function MonitorManager({ slug, pageMonitors, manageKey, onUpdated }) {
  const [allMonitors, setAllMonitors] = useState(null);
  const [showAdd, setShowAdd] = useState(false);
  const [search, setSearch] = useState('');
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState(null);
  const [error, setError] = useState(null);

  const loadMonitors = useCallback(() => {
    getMonitors().then(data => {
      // data might be { monitors: [...] } or just [...]
      setAllMonitors(Array.isArray(data) ? data : (data.monitors || []));
    }).catch(() => setAllMonitors([]));
  }, []);

  useEffect(() => {
    if (showAdd && allMonitors === null) {
      loadMonitors();
    }
  }, [showAdd, allMonitors, loadMonitors]);

  const pageMonitorIds = new Set((pageMonitors || []).map(m => m.id));

  const available = (allMonitors || []).filter(m => !pageMonitorIds.has(m.id));
  const filtered = search.trim()
    ? available.filter(m => m.name.toLowerCase().includes(search.toLowerCase()) || (m.url || '').toLowerCase().includes(search.toLowerCase()))
    : available;

  const handleAdd = async (monitorId) => {
    setError(null);
    setAdding(true);
    try {
      await addPageMonitors(slug, [monitorId], manageKey);
      onUpdated();
      // If only one was left, close the add panel
      if (filtered.length <= 1) setShowAdd(false);
    } catch (err) {
      setError(err.message);
    } finally {
      setAdding(false);
    }
  };

  const handleRemove = async (monitorId) => {
    if (!window.confirm('Remove this monitor from the status page? (The monitor itself is not deleted.)')) return;
    setError(null);
    setRemoving(monitorId);
    try {
      await removePageMonitor(slug, monitorId, manageKey);
      onUpdated();
    } catch (err) {
      setError(err.message);
    } finally {
      setRemoving(null);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 16 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <h3 style={{ margin: 0, fontSize: '1rem' }}>Monitors ({pageMonitors.length})</h3>
        <button className="btn btn-primary" style={{ fontSize: '0.75rem', padding: '4px 12px' }} onClick={() => setShowAdd(!showAdd)}>
          {showAdd ? 'Done' : <><IconPlus size={12} /> Add Monitor</>}
        </button>
      </div>

      {error && <div style={{ color: '#f87171', fontSize: '0.8rem', marginBottom: 8 }}>{error}</div>}

      {/* Current monitors with remove buttons */}
      {pageMonitors.length === 0 ? (
        <div style={{ color: '#64748b', fontSize: '0.85rem', padding: '8px 0' }}>No monitors assigned yet. Add some to populate your status page.</div>
      ) : (
        <div style={{ display: 'grid', gap: 6, marginBottom: showAdd ? 16 : 0 }}>
          {pageMonitors.map(m => {
            const statusColors = { up: '#00d4aa', down: '#ef4444', degraded: '#fbbf24', maintenance: '#8b5cf6', unknown: '#64748b' };
            const color = statusColors[m.current_status] || '#64748b';
            return (
              <div key={m.id} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '8px 12px', background: '#0f172a', borderRadius: 6, border: '1px solid #1e293b' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <div style={{ width: 8, height: 8, borderRadius: '50%', background: color, flexShrink: 0 }} />
                  <span style={{ fontSize: '0.85rem' }}>{m.name}</span>
                  <span style={{ fontSize: '0.7rem', color: '#64748b' }}>{m.uptime_24h?.toFixed(1)}%</span>
                </div>
                <button
                  className="btn"
                  style={{ fontSize: '0.65rem', padding: '2px 6px', color: '#f87171' }}
                  onClick={() => handleRemove(m.id)}
                  disabled={removing === m.id}
                  title="Remove from page"
                >
                  {removing === m.id ? '...' : <IconX size={12} />}
                </button>
              </div>
            );
          })}
        </div>
      )}

      {/* Add monitors panel */}
      {showAdd && (
        <div>
          <div style={{ borderTop: '1px solid #2a2a4a', paddingTop: 12, marginTop: 4 }}>
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search monitors..."
              style={{ ...inputStyle, fontSize: '0.8rem', padding: '6px 10px', marginBottom: 8 }}
            />
            {allMonitors === null ? (
              <div style={{ color: '#94a3b8', fontSize: '0.8rem', padding: 8 }}>Loading monitors...</div>
            ) : filtered.length === 0 ? (
              <div style={{ color: '#64748b', fontSize: '0.8rem', padding: 8 }}>
                {available.length === 0 ? 'All monitors are already on this page.' : 'No matches.'}
              </div>
            ) : (
              <div style={{ maxHeight: 240, overflowY: 'auto', display: 'grid', gap: 4 }}>
                {filtered.slice(0, 50).map(m => {
                  const statusColors = { up: '#00d4aa', down: '#ef4444', degraded: '#fbbf24', maintenance: '#8b5cf6', unknown: '#64748b' };
                  const color = statusColors[m.current_status] || '#64748b';
                  return (
                    <div key={m.id} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '6px 10px', background: '#12122a', borderRadius: 4, border: '1px solid #1e293b' }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <div style={{ width: 8, height: 8, borderRadius: '50%', background: color, flexShrink: 0 }} />
                        <span style={{ fontSize: '0.8rem' }}>{m.name}</span>
                        {m.url && <span style={{ fontSize: '0.65rem', color: '#475569', maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.url}</span>}
                      </div>
                      <button
                        className="btn btn-primary"
                        style={{ fontSize: '0.65rem', padding: '2px 8px' }}
                        onClick={() => handleAdd(m.id)}
                        disabled={adding}
                      >
                        {adding ? '...' : 'Add'}
                      </button>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

// ── Status Page Detail View ──
function StatusPageView({ slug, onBack, onMonitorSelect, onDeleted }) {
  const [page, setPage] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [manageKey, setManageKey] = useState(() => {
    try { return localStorage.getItem(`watchpost_page_key_${slug}`) || ''; } catch (e) { return ''; }
  });
  const [showSettings, setShowSettings] = useState(false);
  const [editing, setEditing] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState(null);

  const load = useCallback(() => {
    getStatusPageDetail(slug)
      .then(setPage)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [slug]);

  useEffect(() => {
    load();
    const interval = setInterval(load, 30000);
    return () => clearInterval(interval);
  }, [load]);

  const handleDelete = async () => {
    setDeleteError(null);
    setDeleting(true);
    try {
      await deleteStatusPage(slug, manageKey);
      try { localStorage.removeItem(`watchpost_page_key_${slug}`); } catch (e) { /* silent */ }
      onDeleted();
    } catch (err) {
      setDeleteError(err.message);
    } finally {
      setDeleting(false);
    }
  };

  if (loading) return <div style={{ textAlign: 'center', padding: 40, color: '#94a3b8' }}>Loading...</div>;
  if (error) return <div style={{ textAlign: 'center', padding: 40, color: '#f87171' }}>{error}</div>;
  if (!page) return null;

  const statusColor = {
    operational: '#00d4aa',
    degraded: '#fbbf24',
    major_outage: '#ef4444',
    unknown: '#64748b',
  }[page.overall] || '#64748b';

  const statusLabel = {
    operational: 'All Systems Operational',
    degraded: 'Partial Degradation',
    major_outage: 'Major Outage',
    unknown: 'Status Unknown',
  }[page.overall] || page.overall;

  // Group monitors by group_name
  const grouped = {};
  const ungrouped = [];
  (page.monitors || []).forEach(m => {
    if (m.group_name) {
      if (!grouped[m.group_name]) grouped[m.group_name] = [];
      grouped[m.group_name].push(m);
    } else {
      ungrouped.push(m);
    }
  });

  return (
    <div>
      {/* Header bar */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <button className="btn" onClick={onBack} style={{ fontSize: '0.8rem' }}>← Back to Pages</button>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {manageKey && (
            <button
              className="btn"
              style={{ fontSize: '0.75rem', padding: '4px 10px' }}
              onClick={() => { setShowSettings(!showSettings); setEditing(false); setConfirmDelete(false); }}
            >
              <IconWrench size={14} style={{ marginRight: 4, verticalAlign: 'middle' }} />
              Settings
            </button>
          )}
        </div>
      </div>

      {/* Manage key input */}
      <div style={{ marginBottom: 16 }}>
        <ManageKeyInput slug={slug} manageKey={manageKey} setManageKey={setManageKey} />
      </div>

      {/* Settings panel (edit, monitor management, delete) */}
      {showSettings && manageKey && (
        <>
          {/* Edit form */}
          {editing ? (
            <EditStatusPage
              page={page}
              slug={slug}
              manageKey={manageKey}
              onUpdated={() => { setEditing(false); load(); }}
              onCancel={() => setEditing(false)}
            />
          ) : (
            <div className="card" style={{ padding: '12px 20px', marginBottom: 16 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span style={{ fontSize: '0.85rem', color: '#94a3b8' }}>
                  /{page.slug}
                  {page.custom_domain && <span> · {page.custom_domain}</span>}
                  {page.is_public === false && <span style={{ color: '#fbbf24', marginLeft: 8 }}>Private</span>}
                </span>
                <button className="btn" style={{ fontSize: '0.75rem', padding: '4px 10px' }} onClick={() => setEditing(true)}>
                  <IconEdit size={14} style={{ marginRight: 4, verticalAlign: 'middle' }} /> Edit
                </button>
              </div>
            </div>
          )}

          {/* Monitor management */}
          <MonitorManager
            slug={slug}
            pageMonitors={page.monitors || []}
            manageKey={manageKey}
            onUpdated={load}
          />

          {/* Delete zone */}
          <div className="card" style={{ padding: '12px 20px', marginBottom: 16, borderLeft: '3px solid #ef4444' }}>
            {!confirmDelete ? (
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span style={{ fontSize: '0.85rem', color: '#94a3b8' }}>Danger zone</span>
                <button
                  className="btn"
                  style={{ fontSize: '0.75rem', padding: '4px 10px', color: '#f87171', borderColor: '#7f1d1d' }}
                  onClick={() => setConfirmDelete(true)}
                >
                  <IconTrash size={14} style={{ marginRight: 4, verticalAlign: 'middle' }} /> Delete Page
                </button>
              </div>
            ) : (
              <div>
                <p style={{ fontSize: '0.85rem', color: '#f87171', marginTop: 0 }}>
                  Delete <strong>"{page.title}"</strong>? This removes the status page and its monitor assignments. Monitors themselves are not deleted.
                </p>
                {deleteError && <div style={{ color: '#f87171', fontSize: '0.8rem', marginBottom: 8 }}>{deleteError}</div>}
                <div style={{ display: 'flex', gap: 8 }}>
                  <button
                    className="btn"
                    style={{ fontSize: '0.75rem', padding: '4px 12px', background: '#7f1d1d', color: '#fca5a5', borderColor: '#991b1b' }}
                    onClick={handleDelete}
                    disabled={deleting}
                  >
                    {deleting ? 'Deleting...' : 'Yes, Delete'}
                  </button>
                  <button className="btn" style={{ fontSize: '0.75rem', padding: '4px 12px' }} onClick={() => setConfirmDelete(false)}>Cancel</button>
                </div>
              </div>
            )}
          </div>
        </>
      )}

      {/* Page header */}
      <div style={{ textAlign: 'center', marginBottom: 24 }}>
        {page.logo_url && <img src={page.logo_url} alt="" style={{ width: 48, height: 48, borderRadius: 8, marginBottom: 8 }} />}
        <h2 style={{ margin: '4px 0' }}>{page.title}</h2>
        {page.description && <p style={{ color: '#94a3b8', fontSize: '0.85rem', marginTop: 4 }}>{page.description}</p>}
      </div>

      {/* Overall status banner */}
      <div className="card" style={{ padding: '16px 20px', marginBottom: 20, borderLeft: `4px solid ${statusColor}` }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={{ width: 12, height: 12, borderRadius: '50%', background: statusColor }} />
          <span style={{ fontWeight: 600, color: statusColor }}>{statusLabel}</span>
        </div>
      </div>

      {(page.monitors || []).length === 0 ? (
        <div style={{ textAlign: 'center', padding: 40, color: '#64748b' }}>
          <p>No monitors assigned to this page yet.</p>
          {manageKey && <p style={{ fontSize: '0.8rem' }}>Open Settings to add monitors.</p>}
        </div>
      ) : (
        <>
          {/* Grouped monitors */}
          {Object.entries(grouped).map(([group, monitors]) => (
            <div key={group} style={{ marginBottom: 16 }}>
              <div style={{ fontSize: '0.75rem', color: '#94a3b8', fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 8 }}>{group}</div>
              {monitors.map(m => <MonitorCard key={m.id} monitor={m} onClick={() => onMonitorSelect(m.id)} />)}
            </div>
          ))}
          {/* Ungrouped monitors */}
          {ungrouped.map(m => <MonitorCard key={m.id} monitor={m} onClick={() => onMonitorSelect(m.id)} />)}
        </>
      )}

      {page.custom_domain && (
        <div style={{ textAlign: 'center', marginTop: 24, fontSize: '0.7rem', color: '#475569' }}>
          Custom domain: {page.custom_domain}
        </div>
      )}
    </div>
  );
}

function MonitorCard({ monitor, onClick }) {
  const m = monitor;
  const statusColors = { up: '#00d4aa', down: '#ef4444', degraded: '#fbbf24', maintenance: '#8b5cf6', unknown: '#64748b' };
  const color = statusColors[m.current_status] || '#64748b';

  return (
    <div className="card" style={{ padding: '12px 16px', marginBottom: 8, cursor: 'pointer' }} onClick={onClick}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={{ width: 10, height: 10, borderRadius: '50%', background: color, flexShrink: 0 }} />
          <div>
            <span style={{ fontWeight: 500, fontSize: '0.9rem' }}>{m.name}</span>
            {m.tags && m.tags.length > 0 && (
              <span style={{ marginLeft: 8 }}>
                {m.tags.map(t => (
                  <span key={t} style={{ fontSize: '0.65rem', background: '#334155', padding: '1px 6px', borderRadius: 4, marginRight: 4 }}>{t}</span>
                ))}
              </span>
            )}
          </div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, fontSize: '0.75rem', color: '#94a3b8' }}>
          <span>{m.uptime_24h.toFixed(1)}%</span>
          {m.avg_response_ms_24h != null && <span>{Math.round(m.avg_response_ms_24h)}ms</span>}
          {m.active_incident && <span style={{ color: '#ef4444', fontWeight: 600 }}>⚠</span>}
        </div>
      </div>
    </div>
  );
}

// ── Main Export ──
export default function StatusPages({ route, onNavigate }) {
  // route.subpage: 'list' | 'create' | 'view'
  // route.slug: for 'view'
  const subpage = route?.subpage || 'list';
  const slug = route?.slug;

  if (subpage === 'create') {
    return (
      <CreateStatusPage
        onCreated={(s) => onNavigate(`/pages/${s}`)}
        onCancel={() => onNavigate('/pages')}
      />
    );
  }

  if (subpage === 'view' && slug) {
    return (
      <StatusPageView
        slug={slug}
        onBack={() => onNavigate('/pages')}
        onMonitorSelect={(id) => onNavigate(`/monitor/${id}`)}
        onDeleted={() => onNavigate('/pages')}
      />
    );
  }

  return (
    <StatusPageList
      onSelect={(s) => onNavigate(`/pages/${s}`)}
      onCreateNew={() => onNavigate('/pages/new')}
    />
  );
}
