const BASE = '/api/v1';

async function request(path, options = {}) {
  const url = `${BASE}${path}`;
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...options,
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || `HTTP ${res.status}`);
  }
  return res.json();
}

export async function getDashboard() {
  return request('/dashboard');
}

export async function getStatus() {
  return request('/status');
}

export async function getMonitors() {
  return request('/monitors');
}

export async function getMonitor(id) {
  return request(`/monitors/${id}`);
}

export async function createMonitor(data) {
  return request('/monitors', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function updateMonitor(id, data, key) {
  return request(`/monitors/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(data),
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function deleteMonitor(id, key) {
  return request(`/monitors/${id}`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function pauseMonitor(id, key) {
  return request(`/monitors/${id}/pause`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function resumeMonitor(id, key) {
  return request(`/monitors/${id}/resume`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function getHeartbeats(id, limit = 50) {
  return request(`/monitors/${id}/heartbeats?limit=${limit}`);
}

export async function getUptime(id) {
  return request(`/monitors/${id}/uptime`);
}

export async function getIncidents(id, limit = 20) {
  return request(`/monitors/${id}/incidents?limit=${limit}`);
}

export async function acknowledgeIncident(id, note, actor, key) {
  return request(`/incidents/${id}/acknowledge`, {
    method: 'POST',
    body: JSON.stringify({ note, actor }),
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function getNotifications(monitorId, key) {
  return request(`/monitors/${monitorId}/notifications`, {
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function createNotification(monitorId, data, key) {
  return request(`/monitors/${monitorId}/notifications`, {
    method: 'POST',
    body: JSON.stringify(data),
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function deleteNotification(id, key) {
  return request(`/notifications/${id}`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function updateNotification(id, data, key) {
  return request(`/notifications/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(data),
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function getTags() {
  return request('/tags');
}

export async function getMaintenanceWindows(monitorId) {
  return request(`/monitors/${monitorId}/maintenance`);
}

export async function createMaintenanceWindow(monitorId, data, key) {
  return request(`/monitors/${monitorId}/maintenance`, {
    method: 'POST',
    body: JSON.stringify(data),
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function deleteMaintenanceWindow(id, key) {
  return request(`/maintenance/${id}`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${key}` },
  });
}

export async function bulkCreateMonitors(monitors) {
  return request('/monitors/bulk', {
    method: 'POST',
    body: JSON.stringify({ monitors }),
  });
}
