import { useEffect, useState } from 'react'
import { AuthProvider, useAuth } from './context/AuthContext'
import { AuthModal } from './components/AuthModal'
import { FavoritesView } from './components/FavoritesView'
import { Header } from './components/Header'
import { HistoryDrawer } from './components/HistoryDrawer'
import { RadarView } from './components/RadarView'

function AppContent() {
  const { loading } = useAuth()
  const [page, setPage] = useState<'radar' | 'favorites'>(() => window.location.pathname === '/favoritos' ? 'favorites' : 'radar')
  const [authOpen, setAuthOpen] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)

  useEffect(() => {
    const openAuth = () => setAuthOpen(true)
    window.addEventListener('baixou:auth', openAuth)
    return () => window.removeEventListener('baixou:auth', openAuth)
  }, [])
  useEffect(() => { window.history.replaceState(null, '', page === 'favorites' ? '/favoritos' : '/') }, [page])

  if (loading) return <div className="app-loading"><div className="loading-logo">B<span>.</span></div><div className="mt-4"><span className="loading-pulse" /> <span className="text-xs text-slate-500">preparando seu radar</span></div></div>
  return <div className="min-h-screen"><Header page={page} onPageChange={setPage} onHistory={() => setHistoryOpen(true)} /><div className="noise" />{page === 'radar' ? <RadarView onOpenAuth={() => setAuthOpen(true)} /> : <FavoritesView />}<HistoryDrawer open={historyOpen} onClose={() => setHistoryOpen(false)} />{authOpen && <AuthModal onClose={() => setAuthOpen(false)} />}</div>
}

export default function App() { return <AuthProvider><AppContent /></AuthProvider> }
