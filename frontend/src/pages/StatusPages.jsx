import { useState, useEffect, useCallback } from 'react';
import { getStatusPages, getStatusPageDetail, createStatusPageApi, updateStatusPage, deleteStatusPage, addPageMonitors, removePageMonitor } from '../api';
import { IconGlobe, IconSettings, IconTrash } from '../Icons';

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
        <div style={{ marginBottom: 12 }}>
          <label style={{ display: 'block', fontSize: '0.8rem', color: '#94a3b8', marginBottom: 4 }}>Slug *</label>
          <input
            type="text"
            value={slug}
            onChange={(e) => setSlug(e.target.value.toLowerCase().replace(/[^a-z0-9_-]/g, ''))}
            placeholder="e.g., production"
            required
            style={{ width: '100%', padding: '8px 12px', background: '#0f172a', border: '1px solid #334155', borderRadius: 6, color: '#e2e8f0', fontSize: '0.9rem' }}
          />
          <div style={{ fontSize: '0.7rem', color: '#64748b', marginTop: 2 }}>URL-safe identifier (a-z, 0-9, hyphens, underscores)</div>
        </div>

        <div style={{ marginBottom: 12 }}>
          <label style={{ display: 'block', fontSize: '0.8rem', color: '#94a3b8', marginBottom: 4 }}>Title *</label>
          <input
            type="text"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="e.g., Production Status"
            required
            style={{ width: '100%', padding: '8px 12px', background: '#0f172a', border: '1px solid #334155', borderRadius: 6, color: '#e2e8f0', fontSize: '0.9rem' }}
          />
        </div>

        <div style={{ marginBottom: 12 }}>
          <label style={{ display: 'block', fontSize: '0.8rem', color: '#94a3b8', marginBottom: 4 }}>Description</label>
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Brief description of this status page"
            rows={2}
            style={{ width: '100%', padding: '8px 12px', background: '#0f172a', border: '1px solid #334155', borderRadius: 6, color: '#e2e8f0', fontSize: '0.9rem', resize: 'vertical' }}
          />
        </div>

        <div style={{ marginBottom: 12 }}>
          <label style={{ display: 'block', fontSize: '0.8rem', color: '#94a3b8', marginBottom: 4 }}>Logo URL</label>
          <input
            type="url"
            value={logoUrl}
            onChange={(e) => setLogoUrl(e.target.value)}
            placeholder="https://example.com/logo.png"
            style={{ width: '100%', padding: '8px 12px', background: '#0f172a', border: '1px solid #334155', borderRadius: 6, color: '#e2e8f0', fontSize: '0.9rem' }}
          />
        </div>

        <div style={{ marginBottom: 12 }}>
          <label style={{ display: 'block', fontSize: '0.8rem', color: '#94a3b8', marginBottom: 4 }}>Custom Domain</label>
          <input
            type="text"
            value={customDomain}
            onChange={(e) => setCustomDomain(e.target.value)}
            placeholder="status.example.com"
            style={{ width: '100%', padding: '8px 12px', background: '#0f172a', border: '1px solid #334155', borderRadius: 6, color: '#e2e8f0', fontSize: '0.9rem' }}
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

// ── Status Page Detail View ──
function StatusPageView({ slug, onBack, onMonitorSelect }) {
  const [page, setPage] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

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
  page.monitors.forEach(m => {
    if (m.group_name) {
      if (!grouped[m.group_name]) grouped[m.group_name] = [];
      grouped[m.group_name].push(m);
    } else {
      ungrouped.push(m);
    }
  });

  return (
    <div>
      <button className="btn" onClick={onBack} style={{ marginBottom: 16, fontSize: '0.8rem' }}>← Back to Pages</button>

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

      {page.monitors.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 40, color: '#64748b' }}>
          <p>No monitors assigned to this page yet.</p>
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
