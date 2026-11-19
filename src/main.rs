use axum::{
    extract::{Path, State},
    http::{HeaderMap as AxumHeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use headless_chrome::{Browser, LaunchOptions};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use rusqlite::{params, Connection, OptionalExtension};
use scraper::{Html as ScraperHtml, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration as StdDuration;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RegistroPreco {
    modelo_chave: String,
    titulo_loja: String,
    loja: String,
    url: String,
    preco: f64,
    data: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct OfertaComparada {
    registro: RegistroPreco,
    categoria_match: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
enum Nicho {
    Tecnologia,
    Moda,
}

struct FonteBusca {
    nome: &'static str,
    categoria: &'static str,
    url_busca: String,
}

struct AppState {
    db: Arc<Mutex<Connection>>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct AnaliseRequest {
    url: String,
}

#[derive(Serialize)]
struct OfertaWeb {
    categoria_match: String,
    loja: String,
    titulo_loja: String,
    preco: f64,
    economia: f64,
    url: String,
}

#[derive(Serialize)]
struct PontoGrafico {
    data: String,
    preco: f64,
    loja: String,
    titulo_loja: String,
}

#[derive(Serialize)]
struct AnaliseResponse {
    produto_alvo: RegistroPreco,
    nicho: String,
    media_30_dias: f64,
    total_registros_30d: usize,
    variacao_percentual: f64,
    status_veredito: String,
    mensagem_veredito: String,
    ofertas_mais_baratas: Vec<OfertaWeb>,
    historico_grafico: Vec<PontoGrafico>,
}

#[derive(Deserialize)]
struct RegisterRequest {
    nome: String,
    email: String,
    senha: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    senha: String,
}

#[derive(Serialize)]
struct UsuarioPublico {
    id: i64,
    nome: String,
    email: String,
}

#[derive(Serialize)]
struct AuthResponse {
    token: String,
    usuario: UsuarioPublico,
}

#[derive(Deserialize)]
struct NovoFavoritoRequest {
    modelo_chave: String,
    titulo_loja: String,
    loja: String,
    url: String,
    preco_salvo: f64,
}

#[derive(Serialize)]
struct FavoritoItem {
    id: i64,
    modelo_chave: String,
    titulo_loja: String,
    loja: String,
    url: String,
    preco_salvo: f64,
    menor_preco_atual: f64,
    criado_em: String,
}

#[derive(Serialize)]
struct ErroResponse {
    erro: String,
}

const ARQUIVO_BANCO: &str = "baixou.db";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = inicializar_banco()?;
    let client = criar_cliente_http();

    let state = Arc::new(AppState {
        db: Arc::new(Mutex::new(conn)),
        client,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(health_check))
        // Rotas de Análise e Gráficos
        .route("/api/analisar", post(api_analisar_link))
        .route("/api/historico", get(api_listar_historico))
        .route("/api/grafico/:modelo", get(api_dados_grafico))
        // Rotas de Autenticação
        .route("/api/auth/register", post(api_register))
        .route("/api/auth/login", post(api_login))
        .route("/api/auth/me", get(api_me))
        // Rotas de Favoritos / Monitoramento do Usuário
        .route("/api/favoritos", get(api_listar_favoritos).post(api_salvar_favorito))
        .route("/api/favoritos/:id", delete(api_remover_favorito))
        .layer(cors)
        .with_state(state);

    let endereco = "0.0.0.0:3000";
    println!("=========================================================");
    println!("   BAIXOU API REST FULL-STACK ONLINE (PORTA 3000)        ");
    println!("   Banco SQLite: {} (Auth + Histórico + Favoritos) ", ARQUIVO_BANCO);
    println!("   Pronto para conectar ao Frontend React/Vite           ");
    println!("=========================================================");

    let listener = tokio::net::TcpListener::bind(endereco).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "online",
        "servico": "Baixou API v2",
        "docs": "Conecte seu frontend React/Vite em http://localhost:3000"
    }))
}

// =========================================================================
// AUTENTICAÇÃO E GESTÃO DE CONTAS
// =========================================================================

fn hash_senha(senha: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("baixou_salt_seguro_2026:{}", senha).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn extrair_usuario_autenticado(headers: &AxumHeaderMap, conn: &Connection) -> Option<UsuarioPublico> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?.trim();

    conn.query_row(
        "SELECT u.id, u.nome, u.email
         FROM sessoes s
         JOIN usuarios u ON u.id = s.usuario_id
         WHERE s.token = ?1",
        params![token],
        |row| {
            Ok(UsuarioPublico {
                id: row.get(0)?,
                nome: row.get(1)?,
                email: row.get(2)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

async fn api_register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    let nome = payload.nome.trim().to_string();
    let email = payload.email.trim().to_lowercase();
    let senha = payload.senha.trim().to_string();

    if nome.len() < 2 || !email.contains('@') || senha.len() < 6 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErroResponse {
                erro: "Preencha nome válido, e-mail válido e senha com no mínimo 6 caracteres."
                    .to_string(),
            }),
        )
            .into_response();
    }

    let conn = state.db.lock().unwrap();

    let existe: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM usuarios WHERE email = ?1)",
            params![email],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if existe {
        return (
            StatusCode::CONFLICT,
            Json(ErroResponse {
                erro: "Este e-mail já está cadastrado.".to_string(),
            }),
        )
            .into_response();
    }

    let senha_hash = hash_senha(&senha);
    let agora = Utc::now().to_rfc3339();

    if let Err(e) = conn.execute(
        "INSERT INTO usuarios (nome, email, senha_hash, criado_em) VALUES (?1, ?2, ?3, ?4)",
        params![nome, email, senha_hash, agora],
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErroResponse {
                erro: format!("Erro ao criar conta: {}", e),
            }),
        )
            .into_response();
    }

    let usuario_id = conn.last_insert_rowid();
    let token = Uuid::new_v4().to_string();

    let _ = conn.execute(
        "INSERT INTO sessoes (token, usuario_id, criado_em) VALUES (?1, ?2, ?3)",
        params![token, usuario_id, agora],
    );

    (
        StatusCode::CREATED,
        Json(AuthResponse {
            token,
            usuario: UsuarioPublico {
                id: usuario_id,
                nome,
                email,
            },
        }),
    )
        .into_response()
}

async fn api_login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    let email = payload.email.trim().to_lowercase();
    let senha_hash = hash_senha(payload.senha.trim());

    let conn = state.db.lock().unwrap();

    let usuario_opt = conn
        .query_row(
            "SELECT id, nome, email FROM usuarios WHERE email = ?1 AND senha_hash = ?2",
            params![email, senha_hash],
            |row| {
                Ok(UsuarioPublico {
                    id: row.get(0)?,
                    nome: row.get(1)?,
                    email: row.get(2)?,
                })
            },
        )
        .optional()
        .unwrap_or(None);

    let Some(usuario) = usuario_opt else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErroResponse {
                erro: "E-mail ou senha incorretos.".to_string(),
            }),
        )
            .into_response();
    };

    let token = Uuid::new_v4().to_string();
    let agora = Utc::now().to_rfc3339();

    let _ = conn.execute(
        "INSERT INTO sessoes (token, usuario_id, criado_em) VALUES (?1, ?2, ?3)",
        params![token, usuario.id, agora],
    );

    (StatusCode::OK, Json(AuthResponse { token, usuario })).into_response()
}

