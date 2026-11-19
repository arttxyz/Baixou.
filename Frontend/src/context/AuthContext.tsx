import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { api, clearToken, getToken, setToken } from '../lib/api'
import type { Favorite, User } from '../types'

type AuthContextValue = {
  user: User | null
  favorites: Favorite[]
  loading: boolean
  favoritesLoading: boolean
  signIn: (email: string, senha: string) => Promise<void>
  signUp: (nome: string, email: string, senha: string) => Promise<void>
  signOut: () => void
  refreshFavorites: () => Promise<void>
  saveFavorite: (favorite: { modelo_chave: string; titulo_loja: string; loja: string; url: string; preco_salvo: number }) => Promise<void>
  removeFavorite: (id: number) => Promise<void>
}

const AuthContext = createContext<AuthContextValue | undefined>(undefined)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [favorites, setFavorites] = useState<Favorite[]>([])
  const [loading, setLoading] = useState(true)
  const [favoritesLoading, setFavoritesLoading] = useState(false)

  const refreshFavorites = async () => {
    if (!getToken()) return setFavorites([])
    setFavoritesLoading(true)
    try {
      setFavorites(await api.favorites())
    } catch {
      setFavorites([])
    } finally {
      setFavoritesLoading(false)
    }
  }

  useEffect(() => {
    const restore = async () => {
      if (!getToken()) return setLoading(false)
      try {
        setUser(await api.me())
        await refreshFavorites()
      } catch {
        clearToken()
        setUser(null)
      } finally {
        setLoading(false)
      }
    }
    void restore()
  }, [])

  const signIn = async (email: string, senha: string) => {
    const response = await api.login({ email, senha })
    setToken(response.token)
    setUser(response.usuario)
    await refreshFavorites()
  }

  const signUp = async (nome: string, email: string, senha: string) => {
    const response = await api.register({ nome, email, senha })
    setToken(response.token)
    setUser(response.usuario)
    await refreshFavorites()
  }

  const signOut = () => {
    clearToken()
    setUser(null)
    setFavorites([])
  }

  const saveFavorite = async (favorite: { modelo_chave: string; titulo_loja: string; loja: string; url: string; preco_salvo: number }) => {
    const created = await api.addFavorite(favorite)
    setFavorites((current) => [created, ...current.filter((item) => item.modelo_chave !== created.modelo_chave)])
  }

  const removeFavorite = async (id: number) => {
    await api.removeFavorite(id)
    setFavorites((current) => current.filter((item) => item.id !== id))
  }

  const value = useMemo(() => ({ user, favorites, loading, favoritesLoading, signIn, signUp, signOut, refreshFavorites, saveFavorite, removeFavorite }), [user, favorites, loading, favoritesLoading])
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  const value = useContext(AuthContext)
  if (!value) throw new Error('useAuth deve ser usado dentro de AuthProvider')
  return value
}
