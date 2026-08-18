const API_BASE = '/api/v1';

const getAuthToken = () => localStorage.getItem('auth_token');

function camelize(str: string): string {
  return str.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
}

function camelizeKeys<T>(obj: unknown): T {
  if (obj === null || obj === undefined) {
    return obj as T;
  }
  if (Array.isArray(obj)) {
    return obj.map((item) => camelizeKeys(item)) as T;
  }
  if (typeof obj === 'object' && !(obj instanceof Date)) {
    const result: Record<string, unknown> = {};
    for (const key of Object.keys(obj as Record<string, unknown>)) {
      const camelKey = camelize(key);
      result[camelKey] = camelizeKeys((obj as Record<string, unknown>)[key]);
    }
    return result as T;
  }
  return obj as T;
}

function unwrapContainer<T>(obj: unknown): T {
  if (obj === null || obj === undefined) {
    return obj as T;
  }
  if (Array.isArray(obj)) {
    return obj as T;
  }
  if (typeof obj === 'object') {
    const keys = Object.keys(obj as Record<string, unknown>);
    if (keys.length === 1) {
      const val = (obj as Record<string, unknown>)[keys[0]];
      if (Array.isArray(val)) {
        return val as T;
      }
    }
  }
  return obj as T;
}

const request = async <T>(
  method: string,
  url: string,
  body?: unknown,
  params?: Record<string, string>
): Promise<T> => {
  const token = getAuthToken();
  const fullUrl = new URL(`${API_BASE}/${url}`, window.location.origin);

  if (params) {
    Object.entries(params).forEach(([key, value]) => {
      fullUrl.searchParams.append(key, value);
    });
  }

  const response = await fetch(fullUrl.toString(), {
    method,
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { 'Authorization': `Bearer ${token}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ message: 'Unknown error' }));
    throw new Error(error.message || `HTTP ${response.status}`);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  const data = await response.json();
  return camelizeKeys<T>(data);
};

export const api = {
  get: <T>(url: string, params?: Record<string, string>) =>
    request<T>('GET', url, undefined, params),

  post: <T>(url: string, body?: unknown) =>
    request<T>('POST', url, body),

  put: <T>(url: string, body?: unknown) =>
    request<T>('PUT', url, body),

  delete: (url: string) =>
    request<void>('DELETE', url),
};

export { camelizeKeys, unwrapContainer };

export default api;