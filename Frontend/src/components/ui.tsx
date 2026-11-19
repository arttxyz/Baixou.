import type { ReactNode } from 'react'
import { ArrowUpRight, Check, CircleAlert, LoaderCircle } from 'lucide-react'

export function Logo() {
  return <div className="flex items-center gap-2.5"><img src="/baixou-mark.svg" alt="" className="h-8 w-8 rounded-xl object-cover shadow-[0_0_24px_rgba(157,247,196,.2)]" /><span className="text-[19px] font-extrabold tracking-[-.06em] text-white">Baixou<span className="text-mint">.</span></span></div>
}

export function Pill({ children, tone = 'neutral' }: { children: ReactNode; tone?: 'neutral' | 'green' | 'rose' | 'amber' | 'blue' }) {
  const styles = { neutral: 'border-white/10 bg-white/[.04] text-slate-300', green: 'border-emerald-300/20 bg-emerald-300/10 text-emerald-200', rose: 'border-rose-300/20 bg-rose-300/10 text-rose-200', amber: 'border-amber-300/20 bg-amber-300/10 text-amber-200', blue: 'border-electric/20 bg-electric/10 text-indigo-200' }
  return <span className={`inline-flex items-center rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-[.13em] ${styles[tone]}`}>{children}</span>
}

export function Metric({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return <div><p className="mb-1 text-[10px] font-semibold uppercase tracking-[.16em] text-slate-500">{label}</p><p className="font-mono text-[18px] font-semibold tracking-[-.05em] text-slate-100">{value}</p>{hint && <p className="mt-1 text-[11px] text-slate-500">{hint}</p>}</div>
}

export function LoadingDots({ label = 'Processando' }: { label?: string }) {
  return <span className="inline-flex items-center gap-2"><LoaderCircle size={15} className="animate-spin" />{label}</span>
}

export function StatusNotice({ children, tone = 'error' }: { children: ReactNode; tone?: 'error' | 'success' }) {
  return <div className={`flex items-start gap-2 rounded-xl border px-3.5 py-3 text-sm ${tone === 'error' ? 'border-rose-300/20 bg-rose-300/[.07] text-rose-200' : 'border-emerald-300/20 bg-emerald-300/[.07] text-emerald-200'}`}><CircleAlert size={16} className="mt-0.5 shrink-0" /> <span>{children}</span></div>
}

export function ExternalAction({ href, children }: { href: string; children: ReactNode }) {
  return <a href={href} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[.04] px-3 py-2 text-xs font-bold text-slate-200 transition hover:border-mint/30 hover:bg-mint/10 hover:text-mint">{children}<ArrowUpRight size={13} /></a>
}

export function CheckMark() { return <span className="flex h-5 w-5 items-center justify-center rounded-full bg-mint/15 text-mint"><Check size={12} strokeWidth={3} /></span> }
