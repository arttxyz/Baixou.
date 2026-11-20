export type Verdict = 'PROMOCAO' | 'INFLADO' | 'NORMAL'
export type MatchCategory = 'MESMO MODELO' | 'SEMELHANTE'

export type PricePoint = {
  data: string
  preco: number
  loja: string
  titulo_loja: string
}

export type ProductTarget = {
  modelo_chave: string
  titulo_loja: string
  loja: string
  url: string
  preco: number
  data: string
}

export type Offer = {
  categoria_match: MatchCategory
  loja: string
  titulo_loja: string
  preco: number
  economia: number
  url: string
}

export type AnalysisResult = {
  modo_busca?: 'LINK' | 'NOME'
  produto_alvo: ProductTarget
  nicho: string
  media_30_dias: number
  total_registros_30d: number
  variacao_percentual: number
  status_veredito: Verdict
  mensagem_veredito: string
  ofertas_mais_baratas: Offer[]
  historico_grafico: PricePoint[]
}

export type Favorite = {
  id: number
  modelo_chave: string
  titulo_loja: string
  loja: string
  url: string
  preco_salvo: number
  menor_preco_atual: number
  preco_alvo: number | null
  webhook_url: string | null
  alerta_disparado: boolean
  criado_em: string
}

export type Notification = {
  id: number
  modelo_chave: string
  titulo_loja: string
  loja: string
  url: string
  preco_encontrado: number
  preco_alvo: number
  mensagem: string
  lida: boolean
  criado_em: string
}

export type User = {
  id: number
  nome: string
  email: string
}

export type AuthResponse = {
  token: string
  usuario: User
}

export type HistoryRecord = PricePoint & {
  modelo_chave?: string
}
