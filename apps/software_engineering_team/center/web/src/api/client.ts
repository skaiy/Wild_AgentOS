import ky from 'ky';

const apiClient = ky.create({
  prefix: '',
  timeout: 30000,
  retry: 0,
});

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

const API_BASE = '/api/v1';

async function request<T>(method: string, url: string, body?: unknown, params?: Record<string, string>): Promise<T> {
  const searchParams = new URLSearchParams();
  if (params) {
    Object.entries(params).forEach(([key, value]) => {
      searchParams.append(key, value);
    });
  }

  const options: Record<string, unknown> = {
    method,
    headers: { 'Content-Type': 'application/json' },
    ...(searchParams.toString() ? { searchParams } : {}),
  };

  if (body && method !== 'GET') {
    options.json = body;
  }

  try {
    const response = await apiClient(`${API_BASE}/${url}`, options);
    const data = await response.json<unknown>();
    return camelizeKeys<T>(data);
  } catch (err) {
    if (err instanceof Error) {
      throw new Error(err.message);
    }
    throw err;
  }
}

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
export default apiClient;