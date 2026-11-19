import { ArrowDownRight, CalendarDays, ExternalLink, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { api } from '../lib/api'
import { demoHistory } from '../lib/demo'
import type { HistoryRecord } from '../types'
import { LoadingDots, Pill } from './ui'

export function HistoryDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [records, setRecords] = useState<HistoryRecord[]>([])
  const [loading, setLoading] = useState(false)
  useEffect(() => {
    if (!open) return
    setLoading(true)
    api.history().then(setRecords).catch(() => setRecords(demoHistory)).finally(() => setLoading(false))
  }, [open])
  return <><div className={`drawer-scrim ${open ? 'drawer-scrim-open' : ''}`} onClick={onClose} /><aside className={`history-drawer ${open ? 'history-drawer-open' : ''}`}><div className="flex items-start justify-between border-b border-white/[.07] px-5 py-5"><div><p className="eyebrow"><CalendarDays size={12} />Market pulse</p><h2 className="mt-2 text-lg font-bold tracking-[-.04em] text-white">Histórico global</h2><p className="mt-1 text-xs leading-5 text-slate-500">Os últimos sinais registrados pelo radar.</p></div><button onClick={onClose} className="rounded-lg p-2 text-slate-500 transition hover:bg-white/10 hover:text-white"><X size={17} /></button></div><div className="flex-1 overflow-y-auto px-5 py-4">{loading ? <div className="flex h-40 items-center justify-center text-sm text-slate-400"><LoadingDots label="Carregando sinais" /></div> : records.length === 0 ? <div className="py-16 text-center text-sm text-slate-500">Nenhum registro recente.</div> : <div className="space-y-2">{records.slice(0, 15).map((record, index) => <div key={`${record.data}-${record.preco}-${index}`} className="history-item"><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><Pill tone={index === 0 ? 'green' : 'neutral'}>{index === 0 ? 'novo sinal' : 'registrado'}</Pill><span className="font-mono text-[10px] text-slate-600">{new Date(record.data).toLocaleDateString('pt-BR')}</span></div><p className="mt-2 truncate text-xs font-semibold text-slate-200">{record.titulo_loja}</p><p className="mt-1 truncate text-[10px] text-slate-500">{record.loja}{record.modelo_chave ? ` · ${record.modelo_chave}` : ''}</p></div><div className="pl-3 text-right"><p className="font-mono text-sm font-semibold text-white">R$ {record.preco.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}</p><p className="mt-1 flex items-center justify-end gap-1 text-[10px] text-emerald-300"><ArrowDownRight size={11} />monitorado</p></div></div>)}</div>}</div><div className="border-t border-white/[.07] px-5 py-4"><p className="flex items-center gap-2 text-[10px] leading-4 text-slate-600"><ExternalLink size={12} />Dados agregados das lojas monitoradas pelo Baixou.</p></div></aside></>
}