async fn api_me(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match extrair_usuario_autenticado(&headers, &conn) {
        Some(usuario) => (StatusCode::OK, Json(usuario)).into_response(),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(ErroResponse {
                erro: "Sessão inválida ou expirada.".to_string(),
            }),
        )
            .into_response(),
    }
}

// =========================================================================
// FAVORITOS / LISTA DE MONITORAMENTO DO USUÁRIO
// =========================================================================

async fn api_listar_favoritos(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let Some(usuario) = extrair_usuario_autenticado(&headers, &conn) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErroResponse {
                erro: "Faça login para ver seus itens salvos.".to_string(),
            }),
        )
            .into_response();
    };

    let mut stmt = conn
        .prepare(
            "SELECT id, modelo_chave, titulo_loja, loja, url, preco_salvo, criado_em
             FROM favoritos
             WHERE usuario_id = ?1
             ORDER BY id DESC",
        )
        .unwrap();

    let rows = stmt
        .query_map(params![usuario.id], |row| {
            let modelo: String = row.get(1)?;
            let preco_salvo: f64 = row.get(5)?;

            let menor_atual: f64 = conn
                .query_row(
                    "SELECT MIN(preco) FROM historico_precos WHERE modelo_chave = ?1",
                    params![modelo],
                    |r| r.get::<_, Option<f64>>(0),
                )
                .unwrap_or(None)
                .unwrap_or(preco_salvo);

            Ok(FavoritoItem {
                id: row.get(0)?,
                modelo_chave: modelo,
                titulo_loja: row.get(2)?,
                loja: row.get(3)?,
                url: row.get(4)?,
                preco_salvo,
                menor_preco_atual: menor_atual,
                criado_em: row.get(6)?,
            })
        })
        .unwrap();

    let mut lista = Vec::new();
    for item in rows {
        if let Ok(fav) = item {
            lista.push(fav);
        }
    }

    (StatusCode::OK, Json(lista)).into_response()
}

async fn api_salvar_favorito(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<NovoFavoritoRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let Some(usuario) = extrair_usuario_autenticado(&headers, &conn) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErroResponse {
                erro: "Faça login para salvar produtos na sua lista.".to_string(),
            }),
        )
            .into_response();
    };

    let agora = Utc::now().to_rfc3339();
    if let Err(e) = conn.execute(
        "INSERT INTO favoritos (usuario_id, modelo_chave, titulo_loja, loja, url, preco_salvo, criado_em)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            usuario.id,
            payload.modelo_chave,
            payload.titulo_loja,
            payload.loja,
            payload.url,
            payload.preco_salvo,
            agora
        ],
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErroResponse {
                erro: format!("Erro ao salvar favorito: {}", e),
            }),
        )
            .into_response();
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "ok": true, "id": conn.last_insert_rowid() })),
    )
        .into_response()
}

async fn api_remover_favorito(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Path(fav_id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let Some(usuario) = extrair_usuario_autenticado(&headers, &conn) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErroResponse {
                erro: "Não autorizado.".to_string(),
            }),
        )
            .into_response();
    };

    let _ = conn.execute(
        "DELETE FROM favoritos WHERE id = ?1 AND usuario_id = ?2",
        params![fav_id, usuario.id],
    );

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

// =========================================================================
// ANÁLISE DE LINKS, HISTÓRICO E SÉRIE TEMPORAL PARA GRÁFICOS
// =========================================================================

async fn api_listar_historico(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let registros = {
        let conn = state.db.lock().unwrap();
        buscar_ultimos_registros(&conn, 15).unwrap_or_default()
    };
    Json(registros)
}

async fn api_dados_grafico(
    State(state): State<Arc<AppState>>,
    Path(modelo): Path<String>,
) -> impl IntoResponse {
    let modelo_limpo = modelo.trim().to_uppercase();
    let pontos = {
        let conn = state.db.lock().unwrap();
        buscar_serie_grafico(&conn, &modelo_limpo).unwrap_or_default()
    };
    Json(pontos)
}

