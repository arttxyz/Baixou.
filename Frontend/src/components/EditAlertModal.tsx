import { BellRing, Link2, Target, X } from 'lucide-react'
import { useState } from 'react'
import { useAuth } from '../context/AuthContext'
import type { Favorite } from '../types'
import { LoadingDots, StatusNotice } from './ui'

export function EditAlertModal({ favorite, onClose }: { favorite: Favorite; onClose: () => void }) {
  const { updateFavorite } = useAuth()
  const [target, setTarget] = useState(favorite.preco_alvo?.toString().replace('.', ',') || '')
  const [webhook, setWebhook] = useState(favorite.webhook_url || '')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); setError('')
    const parsed = target ? Number(target.replace(',', '.')) : null
    if (target && (!parsed || parsed <= 0)) return setError('Informe um preço-alvo válido.')
    if (webhook && !/^https:\/\//i.test(webhook)) return setError('O webhook deve começar com https://.')
    setSaving(true)
    try { await updateFavorite(favorite.id, { preco_alvo: parsed, webhook_url: webhook || null }); onClose() } catch (err) { setError(err instanceof Error ? err.message : 'Não foi possível atualizar o alerta.') } finally { setSaving(false) }
  }
  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}><form onSubmit={submit} className="alert-card"><button type="button" onClick={onClose} className="absolute right-4 top-4 rounded-lg p-2 text-slate-500 hover:bg-white/10 hover:text-white"><X size={17} /></button><div className="flex items-start gap-3"><div className="modal-icon"><BellRing size={19} /></div><div><p className="eyebrow">Editar monitoramento</p><h2 className="mt-2 text-xl font-bold tracking-[-.05em] text-white">Meta de preço</h2><p className="mt-1 max-w-[330px] truncate text-xs text-slate-500">{favorite.titulo_loja}</p></div></div>{error && <div className="mt-4"><StatusNotice>{error}</StatusNotice></div>}<label className="field-label mt-5"><span><Target size={13} />Preço-alvo (R$)</span><input value={target} onChange={(event) => setTarget(event.target.value)} placeholder="deixe vazio para desativar" inputMode="decimal" /></label><label className="field-label mt-3"><span><Link2 size={13} />Webhook Discord/Slack <em>opcional</em></span><input value={webhook} onChange={(event) => setWebhook(event.target.value)} placeholder="https://..." /></label><div className="mt-6 grid grid-cols-2 gap-2"><button type="button" onClick={() => { setTarget(''); setWebhook(''); void submit({ preventDefault: () => undefined } as React.FormEvent) }} className="secondary-action">Desativar alerta</button><button disabled={saving} className="primary-action">{saving ? <LoadingDots label="Salvando" /> : 'Salvar alterações'}</button></div></form></div>
}
