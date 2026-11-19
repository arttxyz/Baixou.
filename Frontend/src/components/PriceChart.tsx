import { useMemo } from 'react'
import { Area, AreaChart, CartesianGrid, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import type { PricePoint } from '../types'

const currency = (value: number) => `R$ ${value.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}`

function TooltipContent({ active, payload }: { active?: boolean; payload?: Array<{ payload: PricePoint }> }) {
  if (!active || !payload?.length) return null
  const point = payload[0].payload
  return <div className="chart-tooltip"><p className="font-mono text-[10px] uppercase tracking-[.14em] text-slate-500">{new Date(point.data).toLocaleDateString('pt-BR', { day: '2-digit', month: 'short' })}</p><p className="mt-1 font-mono text-lg font-semibold text-white">{currency(point.preco)}</p><p className="mt-1 text-[11px] text-mint">{point.loja}</p><p className="mt-1 max-w-[180px] truncate text-[10px] text-slate-500">{point.titulo_loja}</p></div>
}

export function PriceChart({ data, average, compact = false }: { data: PricePoint[]; average: number; compact?: boolean }) {
  const chartData = useMemo(() => data.slice().sort((a, b) => new Date(a.data).getTime() - new Date(b.data).getTime()).map((point) => ({ ...point, label: new Date(point.data).toLocaleDateString('pt-BR', { day: '2-digit', month: 'short' }) })), [data])
  const values = chartData.map((point) => point.preco)
  const min = Math.max(0, Math.floor((Math.min(...values, average) - 60) / 50) * 50)
  const max = Math.ceil((Math.max(...values, average) + 60) / 50) * 50
  return <div className={compact ? 'h-[190px]' : 'h-[300px]'}><ResponsiveContainer width="100%" height="100%"><AreaChart data={chartData} margin={{ top: 10, right: 8, left: -16, bottom: 0 }}><defs><linearGradient id="priceFill" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#9df7c4" stopOpacity={0.28} /><stop offset="100%" stopColor="#9df7c4" stopOpacity={0} /></linearGradient></defs><CartesianGrid vertical={false} stroke="rgba(255,255,255,.055)" /><XAxis dataKey="label" axisLine={false} tickLine={false} tick={{ fill: '#586174', fontSize: 10, fontFamily: 'JetBrains Mono' }} minTickGap={28} /><YAxis domain={[min, max]} axisLine={false} tickLine={false} tick={{ fill: '#586174', fontSize: 10, fontFamily: 'JetBrains Mono' }} tickFormatter={(value) => `R$${value}`} width={55} /><Tooltip content={<TooltipContent />} cursor={{ stroke: 'rgba(157,247,196,.25)', strokeWidth: 1 }} /><ReferenceLine y={average} stroke="#9df7c4" strokeDasharray="4 4" strokeOpacity={0.75} label={{ value: `média ${currency(average)}`, fill: '#9df7c4', fontSize: 10, position: 'insideTopRight' }} /><Area type="monotone" dataKey="preco" stroke="#9df7c4" strokeWidth={2} fill="url(#priceFill)" activeDot={{ r: 4, fill: '#9df7c4', stroke: '#07090d', strokeWidth: 2 }} /></AreaChart></ResponsiveContainer></div>
}