async fn api_analisar_link(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AnaliseRequest>,
) -> impl IntoResponse {
    let url = payload.url.trim().to_string();
    if url.is_empty() || !url.starts_with("http") {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErroResponse {
                erro: "Cole um link válido começando com http:// ou https://".to_string(),
            }),
        )
            .into_response();
    }

    let loja_origem = identificar_loja(&url);
    let nicho = detectar_nicho(&url);
    let nicho_str = match nicho {
        Nicho::Tecnologia => "Tecnologia & Gear",
        Nicho::Moda => "Moda & Streetwear",
    };

    println!("\n[API] Analisando: {} ({})", loja_origem, url);

    let produto_alvo = match extrair_produto_da_url(&state.client, &url, loja_origem, &nicho).await
    {
        Ok(prod) => prod,
        Err(erro) => {
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(ErroResponse { erro })).into_response();
        }
    };

    {
        let conn = state.db.lock().unwrap();
        let _ = inserir_registro(&conn, &produto_alvo);
    }

    let ofertas_encontradas =
        buscar_nas_lojas_confiaveis(&state.client, &produto_alvo, &nicho).await;

    let (registros_30d, historico_grafico) = {
        let conn = state.db.lock().unwrap();
        for oferta in &ofertas_encontradas {
            if oferta.categoria_match == "MESMO MODELO" {
                let _ = inserir_registro(&conn, &oferta.registro);
            }
        }
        let r30 = buscar_historico_30_dias(&conn, &produto_alvo.modelo_chave).unwrap_or_default();
        let graf = buscar_serie_grafico(&conn, &produto_alvo.modelo_chave).unwrap_or_default();
        (r30, graf)
    };

    let media_30d = if registros_30d.is_empty() {
        produto_alvo.preco
    } else {
        let soma: f64 = registros_30d.iter().map(|r| r.preco).sum();
        soma / registros_30d.len() as f64
    };

    let diff_media = produto_alvo.preco - media_30d;
    let perc_media = if media_30d > 0.0 {
        (diff_media / media_30d) * 100.0
    } else {
        0.0
    };

    let (status_veredito, mensagem_veredito) = if perc_media <= -5.0 {
        (
            "PROMOCAO".to_string(),
            format!(
                "Vale a pena! Este link está {:.1}% abaixo da média dos últimos 30 dias.",
                perc_media.abs()
            ),
        )
    } else if perc_media > 5.0 {
        (
            "INFLADO".to_string(),
            format!(
                "Cuidado! Este link está {:.1}% acima da média registrada nos últimos 30 dias.",
                perc_media
            ),
        )
    } else {
        (
            "NORMAL".to_string(),
            "Preço dentro da faixa normal praticada nos últimos 30 dias.".to_string(),
        )
    };

    let mut mais_baratas: Vec<&OfertaComparada> = ofertas_encontradas
        .iter()
        .filter(|o| o.registro.preco < produto_alvo.preco)
        .collect();

    mais_baratas.sort_by(|a, b| {
        a.registro
            .preco
            .partial_cmp(&b.registro.preco)
            .unwrap_or(Ordering::Equal)
    });

    let ofertas_web: Vec<OfertaWeb> = mais_baratas
        .into_iter()
        .take(8)
        .map(|o| OfertaWeb {
            categoria_match: o.categoria_match.to_string(),
            loja: o.registro.loja.clone(),
            titulo_loja: o.registro.titulo_loja.clone(),
            preco: o.registro.preco,
            economia: produto_alvo.preco - o.registro.preco,
            url: o.registro.url.clone(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(AnaliseResponse {
            produto_alvo,
            nicho: nicho_str.to_string(),
            media_30_dias: media_30d,
            total_registros_30d: registros_30d.len(),
            variacao_percentual: perc_media,
            status_veredito,
            mensagem_veredito,
            ofertas_mais_baratas: ofertas_web,
            historico_grafico,
        }),
    )
        .into_response()
}

// =========================================================================
// CAMADA DE BANCO DE DADOS (SQLITE)
// =========================================================================

fn inicializar_banco() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(ARQUIVO_BANCO)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS historico_precos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            modelo_chave TEXT NOT NULL,
            titulo_loja TEXT NOT NULL,
            loja TEXT NOT NULL,
            url TEXT NOT NULL,
            preco REAL NOT NULL,
            data_coleta TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_modelo_data ON historico_precos (modelo_chave, data_coleta)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS usuarios (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            nome TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE,
            senha_hash TEXT NOT NULL,
            criado_em TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessoes (
            token TEXT PRIMARY KEY,
            usuario_id INTEGER NOT NULL,
            criado_em TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS favoritos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            usuario_id INTEGER NOT NULL,
            modelo_chave TEXT NOT NULL,
            titulo_loja TEXT NOT NULL,
            loja TEXT NOT NULL,
            url TEXT NOT NULL,
            preco_salvo REAL NOT NULL,
            criado_em TEXT NOT NULL
        )",
        [],
    )?;

    Ok(conn)
}

fn inserir_registro(conn: &Connection, reg: &RegistroPreco) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO historico_precos (modelo_chave, titulo_loja, loja, url, preco, data_coleta)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            reg.modelo_chave,
            reg.titulo_loja,
            reg.loja,
            reg.url,
            reg.preco,
            reg.data.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn buscar_historico_30_dias(
    conn: &Connection,
    modelo_chave: &str,
) -> Result<Vec<RegistroPreco>, rusqlite::Error> {
    let limite_30_dias = (Utc::now() - Duration::days(30)).to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT modelo_chave, titulo_loja, loja, url, preco, data_coleta
         FROM historico_precos
         WHERE modelo_chave = ?1 AND data_coleta >= ?2",
    )?;

    let linhas = stmt.query_map(params![modelo_chave, limite_30_dias], |row| {
        let data_str: String = row.get(5)?;
        let data_parseada = DateTime::parse_from_rfc3339(&data_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(RegistroPreco {
            modelo_chave: row.get(0)?,
            titulo_loja: row.get(1)?,
            loja: row.get(2)?,
            url: row.get(3)?,
            preco: row.get(4)?,
            data: data_parseada,
        })
    })?;

    let mut registros = Vec::new();
    for item in linhas {
        registros.push(item?);
    }

    Ok(registros)
}

fn buscar_serie_grafico(
    conn: &Connection,
    modelo_chave: &str,
) -> Result<Vec<PontoGrafico>, rusqlite::Error> {
    let limite_30_dias = (Utc::now() - Duration::days(30)).to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT data_coleta, preco, loja, titulo_loja
         FROM historico_precos
         WHERE modelo_chave = ?1 AND data_coleta >= ?2
         ORDER BY id ASC",
    )?;

    let linhas = stmt.query_map(params![modelo_chave, limite_30_dias], |row| {
        Ok(PontoGrafico {
            data: row.get(0)?,
            preco: row.get(1)?,
            loja: row.get(2)?,
            titulo_loja: row.get(3)?,
        })
    })?;

    let mut pontos = Vec::new();
    for p in linhas {
        pontos.push(p?);
    }
    Ok(pontos)
}

