# Entregas aprovadas

## 1. Estrutura e identidade visual
- [x] Projeto Vite + React 18 + TypeScript, pronto para `npm install && npm run dev`.
- [x] Tailwind CSS, Recharts, Lucide React, Plus Jakarta Sans e JetBrains Mono configurados.
- [x] Interface responsiva em dark mode sofisticado, com logo Baixou., navegação Radar, Meus Favoritos, Histórico Global e perfil.

## 2. Autenticação e sessão
- [x] Modal com abas Entrar/Criar Conta, validação, feedback de erro e senha mínima de 6 caracteres.
- [x] Token persistido em `localStorage`, enviado como Bearer e validado automaticamente em `/api/auth/me`.
- [x] Menu de perfil com nome do usuário e ação de sair.

## 3. Radar Inteligente
- [x] Hero com campo de URL, ações de colar/limpar, CTA Analisar Oferta e loading em etapas progressivas.
- [x] Integração de análise com `POST /api/analisar` e tratamento visual dos estados PROMOCAO, INFLADO e NORMAL.
- [x] Resumo do produto, nicho, modelo, preço atual, média de 30 dias, variação e registros.
- [x] Gráfico de histórico com AreaChart, tooltip pt-BR e ReferenceLine de média.
- [x] Grade de ofertas com filtros, destaque do melhor preço, economia em reais e link externo.
- [x] Fluxo de favoritos, com solicitação de login quando necessário.

## 4. Favoritos e histórico
- [x] Página Meus Favoritos com comparação entre preço salvo e menor preço atual, queda destacada e remoção.
- [x] Consulta do gráfico por modelo com `GET /api/grafico/:modelo`.
- [x] Histórico Global em drawer lateral discreto via `GET /api/historico`, com loading, vazio e erro.

## 5. Integração e entrega
- [x] Variável base de API configurada para `http://localhost:3000`.
- [x] Manifesto `manus-routes.json` servido estaticamente.
- [x] Validar build, preview e sincronização final no checkpoint.
