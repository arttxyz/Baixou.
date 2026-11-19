import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { api, clearToken, getToken, setToken } from '../lib/api'
import type { Favorite, Notification, User } from '../types'

type FavoriteInput = { modelo_chave: string; titulo_loja: string; loja: string; url: string; preco_salvo: number; preco_alvo?: number; webhook_url?: string }
type AlertSettings = { preco_alvo: number | null; webhook_url: string | null }
type AuthContextValue = {
  user: User | null
  favorites: Favorite[]
  loading: boolean
  favoritesLoading: boolean
  notifications: Notification[]
  unreadNotifications: number
  notificationsLoading: boolean
  signIn: (email: string, senha: string) => Promise<void>
  signUp: (nome: string, email: string, senha: string) => Promise<void>
  signOut: () => void
  refreshFavorites: () => Promise<void>
  saveFavorite: (favorite: FavoriteInput) => Promise<void>
  updateFavorite: (id: number, body: AlertSettings) => Promise<void>
  removeFavorite: (id: number) => Promise<void>
  refreshNotifications: () => Promise<void>
  markNotificationRead: (id: number) => Promise<void>
  markAllNotificationsRead: () => Promise<void>
  verifyWorker: () => Promise<{ ok: boolean; alertas_disparados: number; mensagem: string }>
}

const AuthContext = createContext<AuthContextValue | undefined>(undefined)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [favorites, setFavorites] = useState<Favorite[]>([])
  const [notifications, setNotifications] = useState<Notification[]>([])
  const [loading, setLoading] = useState(true)
  const [favoritesLoading, setFavoritesLoading] = useState(false)
  const [notificationsLoading, setNotificationsLoading] = useState(false)

  const refreshFavorites = async () => {
    if (!getToken()) return setFavorites([])
    setFavoritesLoading(true)
    try { setFavorites(await api.favorites()) } catch { setFavorites([]) } finally { setFavoritesLoading(false) }
  }

  const refreshNotifications = async () => {
    if (!getToken()) return setNotifications([])
    setNotificationsLoading(true)
    try { setNotifications(await api.notifications()) } catch { setNotifications([]) } finally { setNotificationsLoading(false) }
  }

  useEffect(() => {
    const restore = async () => {
      if (!getToken()) return setLoading(false)
      try {
        setUser(await api.me())
        await Promise.all([refreshFavorites(), refreshNotifications()])
      } catch { clearToken(); setUser(null) } finally { setLoading(false) }
    }
    void restore()
  }, [])

  const signIn = async (email: string, senha: string) => { const response = await api.login({ email, senha }); setToken(response.token); setUser(response.usuario); await Promise.all([refreshFavorites(), refreshNotifications()]) }
  const signUp = async (nome: string, email: string, senha: string) => { const response = await api.register({ nome, email, senha }); setToken(response.token); setUser(response.usuario); await Promise.all([refreshFavorites(), refreshNotifications()]) }
  const signOut = () => { clearToken(); setUser(null); setFavorites([]); setNotifications([]) }
  const saveFavorite = async (favorite: FavoriteInput) => { const created = await api.addFavorite(favorite); setFavorites((current) => [created, ...current.filter((item) => item.modelo_chave !== created.modelo_chave)]) }
  const updateFavorite = async (id: number, body: AlertSettings) => { await api.updateFavorite(id, body); await refreshFavorites() }
  const removeFavorite = async (id: number) => { await api.removeFavorite(id); setFavorites((current) => current.filter((item) => item.id !== id)) }
  const markNotificationRead = async (id: number) => { await api.markNotificationRead(id); setNotifications((current) => current.map((item) => item.id === id ? { ...item, lida: true } : item)) }
  const markAllNotificationsRead = async () => { await api.markAllNotificationsRead(); setNotifications((current) => current.map((item) => ({ ...item, lida: true }))) }
  const verifyWorker = async () => { const result = await api.verifyWorker(); await Promise.all([refreshFavorites(), refreshNotifications()]); return result }

  const unreadNotifications = notifications.filter((item) => !item.lida).length
  const value = useMemo(() => ({ user, favorites, loading, favoritesLoading, notifications, unreadNotifications, notificationsLoading, signIn, signUp, signOut, refreshFavorites, saveFavorite, updateFavorite, removeFavorite, refreshNotifications, markNotificationRead, markAllNotificationsRead, verifyWorker }), [user, favorites, loading, favoritesLoading, notifications, unreadNotifications, notificationsLoading])
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() { const value = useContext(AuthContext); if (!value) throw new Error('useAuth deve ser usado dentro de AuthProvider'); return value }
