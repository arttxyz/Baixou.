import { Eye, EyeOff, LockKeyhole, Mail, UserRound, X } from 'lucide-react'
import { useState } from 'react'
import { useAuth } from '../context/AuthContext'
import { LoadingDots, Logo, StatusNotice } from './ui'

type Mode = 'login' | 'register'

export function AuthModal({ onClose }: { onClose: () => void }) {
  const { signIn, signUp } = useAuth()
  const [mode, setMode] = useState<Mode>('login')
  const [name, setName] = useState('')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [visible, setVisible] = useState(false)
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)

  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); setError('')
    if (!email.includes('@')) return setError('Insira um e-mail válido.')
    if (password.length < 6) return setError('A senha precisa ter pelo menos 6 caracteres.')
    if (mode === 'register' && name.trim().length < 2) return setError('Digite seu nome para criar a conta.')
    setSaving(true)
    try {
      if (mode === 'login') await signIn(email, password)
      else await signUp(name.trim(), email, password)
      onClose()
    } catch (err) { setError(err instanceof Error ? err.message : 'Não foi possível concluir. Tente novamente.') } finally { setSaving(false) }
  }

  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}><div className="auth-card"><button onClick={onClose} className="absolute right-4 top-4 rounded-lg p-2 text-slate-500 hover:bg-white/10 hover:text-white"><X size={17} /></button><div className="mb-7 flex flex-col items-center text-center"><Logo /><p className="mt-5 text-xl font-bold tracking-[-.04em] text-white">Sua inteligência de compra</p><p className="mt-2 max-w-[270px] text-sm leading-6 text-slate-400">Acompanhe preços reais e nunca pague mais do que precisa.</p></div><div className="mb-5 grid grid-cols-2 rounded-xl border border-white/[.07] bg-white/[.025] p-1"><button className={`tab-button ${mode === 'login' ? 'tab-button-active' : ''}`} onClick={() => { setMode('login'); setError('') }}>Entrar</button><button className={`tab-button ${mode === 'register' ? 'tab-button-active' : ''}`} onClick={() => { setMode('register'); setError('') }}>Criar conta</button></div>{error && <div className="mb-4"><StatusNotice>{error}</StatusNotice></div>}<form onSubmit={submit} className="space-y-3.5">{mode === 'register' && <label className="input-shell"><UserRound size={16} /><input value={name} onChange={(event) => setName(event.target.value)} placeholder="Seu nome" autoComplete="name" /></label>}<label className="input-shell"><Mail size={16} /><input value={email} onChange={(event) => setEmail(event.target.value)} placeholder="seu@email.com" type="email" autoComplete="email" /></label><label className="input-shell"><LockKeyhole size={16} /><input value={password} onChange={(event) => setPassword(event.target.value)} placeholder="Senha (mín. 6 caracteres)" type={visible ? 'text' : 'password'} autoComplete={mode === 'login' ? 'current-password' : 'new-password'} /><button type="button" onClick={() => setVisible(!visible)} className="text-slate-500 hover:text-slate-200">{visible ? <EyeOff size={16} /> : <Eye size={16} />}</button></label><button disabled={saving} className="mt-2 flex h-11 w-full items-center justify-center rounded-xl bg-mint text-sm font-extrabold text-ink transition hover:bg-white disabled:cursor-wait disabled:opacity-60">{saving ? <LoadingDots label="Aguarde" /> : mode === 'login' ? 'Entrar no Baixou' : 'Criar minha conta'}</button></form><p className="mt-5 text-center text-[10px] leading-5 text-slate-600">Seus dados ficam protegidos. Você pode sair quando quiser.</p></div></div>
}
