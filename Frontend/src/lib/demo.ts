import type { AnalysisResult, HistoryRecord } from '../types'

const points = [
  ['2026-10-22T10:00:00Z', 789, 'Kabum'],
  ['2026-10-25T10:00:00Z', 774, 'Amazon'],
  ['2026-10-28T10:00:00Z', 824, 'Doctor Mouse'],
  ['2026-11-01T10:00:00Z', 805, 'Kabum'],
  ['2026-11-04T10:00:00Z', 768, 'Mercado Livre'],
  ['2026-11-08T10:00:00Z', 792, 'Amazon'],
  ['2026-11-12T10:00:00Z', 741, 'Doctor Mouse'],
  ['2026-11-15T10:00:00Z', 729.9, 'Kabum'],
  ['2026-11-19T10:00:00Z', 869.9, 'Doctor Mouse'],
] as const

export const demoAnalysis: AnalysisResult = {
  produto_alvo: {
    modelo_chave: 'FX HIEN XL',
    titulo_loja: 'Mousepad Artisan FX Hien XL',
    loja: 'Doctor Mouse',
    url: 'https://doctormouse.example/produto/fx-hien-xl',
    preco: 869.9,
    data: '2026-11-19T14:00:00Z',
  },
  nicho: 'Tecnologia & Gear',
  media_30_dias: 801.4,
  total_registros_30d: 7,
  variacao_percentual: 8.5,
  status_veredito: 'INFLADO',
  mensagem_veredito: 'Cuidado! Este link está 8.5% acima da média registrada nos últimos 30 dias.',
  ofertas_mais_baratas: [
    { categoria_match: 'MESMO MODELO', loja: 'Kabum', titulo_loja: 'Artisan FX Hien XL — Soft', preco: 729.9, economia: 140, url: 'https://kabum.com.br' },
    { categoria_match: 'MESMO MODELO', loja: 'Amazon', titulo_loja: 'Mousepad Artisan FX Hien XL', preco: 759.9, economia: 110, url: 'https://amazon.com.br' },
    { categoria_match: 'SEMELHANTE', loja: 'Mercado Livre', titulo_loja: 'Mousepad Hien XL Cordura', preco: 649.9, economia: 220, url: 'https://mercadolivre.com.br' },
  ],
  historico_grafico: points.map(([data, preco, loja]) => ({ data, preco, loja, titulo_loja: 'Mousepad Artisan FX Hien XL' })),
}

export const demoHistory: HistoryRecord[] = points.slice().reverse().map(([data, preco, loja], index) => ({
  data,
  preco,
  loja,
  titulo_loja: index === 0 ? 'Mousepad Artisan FX Hien XL' : 'Teclado mecânico Keychron K2',
  modelo_chave: index === 0 ? 'FX HIEN XL' : 'KEYCHRON K2',
}))
