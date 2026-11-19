# 📉 Baixou. — Radar Inteligente de Preços & Anti-Fraude de Ofertas

**Baixou.** é uma aplicação **Full Stack** de monitoramento de preços e auditoria de promoções em tempo real. A plataforma analisa links de e-commerce, identifica o modelo exato do produto, compara valores entre múltiplas lojas confiáveis e cruza o preço atual com o histórico dos últimos 30 dias para apontar se a oferta é real ou se o valor está inflado.

---

## ✨ Funcionalidades Principais

* **🔍 Extração Híbrida & Anti-Bloqueio:** Motor de coleta em **Rust** que combina requisições HTTP assíncronas (`reqwest` + `scraper` + JSON-LD) com fallback automático para navegação headless (`headless_chrome`), contornando proteções anti-bot e carregamento dinâmico.
* **🧠 Normalização Inteligente de Modelos:** Identificação automática de nicho (*Tecnologia & Gear* ou *Moda & Streetwear*) via expressões regulares e filtragem semântica para evitar falsos positivos com acessórios (cabos, keycaps, cadarços, palmilhas, etc.).
* **🏪 Comparação Multi-Lojas em Tempo Real:** Varredura simultânea em grandes varejistas, marketplaces e lojas especializadas (*Kabum, Pichau, Terabyte, Amazon BR, Mercado Livre, Doctor Mouse, VTR Imports, Farfetch, Droper e Maze*).
* **📊 Veredito Histórico & Gráficos Interativos:** Armazenamento de séries temporais em **SQLite** com cálculo de média móvel de 30 dias, variação percentual e visualização gráfica interativa no frontend.
* **🔔 Alertas de Preço-Alvo & Worker em Segundo Plano:**
  * Definição de meta de preço personalizada por produto favoritado.
  * Tarefa assíncrona (`tokio::spawn`) que re-verifica periodicamente os itens monitorados em segundo plano.
  * Central de notificações *In-App* (com contador de não lidas) e integração externa via **Webhooks (Discord / Slack)**.
* **🔐 Autenticação & Lista de Monitoramento:** Sistema completo de cadastro, login com hashing **SHA-256**, sessões via token Bearer e gerenciamento de favoritos por usuário.

---

## 🛠️ Arquitetura & Stack Tecnológica

### Backend (API REST & Worker)
* **Linguagem:** Rust
* **Framework Web:** Axum + Tower HTTP (CORS)
* **Runtime Assíncrono:** Tokio
* **Scraping & Parsing:** Reqwest, Scraper, Headless Chrome, Regex, Serde JSON
* **Banco de Dados:** SQLite (`rusqlite` com migração automática de schema)
* **Segurança:** SHA-256 & UUID v4

### Frontend (Interface Web)
* **Core:** React + TypeScript + Vite
* **Estilização:** Tailwind CSS (Dark Mode)
* **Visualização de Dados & Ícones:** Recharts + Lucide React

---

## 📁 Estrutura do Projeto

```text
baixou/
├── src/
│   └── main.rs          # API REST Axum, motor de scraping, worker de alertas e SQLite
├── Cargo.toml           # Dependências do backend Rust
├── baixou.db            # Banco SQLite gerado automaticamente na primeira execução
└── frontend/            # Aplicação React + TypeScript + Vite
    ├── src/
    ├── package.json
    └── vite.config.ts
```

---

## 🚀 Como Executar Localmente

### Pré-requisitos
* [Rust & Cargo](https://www.rust-lang.org/tools/install) instalados
* [Node.js](https://nodejs.org/) (v18+) instalado
* Google Chrome ou Chromium instalado (utilizado pelo fallback headless)

### 1. Subindo o Backend (Porta `3000`)
Na raiz do projeto, execute:

```bash
cargo run
```
O banco de dados `baixou.db` será inicializado automaticamente com todas as tabelas (`historico_precos`, `usuarios`, `sessoes`, `favoritos` e `notificacoes`).

### 2. Subindo o Frontend (Porta `5173` / `4173`)
Em um segundo terminal, entre na pasta do frontend, instale as dependências e inicie o servidor:

```bash
cd frontend
npm install
npm run dev
```
Acesse o endereço exibido no terminal (ex: `http://localhost:5173`).

---

## 🔌 Endpoints da API

| Método | Rota | Descrição | Auth |
| :--- | :--- | :--- | :---: |
| `GET` | `/` | Health check da API e status do worker | Não |
| `POST` | `/api/analisar` | Analisa uma URL, busca ofertas e retorna o veredito | Não |
| `GET` | `/api/historico` | Lista as últimas leituras registradas no radar | Não |
| `GET` | `/api/grafico/:modelo` | Retorna a série histórica de 30 dias de um modelo | Não |
| `POST` | `/api/auth/register` | Cria uma nova conta de usuário | Não |
| `POST` | `/api/auth/login` | Autentica o usuário e retorna o token Bearer | Não |
| `GET` | `/api/favoritos` | Lista os produtos monitorados pelo usuário | Sim |
| `POST` | `/api/favoritos` | Salva um produto com preço-alvo e webhook opcional | Sim |
| `PATCH` | `/api/favoritos/:id` | Atualiza o preço-alvo ou webhook de um favorito | Sim |
| `DELETE` | `/api/favoritos/:id` | Remove um item da lista de monitoramento | Sim |
| `GET` | `/api/notificacoes` | Lista alertas de preço-alvo disparados | Sim |
| `PATCH` | `/api/notificacoes/:id/ler` | Marca uma notificação específica como lida | Sim |
| `POST` | `/api/worker/verificar` | Executa manualmente um ciclo de verificação de preços | Não |