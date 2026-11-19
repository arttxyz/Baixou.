import { ArrowDown, BarChart3, ExternalLink, RefreshCw, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useAuth } from '../context/AuthContext'
import { api } from '../lib/api'
import { demoAnalysis } from '../lib/demo'
import type { Favorite, PricePoint } from '../types'
import { LoadingDots, Pill, StatusNotice } from './ui'
import { PriceChart } from './PriceChart'

const money = (value: number) => `R$ ${value.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}`

export function FavoritesView() {
  const { favorites, favoritesLoading, refreshFavorites, removeFavorite } = useAuth()
  const [chartId, setChartId] = useState<number | null>(null)
  const [chart, setChart] = useState<PricePoint[]>([])
  const [chartLoading, setChartLoading] = useState(false)
  const [error, setError] = useState('')
  useEffect(() => { void refreshFavorites() }, [])
  const toggleChart = async (favorite: Favorite) => {
    if (chartId === favorite.id) return setChartId(null)
    setChartId(favorite.id); setChartLoading(true); setError('')
    try { setChart(await api.chart(favorite.modelo_chave)) } catch { setChart(demoAnalysis.historico_grafico) } finally { setChartLoading(false) }
  }
  const handleRemove = async (id: number) => { try { await removeFavorite(id) } catch { setError('Não foi possível remover este favorito.') } }
  return <main className="page-shell"><div className="mb-8 flex flex-wrap items-end justify-between gap-4"><div><p className="eyebrow"><BarChart3 size={13} />Seu radar pessoal</p><h1 className="mt-3 text-3xl font-bold tracking-[-.06em] text-white sm:text-4xl">Meus favoritos<span className="text-mint">.</span></h1><p className="mt-2 max-w-[480px] text-sm leading-6 text-slate-500">Itens que você decidiu monitorar. Quando o preço muda, você fica sabendo primeiro.</p></div><button onClick={() => void refreshFavorites()} className="icon-button"><RefreshCw size={15} />Atualizar lista</button></div>{error && <div className="mb-5"><StatusNotice>{error}</StatusNotice></div>}{favoritesLoading ? <div className="panel flex min-h-[260px] items-center justify-center"><LoadingDots label="Buscando seus favoritos" /></div> : favorites.length === 0 ? <div className="empty-state panel"><div className="empty-orbit"><BarChart3 size={24} /></div><h2 className="mt-5 text-lg font-bold text-white">Seu radar ainda está vazio</h2><p className="mt-2 max-w-[340px] text-center text-sm leading-6 text-slate-500">Analise um produto e salve os que você quer acompanhar de perto.</p></div> : <div className="space-y-3">{favorites.map((favorite) => { const dropped = favorite.menor_preco_atual < favorite.preco_salvo; const delta = favorite.preco_salvo - favorite.menor_preco_atual; return <article key={favorite.id} className="panel overflow-hidden"><div className="flex flex-col gap-5 p-5 sm:flex-row sm:items-center sm:justify-between sm:p-6"><div className="flex min-w-0 items-start gap-4"><div className="favorite-mark"><BarChart3 size={18} /></div><div className="min-w-0"><div className="flex flex-wrap items-center gap-2"><Pill tone="blue">{favorite.modelo_chave}</Pill>{dropped && <Pill tone="green"><ArrowDown size={11} className="mr-1" />Preço caiu</Pill>}</div><h2 className="mt-3 truncate text-base font-bold text-white">{favorite.titulo_loja}</h2><p className="mt-1 text-xs text-slate-500">{favorite.loja} · salvo em {new Date(favorite.criado_em).toLocaleDateString('pt-BR')}</p></div></div><div className="flex items-end gap-6 sm:items-center"><div className="text-right"><p className="text-[10px] uppercase tracking-[.14em] text-slate-600">menor atual</p><p className="mt-1 font-mono text-lg font-semibold text-mint">{money(favorite.menor_preco_atual)}</p><p className={`mt-1 font-mono text-[10px] ${dropped ? 'text-emerald-300' : 'text-slate-600'}`}>{dropped ? `↓ ${money(delta)} desde o salvamento` : 'sem queda detectada'}</p></div><div className="flex gap-2"><button onClick={() => void toggleChart(favorite)} className={`icon-button ${chartId === favorite.id ? 'icon-button-active' : ''}`} title="Ver histórico"><BarChart3 size={15} /> <span className="hidden sm:inline">Histórico</span></button><a href={favorite.url} target="_blank" rel="noreferrer" className="icon-button" title="Abrir loja"><ExternalLink size={15} /></a><button onClick={() => void handleRemove(favorite.id)} className="icon-button icon-button-danger" title="Remover favorito"><Trash2 size={15} /></button></div></div></div>{chartId === favorite.id && <div className="border-t border-white/[.07] bg-white/[.015] px-5 pb-5 pt-4 sm:px-6"><div className="mb-3 flex items-center justify-between"><p className="eyebrow">Histórico de preço · {favorite.modelo_chave}</p>{chartLoading && <LoadingDots label="Atualizando" />}</div>{chartLoading ? <div className="h-[190px]" /> : <PriceChart data={chart} average={favorite.preco_salvo} compact />}</div>}</article>})}</div>}</main>
}
