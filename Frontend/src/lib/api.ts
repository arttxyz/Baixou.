import type { AnalysisResult, AuthResponse, Favorite, HistoryRecord, PricePoint, User } from '../types'

export const API_BASE = import.meta.env.VITE_API_BASE_URL || 'http://localhost:3000'
const TOKEN_KEY = 'baixou_token'

export const getToken = () => localStorage.getItem(TOKEN_KEY)
export const setToken = (token: string) => localStorage.setItem(TOKEN_KEY, token)
export const clearToken = () => localStorage.removeItem(TOKEN_KEY)

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers)
  headers.set('Content-Type', 'application/json')
  const token = getToken()
  if (token) headers.set('Authorization', `Bearer ${token}`)

  const response = await fetch(`${API_BASE}${path}`, { ...options, headers })
  const payload = await response.json().catch(() => null)
  if (!response.ok) {
    const message = payload?.erro || payload?.error || 'Não foi possível concluir a solicitação.'
    throw new Error(message)
  }
  return payload as T
}

export const api = {
  analyze: (url: string) => request<AnalysisResult>('/api/analisar', { method: 'POST', body: JSON.stringify({ url }) }),
  chart: (model: string) => request<PricePoint[]>(`/api/grafico/${encodeURIComponent(model)}`),
  history: () => request<HistoryRecord[]>('/api/historico'),
  register: (body: { nome: string; email: string; senha: string }) => request<AuthResponse>('/api/auth/register', { method: 'POST', body: JSON.stringify(body) }),
  login: (body: { email: string; senha: string }) => request<AuthResponse>('/api/auth/login', { method: 'POST', body: JSON.stringify(body) }),
  me: () => request<User>('/api/auth/me'),
  favorites: () => request<Favorite[]>('/api/favoritos'),
  addFavorite: (body: { modelo_chave: string; titulo_loja: string; loja: string; url: string; preco_salvo: number }) => request<Favorite>('/api/favoritos', { method: 'POST', body: JSON.stringify(body) }),
  removeFavorite: (id: number) => request<void>(`/api/favoritos/${id}`, { method: 'DELETE' }),
}