fn buscar_ultimos_registros(
    conn: &Connection,
    limite: usize,
) -> Result<Vec<RegistroPreco>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT modelo_chave, titulo_loja, loja, url, preco, data_coleta
         FROM historico_precos
         ORDER BY id DESC
         LIMIT ?1",
    )?;

    let linhas = stmt.query_map(params![limite as i64], |row| {
        let data_str: String = row.get(5)?;
        let data_parseada = DateTime::parse_from_rfc3339(&data_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(RegistroPreco {
            modelo_chave: row.get(0)?,
            titulo_loja: row.get(1)?,
            loja: row.get(2)?,
            url: row.get(3)?,
            preco: row.get(4)?,
            data: data_parseada,
        })
    })?;

    let mut registros = Vec::new();
    for item in linhas {
        registros.push(item?);
    }

    Ok(registros)
}

// =========================================================================
// MOTOR DE CONEXÃO RESILIENTE (HTTP + CHROME STEALTH)
// =========================================================================

fn criar_cliente_http() -> reqwest::Client {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static("pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7"),
    );

    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(StdDuration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

async fn baixar_html_resiliente(client: &reqwest::Client, url: &str) -> Result<String, String> {
    if let Ok(resp) = client.get(url).send().await {
        if resp.status().is_success() {
            if let Ok(texto) = resp.text().await {
                if !texto.contains("Just a moment...")
                    && !texto.contains("Type the characters you see in this image")
                    && texto.len() > 2500
                {
                    return Ok(texto);
                }
            }
        }
    }

    let url_owned = url.to_string();
    tokio::task::spawn_blocking(move || {
        let args = vec![
            OsStr::new("--disable-blink-features=AutomationControlled"),
            OsStr::new("--no-sandbox"),
            OsStr::new("--disable-gpu"),
            OsStr::new("--window-size=1920,1080"),
            OsStr::new("--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"),
        ];

        let options = LaunchOptions::default_builder()
            .headless(true)
            .args(args)
            .build()
            .unwrap_or_default();

        let browser =
            Browser::new(options).map_err(|e| format!("Falha ao abrir Chrome: {}", e))?;
        let tab = browser
            .new_tab()
            .map_err(|e| format!("Falha ao abrir aba: {}", e))?;

        tab.navigate_to(&url_owned)
            .map_err(|e| format!("Falha ao navegar: {}", e))?;
        let _ = tab.wait_until_navigated();

        thread::sleep(StdDuration::from_secs(4));

        tab.get_content()
            .map_err(|e| format!("Falha ao ler conteúdo: {}", e))
    })
    .await
    .map_err(|e| format!("Erro de thread: {}", e))?
}

// =========================================================================
// EXTRAÇÃO DO LINK ENVIADO PELO USUÁRIO
// =========================================================================

async fn extrair_produto_da_url(
    client: &reqwest::Client,
    url: &str,
    loja: &str,
    nicho: &Nicho,
) -> Result<RegistroPreco, String> {
    let html_content = baixar_html_resiliente(client, url).await?;
    let documento = ScraperHtml::parse_document(&html_content);

    let titulo_loja = extrair_titulo_completo(&documento);

    let preco = extrair_preco_html(&documento).ok_or_else(|| {
        "Preço não localizado (item esgotado ou bloqueio anti-bot ativo na loja).".to_string()
    })?;

    let modelo_chave = extrair_modelo_inteligente(&titulo_loja, nicho);

    Ok(RegistroPreco {
        modelo_chave,
        titulo_loja,
        loja: loja.to_string(),
        url: url.to_string(),
        preco,
        data: Utc::now(),
    })
}

fn extrair_titulo_completo(documento: &ScraperHtml) -> String {
    let seletor_h1 = Selector::parse("h1").unwrap();
    let titulo_h1 = documento
        .select(&seletor_h1)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .unwrap_or_default();

    let mut titulo_json_ld = String::new();
    if let Ok(sel_ld) = Selector::parse(r#"script[type="application/ld+json"]"#) {
        for script in documento.select(&sel_ld) {
            if let Ok(json) = serde_json::from_str::<Value>(&script.inner_html()) {
                if let Some(nome_completo) = extrair_nome_produto_json(&json) {
                    titulo_json_ld = nome_completo;
                    break;
                }
            }
        }
    }

    let mut titulo_og = String::new();
    if let Ok(sel_og) = Selector::parse(r#"meta[property="og:title"]"#) {
        if let Some(el) = documento.select(&sel_og).next() {
            if let Some(content) = el.value().attr("content") {
                titulo_og = content
                    .split('|')
                    .next()
                    .unwrap_or(content)
                    .split(" - ")
                    .next()
                    .unwrap_or(content)
                    .trim()
                    .to_string();
            }
        }
    }

    if titulo_h1.split_whitespace().count() >= 3 {
        titulo_h1
    } else if !titulo_json_ld.is_empty() {
        titulo_json_ld
    } else if !titulo_og.is_empty() {
        titulo_og
    } else if !titulo_h1.is_empty() {
        titulo_h1
    } else {
        "Produto sem título".to_string()
    }
}

fn extrair_nome_produto_json(json: &Value) -> Option<String> {
    if let Some(obj) = json.as_object() {
        if obj.get("@type").and_then(|t| t.as_str()) == Some("Product") {
            let nome = obj.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
            let marca = obj
                .get("brand")
                .and_then(|b| {
                    b.as_str()
                        .or_else(|| b.get("name").and_then(|n| n.as_str()))
                })
                .unwrap_or("")
                .trim();

            if !nome.is_empty() {
                if !marca.is_empty() && !nome.to_lowercase().contains(&marca.to_lowercase()) {
                    return Some(format!("{} {}", marca, nome));
                }
                return Some(nome.to_string());
            }
        }
        if let Some(graph) = obj.get("@graph").and_then(|g| g.as_array()) {
            for item in graph {
                if let Some(res) = extrair_nome_produto_json(item) {
                    return Some(res);
                }
            }
        }
    } else if let Some(arr) = json.as_array() {
        for item in arr {
            if let Some(res) = extrair_nome_produto_json(item) {
                return Some(res);
            }
        }
    }
    None
}

fn extrair_modelo_inteligente(titulo: &str, nicho: &Nicho) -> String {
    if *nicho == Nicho::Tecnologia {
        let re_gpu =
            Regex::new(r"(?i)\b(rtx|gtx|rx|arc)\s*\d{3,4}(\s*(ti|super|xt|xtx))?\b").unwrap();
        if let Some(m) = re_gpu.find(titulo) {
            return m
                .as_str()
                .to_uppercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }

        let re_cpu =
            Regex::new(r"(?i)\b(ryzen\s*\d\s*\d{4}[a-z0-9]*|core\s*i\d[-\s]*\d{4,5}[a-z]*)\b")
                .unwrap();
        if let Some(m) = re_cpu.find(titulo) {
            return m
                .as_str()
                .to_uppercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }
    } else {
        let re_sneaker = Regex::new(
            r"(?i)\b(air\s*jordan\s*\d{1,2}(\s*(low|mid|high))?|dunk\s*(low|high)(\s*retro|\s*sb)?|air\s*force\s*1(\s*(low|mid|high))?|air\s*max\s*\d{2,3}|samba\s*(og|xl)?|campus\s*00s|gazelle(\s*indoor)?|yeezy\s*(boost\s*)?\d{3,4}|new\s*balance\s*\d{3,4}[a-z]?|gel-[a-z0-9-]+)\b",
        )
        .unwrap();

        if let Some(m) = re_sneaker.find(titulo) {
            let silhueta = m
                .as_str()
                .to_uppercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");

            let primeira_palavra = titulo
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_uppercase();

            let marcas_conhecidas = ["NIKE", "ADIDAS", "JORDAN", "ASICS", "PUMA", "VANS", "CONVERSE"];
            if marcas_conhecidas.contains(&primeira_palavra.as_str())
                && !silhueta.starts_with(&primeira_palavra)
            {
                return format!("{} {}", primeira_palavra, silhueta);
            }
            return silhueta;
        }
    }

    let base_sem_virgula = titulo.split(',').next().unwrap_or(titulo);
    let base_limpa = base_sem_virgula
        .split(" - ")
        .next()
        .unwrap_or(base_sem_virgula);

    let ignorar = [
        "mouse", "mousepad", "gamer", "sem", "fio", "wireless", "teclado", "mecanico",
        "mecânico", "magnetico", "magnético", "headset", "fone", "ouvido", "monitor",
        "cadeira", "placa", "de", "vídeo", "video", "processador", "memória", "memoria",
        "ssd", "hd", "fonte", "gabinete", "water", "cooler", "tênis", "tenis", "sneaker",
        "sapato", "camiseta", "jaqueta", "moletom", "calça", "calca", "boné", "bone",
        "bolsa", "com", "para", "em", "e", "da", "do", "na", "no", "preto", "branco",
        "rosa", "azul", "vermelho", "cinza", "verde", "bege", "rgb", "led", "usb",
        "bluetooth", "masculino", "feminino", "unissex", "edicao", "edição", "especial",
        "cano", "baixo", "alto", "ultra", "leve", "esports",
    ];

    let palavras_uteis: Vec<String> = base_limpa
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty() && !ignorar.contains(&w.to_lowercase().as_str()))
        .take(5)
        .map(|w| w.to_uppercase())
        .collect();

    if palavras_uteis.is_empty() {
        base_limpa.chars().take(25).collect::<String>().to_uppercase()
    } else {
        palavras_uteis.join(" ")
    }
}

