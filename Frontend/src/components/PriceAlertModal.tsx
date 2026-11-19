import { BellRing, Link2, Target, X } from 'lucide-react'
import { useState } from 'react'
import { useAuth } from '../context/AuthContext'
import { LoadingDots, StatusNotice } from './ui'

type Props = { product: { modelo_chave: string; titulo_loja: string; loja: string; url: string; preco: number }; onClose: () => void; onSaved: () => void }

export function PriceAlertModal({ product, onClose, onSaved }: Props) {
  const { saveFavorite } = useAuth()
  const suggested = Math.round(product.preco * 0.9 * 100) / 100
  const [target, setTarget] = useState('')
  const [webhook, setWebhook] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const submit = async (withAlert: boolean) => {
    setError('')
    const parsed = target ? Number(target.replace(',', '.')) : undefined
    if (withAlert && (!parsed || parsed <= 0)) return setError('Informe um preço-alvo válido para ativar o alerta.')
    if (webhook && !/^https:\/\//i.test(webhook)) return setError('O webhook deve começar com https://.')
    setSaving(true)
    try { await saveFavorite({ modelo_chave: product.modelo_chave, titulo_loja: product.titulo_loja, loja: product.loja, url: product.url, preco_salvo: product.preco, ...(withAlert && parsed ? { preco_alvo: parsed } : {}), ...(withAlert && webhook ? { webhook_url: webhook } : {}) }); onSaved(); onClose() } catch (err) { setError(err instanceof Error ? err.message : 'Não foi possível salvar o favorito.') } finally { setSaving(false) }
  }
  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}><div className="alert-card"><button onClick={onClose} className="absolute right-4 top-4 rounded-lg p-2 text-slate-500 hover:bg-white/10 hover:text-white"><X size={17} /></button><div className="flex items-start gap-3"><div className="modal-icon"><BellRing size={19} /></div><div><p className="eyebrow">Novo monitoramento</p><h2 className="mt-2 text-xl font-bold tracking-[-.05em] text-white">Salve este produto</h2><p className="mt-1 max-w-[330px] text-xs leading-5 text-slate-500">Escolha apenas salvar ou defina um preço para receber um alerta quando a meta for atingida.</p></div></div><div className="mt-5 rounded-xl border border-white/[.07] bg-white/[.025] p-3"><p className="truncate text-sm font-semibold text-slate-200">{product.titulo_loja}</p><p className="mt-1 font-mono text-xs text-mint">R$ {product.preco.toLocaleString('pt-BR', { minimumFractionDigits: 2 })} hoje</p></div>{error && <div className="mt-4"><StatusNotice>{error}</StatusNotice></div>}<label className="field-label mt-5"><span><Target size={13} />Preço-alvo (R$)</span><input value={target} onChange={(event) => setTarget(event.target.value)} placeholder={`ex.: ${suggested.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}`} inputMode="decimal" /></label><label className="field-label mt-3"><span><Link2 size={13} />Webhook Discord/Slack <em>opcional</em></span><input value={webhook} onChange={(event) => setWebhook(event.target.value)} placeholder="https://discord.com/api/webhooks/..." /></label><div className="mt-6 grid gap-2 sm:grid-cols-2"><button disabled={saving} onClick={() => void submit(false)} className="secondary-action">{saving ? <LoadingDots label="Salvando" /> : 'Salvar apenas nos favoritos'}</button><button disabled={saving} onClick={() => void submit(true)} className="primary-action">{saving ? <LoadingDots label="Ativando" /> : <><BellRing size={15} />Salvar com alerta</>}</button></div></div></div>
}
