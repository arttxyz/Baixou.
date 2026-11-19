# Baixou.

Frontend da plataforma Baixou — um radar de preços que combina histórico, comparação de ofertas e leitura de promoções reais.

## Rodando localmente

```bash
npm install
npm run dev
```

O frontend sobe na porta `4173` para não conflitar com o backend Rust/Axum. A API é lida de `VITE_API_BASE_URL`, com valor padrão em `http://localhost:3000`.

Para trocar a base da API, crie um `.env` a partir do exemplo:

```bash
cp .env.example .env
VITE_API_BASE_URL=http://localhost:3000
```

## Stack

React 18, Vite, TypeScript, Tailwind CSS, Recharts e Lucide React. As fontes Plus Jakarta Sans e JetBrains Mono são carregadas pelo `index.html`.

## Rotas

- `/` — Radar Inteligente
- `/favoritos` — Meus Favoritos (área autenticada)

O arquivo `public/manus-routes.json` mantém o manifesto de rotas para o preview e publicação.