fn avaliar_compatibilidade(modelo_chave: &str, titulo_candidato: &str) -> Option<&'static str> {
    let tokens_alvo: Vec<String> = modelo_chave
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_uppercase())
        .collect();

    if tokens_alvo.is_empty() {
        return None;
    }

    let tokens_cand: Vec<String> = titulo_candidato
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_uppercase())
        .collect();

    let bloqueados = [
        "CADARCO", "CADARÇO", "MEIA", "MEIAS", "LIMPADOR", "PALMILHA", "CHAVEIRO",
        "PROTETOR", "FEET", "SKATES", "GRIP", "TAPE", "DONGLE", "RECEPTOR", "CABO",
        "SWITCHES", "KEYCAPS", "CASE", "CAPA",
    ];
    let alvo_eh_acessorio = tokens_alvo.iter().any(|t| bloqueados.contains(&t.as_str()));
    let cand_eh_acessorio = tokens_cand.iter().any(|t| bloqueados.contains(&t.as_str()));
    if !alvo_eh_acessorio && cand_eh_acessorio {
        return None;
    }

    let mut acertos = 0;
    for alvo in &tokens_alvo {
        if tokens_cand.iter().any(|c| c == alvo) {
            acertos += 1;
        }
    }

    if tokens_alvo.len() >= 2 && acertos < 2 {
        return None;
    }

    let taxa_acerto = acertos as f64 / tokens_alvo.len() as f64;

    if taxa_acerto < 0.65 {
        return None;
    }

    let variantes = [
        "HYPERSPEED", "LITE", "MINI", "TI", "SUPER", "SE", "NEO", "ESSENTIAL", "NEXT",
        "PRO", "SB", "MAX", "PLUS", "8K",
    ];
    let variante_diferente = variantes.iter().any(|&v| {
        let cand_tem = tokens_cand.iter().any(|t| t == v);
        let alvo_tem = tokens_alvo.iter().any(|t| t == v);
        cand_tem != alvo_tem
    });

    if taxa_acerto >= 0.99 && !variante_diferente {
        Some("MESMO MODELO")
    } else {
        Some("SEMELHANTE")
    }
}

// =========================================================================
// BUSCA MULTI-LOJAS, MARKETPLACES E HIGH-END
// =========================================================================

async fn buscar_nas_lojas_confiaveis(
    client: &reqwest::Client,
    alvo: &RegistroPreco,
    nicho: &Nicho,
) -> Vec<OfertaComparada> {
    let mut resultados = Vec::new();
    let termo_encoded = urlencoding::encode(&alvo.modelo_chave).to_string();
    let slug_hifen = alvo.modelo_chave.to_lowercase().replace(' ', "-");

    let fontes: Vec<FonteBusca> = match nicho {
        Nicho::Tecnologia => vec![
            FonteBusca {
                nome: "Kabum",
                categoria: "Varejo Tech",
                url_busca: format!("https://www.kabum.com.br/busca/{}", slug_hifen),
            },
            FonteBusca {
                nome: "Pichau",
                categoria: "Varejo Tech",
                url_busca: format!("https://www.pichau.com.br/search?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Terabyte",
                categoria: "Varejo Tech",
                url_busca: format!("https://www.terabyteshop.com.br/busca?str={}", termo_encoded),
            },
            FonteBusca {
                nome: "Amazon BR",
                categoria: "Marketplace",
                url_busca: format!("https://www.amazon.com.br/s?k={}", termo_encoded),
            },
            FonteBusca {
                nome: "Mercado Livre",
                categoria: "Marketplace",
                url_busca: format!("https://lista.mercadolivre.com.br/{}", slug_hifen),
            },
            FonteBusca {
                nome: "Doctor Mouse",
                categoria: "High-End Gear",
                url_busca: format!("https://www.doctormouse.com.br/search/?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "VTR Imports",
                categoria: "High-End Gear",
                url_busca: format!("https://www.vtrimports.com.br/search/?q={}", termo_encoded),
            },
        ],
        Nicho::Moda => vec![
            FonteBusca {
                nome: "Farfetch",
                categoria: "Luxury & Fashion",
                url_busca: format!(
                    "https://www.farfetch.com/br/shopping/men/search/items.aspx?q={}",
                    termo_encoded
                ),
            },
            FonteBusca {
                nome: "Droper",
                categoria: "Streetwear",
                url_busca: format!("https://droper.app/busca?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Maze",
                categoria: "Streetwear",
                url_busca: format!("https://www.maze.com.br/busca?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Mercado Livre",
                categoria: "Marketplace",
                url_busca: format!("https://lista.mercadolivre.com.br/{}", slug_hifen),
            },
        ],
    };

    let preco_minimo_valido = (alvo.preco * 0.35).max(45.0);

    for fonte in fontes {
        println!("  -> Varrendo [{}] {}...", fonte.categoria, fonte.nome);

        if let Ok(html) = baixar_html_resiliente(client, &fonte.url_busca).await {
            let mut itens = match fonte.nome {
                "Kabum" => extrair_resultados_kabum(&html, &alvo.modelo_chave),
                "Amazon BR" => extrair_resultados_amazon(&html, &alvo.modelo_chave),
                "Mercado Livre" => extrair_resultados_mercadolivre(&html, &alvo.modelo_chave),
                _ => extrair_resultados_lojas_especializadas(
                    &html,
                    fonte.nome,
                    &fonte.url_busca,
                    &alvo.modelo_chave,
                ),
            };

            itens.retain(|i| i.registro.url != alvo.url && i.registro.preco >= preco_minimo_valido);
            resultados.extend(itens);
        }
    }

    resultados
}

fn extrair_resultados_kabum(html: &str, modelo_chave: &str) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let doc = ScraperHtml::parse_document(html);

    let Ok(sel) = Selector::parse(r#"script#__NEXT_DATA__"#) else {
        return encontrados;
    };

    if let Some(script) = doc.select(&sel).next() {
        if let Ok(json) = serde_json::from_str::<Value>(&script.inner_html()) {
            if let Some(data_str) = json["props"]["pageProps"]["data"].as_str() {
                if let Ok(data_json) = serde_json::from_str::<Value>(data_str) {
                    coletar_produtos_kabum_json(&data_json, modelo_chave, &mut encontrados);
                }
            } else {
                coletar_produtos_kabum_json(&json, modelo_chave, &mut encontrados);
            }
        }
    }

    encontrados
}

fn coletar_produtos_kabum_json(val: &Value, modelo_chave: &str, lista: &mut Vec<OfertaComparada>) {
    if lista.len() >= 8 {
        return;
    }
    if let Some(obj) = val.as_object() {
        if let (Some(nome), Some(code)) = (
            obj.get("name").and_then(|v| v.as_str()),
            obj.get("code").and_then(|v| v.as_u64()),
        ) {
            let preco = obj
                .get("priceWithDiscount")
                .and_then(|v| v.as_f64())
                .or_else(|| obj.get("price").and_then(|v| v.as_f64()))
                .unwrap_or(0.0);

            let friendly = obj
                .get("friendlyName")
                .and_then(|v| v.as_str())
                .unwrap_or("produto");

            if preco > 0.0 {
                if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, nome) {
                    let url_produto =
                        format!("https://www.kabum.com.br/produto/{}/{}", code, friendly);
                    if !lista.iter().any(|item| item.registro.url == url_produto) {
                        lista.push(OfertaComparada {
                            registro: RegistroPreco {
                                modelo_chave: modelo_chave.to_string(),
                                titulo_loja: nome.to_string(),
                                loja: "Kabum".to_string(),
                                url: url_produto,
                                preco,
                                data: Utc::now(),
                            },
                            categoria_match,
                        });
                    }
                }
            }
        }
        for v in obj.values() {
            coletar_produtos_kabum_json(v, modelo_chave, lista);
        }
    } else if let Some(arr) = val.as_array() {
        for item in arr {
            coletar_produtos_kabum_json(item, modelo_chave, lista);
        }
    }
}

fn extrair_resultados_amazon(html: &str, modelo_chave: &str) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let doc = ScraperHtml::parse_document(html);

    let Ok(sel_card) = Selector::parse(r#"div[data-component-type="s-search-result"]"#) else {
        return encontrados;
    };
    let sel_titulo = Selector::parse("h2 span").unwrap();
    let sel_link = Selector::parse("h2 a[href], a.a-link-normal[href]").unwrap();
    let sel_preco = Selector::parse("span.a-price span.a-offscreen").unwrap();

    for card in doc.select(&sel_card).take(15) {
        let titulo = card
            .select(&sel_titulo)
            .map(|el| el.text().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        if titulo.is_empty() {
            continue;
        }

        if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, &titulo) {
            if let Some(preco_el) = card.select(&sel_preco).next() {
                let preco_txt = preco_el.text().collect::<Vec<_>>().join("");
                if let Some(preco) = extrair_numero_brl(&preco_txt) {
                    let href = card
                        .select(&sel_link)
                        .next()
                        .and_then(|a| a.value().attr("href"))
                        .unwrap_or("");

                    let url_limpa = if href.starts_with("http") {
                        href.split('?').next().unwrap_or(href).to_string()
                    } else if !href.is_empty() {
                        format!(
                            "https://www.amazon.com.br{}",
                            href.split('?').next().unwrap_or(href)
                        )
                    } else {
                        continue;
                    };

                    if !encontrados.iter().any(|e| e.registro.url == url_limpa) {
                        encontrados.push(OfertaComparada {
                            registro: RegistroPreco {
                                modelo_chave: modelo_chave.to_string(),
                                titulo_loja: titulo,
                                loja: "Amazon BR".to_string(),
                                url: url_limpa,
                                preco,
                                data: Utc::now(),
                            },
                            categoria_match,
                        });
                    }
                }
            }
        }
    }

    encontrados
}

fn extrair_resultados_mercadolivre(html: &str, modelo_chave: &str) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let doc = ScraperHtml::parse_document(html);

    let Ok(sel_card) =
        Selector::parse("li.ui-search-layout__item, div.ui-search-result__wrapper, div.poly-card")
    else {
        return encontrados;
    };
    let sel_titulo = Selector::parse(
        "a.poly-component__title, h2.ui-search-item__title, a.ui-search-link__title-card",
    )
    .unwrap();
    let sel_link =
        Selector::parse("a.poly-component__title[href], a.ui-search-link[href], a[href]").unwrap();
    let sel_frac = Selector::parse(
        ".poly-price__current .andes-money-amount__fraction, .ui-search-price__second-line .andes-money-amount__fraction, .andes-money-amount__fraction",
    )
    .unwrap();
    let sel_cents = Selector::parse(
        ".poly-price__current .andes-money-amount__cents, .ui-search-price__second-line .andes-money-amount__cents",
    )
    .unwrap();

    for card in doc.select(&sel_card).take(15) {
        let titulo = card
            .select(&sel_titulo)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .unwrap_or_default();

        if titulo.is_empty() {
            continue;
        }

        if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, &titulo) {
            if let Some(frac_el) = card.select(&sel_frac).next() {
                let inteiro_str: String = frac_el
                    .text()
                    .collect::<Vec<_>>()
                    .join("")
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect();

                let centavos_str: String = card
                    .select(&sel_cents)
                    .next()
                    .map(|el| {
                        el.text()
                            .collect::<Vec<_>>()
                            .join("")
                            .chars()
                            .filter(|c| c.is_ascii_digit())
                            .collect()
                    })
                    .filter(|s: &String| !s.is_empty())
                    .unwrap_or_else(|| "00".to_string());

                if let Ok(preco) = format!("{}.{}", inteiro_str, centavos_str).parse::<f64>() {
                    let href = card
                        .select(&sel_link)
                        .next()
                        .and_then(|a| a.value().attr("href"))
                        .unwrap_or("");

                    if href.starts_with("http") {
                        let url_limpa = href.split('#').next().unwrap_or(href).to_string();
                        if !encontrados.iter().any(|e| e.registro.url == url_limpa) {
                            encontrados.push(OfertaComparada {
                                registro: RegistroPreco {
                                    modelo_chave: modelo_chave.to_string(),
                                    titulo_loja: titulo,
                                    loja: "Mercado Livre".to_string(),
                                    url: url_limpa,
                                    preco,
                                    data: Utc::now(),
                                },
                                categoria_match,
                            });
                        }
                    }
                }
            }
        }
    }

    encontrados
}

fn extrair_resultados_lojas_especializadas(
    html: &str,
    loja: &str,
    url_base: &str,
    modelo_chave: &str,
) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let doc = ScraperHtml::parse_document(html);
    let dominio_base = obter_dominio_base(loja);

    if let Ok(sel_ld) = Selector::parse(r#"script[type="application/ld+json"]"#) {
        for script in doc.select(&sel_ld) {
            if let Ok(json) = serde_json::from_str::<Value>(&script.inner_html()) {
                if let Some(items) = json["itemListElement"].as_array() {
                    for item in items.iter().take(25) {
                        let prod = if item["item"].is_object() {
                            &item["item"]
                        } else {
                            item
                        };
                        let nome = prod["name"].as_str().unwrap_or("");
                        let href = prod["url"].as_str().unwrap_or(url_base);
                        let link = if href.starts_with("http") {
                            href.to_string()
                        } else {
                            format!("{}{}", dominio_base, href)
                        };

                        if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, nome) {
                            if let Some(preco) = prod.get("offers").and_then(extrair_valor_oferta) {
                                if preco > 0.0 {
                                    encontrados.push(OfertaComparada {
                                        registro: RegistroPreco {
                                            modelo_chave: modelo_chave.to_string(),
                                            titulo_loja: nome.to_string(),
                                            loja: loja.to_string(),
                                            url: link,
                                            preco,
                                            data: Utc::now(),
                                        },
                                        categoria_match,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if encontrados.is_empty() {
        if let Ok(sel_card) = Selector::parse(
            ".js-item-product, .item-product, .card-wrapper, .product-item, .product-card",
        ) {
            let sel_nome = Selector::parse(
                ".js-item-name, .item-name, .card__heading, .product-title, .product-name, h2, h3",
            )
            .unwrap();
            let sel_preco = Selector::parse(
                ".js-price-display, .item-price, .price-item--sale, .price-item--regular, .price",
            )
            .unwrap();
            let sel_link = Selector::parse("a[href]").unwrap();

            for card in doc.select(&sel_card).take(15) {
                let nome = card
                    .select(&sel_nome)
                    .next()
                    .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                    .unwrap_or_default();

                if nome.is_empty() {
                    continue;
                }

                if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, &nome) {
                    let texto_preco = card
                        .select(&sel_preco)
                        .map(|el| el.text().collect::<Vec<_>>().join(" "))
                        .collect::<Vec<_>>()
                        .join(" ");

                    if let Some(preco) = extrair_numero_brl(&texto_preco) {
                        let href = card
                            .select(&sel_link)
                            .next()
                            .and_then(|a| a.value().attr("href"))
                            .unwrap_or("");

                        let url_completa = if href.starts_with("http") {
                            href.to_string()
                        } else if !href.is_empty() {
                            format!("{}{}", dominio_base, href)
                        } else {
                            continue;
                        };

                        if !encontrados.iter().any(|e| e.registro.url == url_completa) {
                            encontrados.push(OfertaComparada {
                                registro: RegistroPreco {
                                    modelo_chave: modelo_chave.to_string(),
                                    titulo_loja: nome,
                                    loja: loja.to_string(),
                                    url: url_completa,
                                    preco,
                                    data: Utc::now(),
                                },
                                categoria_match,
                            });
                        }
                    }
                }
            }
        }
    }

    if encontrados.is_empty() {
        if let Ok(sel_a) = Selector::parse("a[href]") {
            for a in doc.select(&sel_a) {
                if encontrados.len() >= 6 {
                    break;
                }
                let texto = a.text().collect::<Vec<_>>().join(" ");
                if texto.contains("R$") {
                    let titulo_limpo = texto.split("R$").next().unwrap_or("").trim();
                    if let Some(categoria_match) =
                        avaliar_compatibilidade(modelo_chave, titulo_limpo)
                    {
                        if let Some(preco) = extrair_numero_brl(&texto) {
                            let href = a.value().attr("href").unwrap_or("");
                            let url_completa = if href.starts_with("http") {
                                href.to_string()
                            } else {
                                format!("{}{}", dominio_base, href)
                            };

                            if !encontrados.iter().any(|e| e.registro.url == url_completa) {
                                encontrados.push(OfertaComparada {
                                    registro: RegistroPreco {
                                        modelo_chave: modelo_chave.to_string(),
                                        titulo_loja: titulo_limpo.chars().take(90).collect(),
                                        loja: loja.to_string(),
                                        url: url_completa,
                                        preco,
                                        data: Utc::now(),
                                    },
                                    categoria_match,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    encontrados
}

fn extrair_numero_brl(texto: &str) -> Option<f64> {
    let re_brl = Regex::new(r"(\d{1,3}(?:\.\d{3})*,\d{2})").unwrap();
    if let Some(caps) = re_brl.captures(texto) {
        let limpo = caps[1].replace('.', "").replace(',', ".");
        return limpo.parse::<f64>().ok();
    }
    None
}

fn obter_dominio_base(loja: &str) -> &'static str {
    match loja {
        "Kabum" => "https://www.kabum.com.br",
        "Pichau" => "https://www.pichau.com.br",
        "Terabyte" => "https://www.terabyteshop.com.br",
        "Amazon BR" => "https://www.amazon.com.br",
        "Mercado Livre" => "https://www.mercadolivre.com.br",
        "Doctor Mouse" => "https://www.doctormouse.com.br",
        "VTR Imports" => "https://www.vtrimports.com.br",
        "Farfetch" => "https://www.farfetch.com",
        "Droper" => "https://droper.app",
        "Maze" => "https://www.maze.com.br",
        _ => "",
    }
}

fn extrair_preco_html(documento: &ScraperHtml) -> Option<f64> {
    if let Ok(seletor_json_ld) = Selector::parse(r#"script[type="application/ld+json"]"#) {
        for script in documento.select(&seletor_json_ld) {
            let texto_json = script.inner_html();
            if let Ok(json) = serde_json::from_str::<Value>(&texto_json) {
                if let Some(preco) = buscar_oferta_no_json(&json) {
                    if preco > 0.0 {
                        return Some(preco);
                    }
                }
            }
        }
    }

    let seletores_meta = [
        r#"meta[property="product:price:amount"]"#,
        r#"meta[property="og:price:amount"]"#,
        r#"meta[itemprop="price"]"#,
    ];

    for sel_str in seletores_meta {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = documento.select(&sel).next() {
                if let Some(content) = el.value().attr("content") {
                    if let Ok(num) = content.trim().replace(',', ".").parse::<f64>() {
                        if num > 0.0 {
                            return Some(num);
                        }
                    }
                }
            }
        }
    }

    let seletores_visuais = [
        "span.a-price span.a-offscreen",
        ".ui-pdp-price__second-line .andes-money-amount__fraction",
        ".js-price-display",
    ];

    for sel_str in seletores_visuais {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = documento.select(&sel).next() {
                let txt = el.text().collect::<Vec<_>>().join("");
                if let Some(num) = extrair_numero_brl(&txt) {
                    if num > 0.0 {
                        return Some(num);
                    }
                }
                let apenas_digitos: String = txt.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(num) = apenas_digitos.parse::<f64>() {
                    if num > 0.0 {
                        return Some(num);
                    }
                }
            }
        }
    }

    None
}

fn buscar_oferta_no_json(json: &Value) -> Option<f64> {
    if let Some(obj) = json.as_object() {
        if obj.get("@type").and_then(|t| t.as_str()) == Some("Product") {
            if let Some(offers) = obj.get("offers") {
                if let Some(preco) = extrair_valor_oferta(offers) {
                    return Some(preco);
                }
            }
        }
        if let Some(graph) = obj.get("@graph").and_then(|g| g.as_array()) {
            for item in graph {
                if let Some(preco) = buscar_oferta_no_json(item) {
                    return Some(preco);
                }
            }
        }
    } else if let Some(arr) = json.as_array() {
        for item in arr {
            if let Some(preco) = buscar_oferta_no_json(item) {
                return Some(preco);
            }
        }
    }
    None
}

fn extrair_valor_oferta(offers: &Value) -> Option<f64> {
    if offers.is_object() {
        for campo in ["price", "lowPrice"] {
            if let Some(val) = offers.get(campo) {
                if let Some(num) = val.as_f64() {
                    return Some(num);
                }
                if let Some(s) = val.as_str() {
                    if let Ok(num) = s.trim().replace(',', ".").parse::<f64>() {
                        return Some(num);
                    }
                }
            }
        }
    } else if let Some(arr) = offers.as_array() {
        for item in arr {
            if let Some(preco) = extrair_valor_oferta(item) {
                return Some(preco);
            }
        }
    }
    None
}

fn identificar_loja(url: &str) -> &'static str {
    let u = url.to_lowercase();
    if u.contains("kabum") {
        "Kabum"
    } else if u.contains("pichau") {
        "Pichau"
    } else if u.contains("terabyteshop") {
        "Terabyte"
    } else if u.contains("amazon") {
        "Amazon BR"
    } else if u.contains("mercadolivre") {
        "Mercado Livre"
    } else if u.contains("doctormouse") {
        "Doctor Mouse"
    } else if u.contains("vtrimports") {
        "VTR Imports"
    } else if u.contains("farfetch") {
        "Farfetch"
    } else if u.contains("droper") {
        "Droper"
    } else if u.contains("maze") {
        "Maze"
    } else {
        "Loja Externa"
    }
}

fn detectar_nicho(url: &str) -> Nicho {
    let u = url.to_lowercase();
    if u.contains("farfetch") || u.contains("droper") || u.contains("maze") {
        Nicho::Moda
    } else {
        Nicho::Tecnologia
    }
}