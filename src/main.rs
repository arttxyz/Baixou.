use axum::{
    extract::{Path, State},
    http::{HeaderMap as AxumHeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, patch, post},
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
use tokio::task::JoinSet;
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

#[derive(Clone)]
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

#[derive(Deserialize)]
struct BuscaNomeRequest {
    termo: String,
    nicho: Option<String>,
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
    modo_busca: String,
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
    preco_alvo: Option<f64>,
    webhook_url: Option<String>,
}

#[derive(Deserialize)]
struct AtualizarAlertaRequest {
    preco_alvo: Option<f64>,
    webhook_url: Option<String>,
}

#[derive(Serialize)]
struct FavoritoItem {
    id: i64,
    modelo_chave: String,
    titulo_loja: String,
    loja: String,
    url: String,
    preco_salvo: f64,
    preco_alvo: Option<f64>,
    webhook_url: Option<String>,
    alerta_disparado: bool,
    menor_preco_atual: f64,
    criado_em: String,
}

#[derive(Serialize)]
struct NotificacaoItem {
    id: i64,
    modelo_chave: String,
    titulo_loja: String,
    loja: String,
    url: String,
    preco_encontrado: f64,
    preco_alvo: f64,
    mensagem: String,
    lida: bool,
    criado_em: String,
}

#[derive(Clone)]
struct TarefaMonitoramento {
    favorito_id: i64,
    usuario_id: i64,
    modelo_chave: String,
    titulo_loja: String,
    loja: String,
    url: String,
    preco_alvo: f64,
    webhook_url: Option<String>,
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

    let state_worker = Arc::clone(&state);
    tokio::spawn(async move {
        tokio::time::sleep(StdDuration::from_secs(30)).await;
        loop {
            println!("\n[WORKER] Iniciando varredura automática de alertas de preço-alvo...");
            let disparados = executar_ciclo_monitoramento(&state_worker).await;
            println!(
                "[WORKER] Ciclo concluído. {} novo(s) alerta(s) disparado(s).",
                disparados
            );
            tokio::time::sleep(StdDuration::from_secs(7200)).await;
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(health_check))
        .route("/api/analisar", post(api_analisar_link))
        .route("/api/buscar", post(api_buscar_por_nome))
        .route("/api/historico", get(api_listar_historico))
        .route("/api/grafico/:modelo", get(api_dados_grafico))
        .route("/api/auth/register", post(api_register))
        .route("/api/auth/login", post(api_login))
        .route("/api/auth/me", get(api_me))
        .route("/api/favoritos", get(api_listar_favoritos).post(api_salvar_favorito))
        .route(
            "/api/favoritos/:id",
            patch(api_atualizar_alerta_favorito).delete(api_remover_favorito),
        )
        .route("/api/notificacoes", get(api_listar_notificacoes))
        .route("/api/notificacoes/:id/ler", patch(api_marcar_notificacao_lida))
        .route("/api/notificacoes/ler-todas", post(api_marcar_todas_lidas))
        .route("/api/worker/verificar", post(api_forcar_verificacao_alertas))
        .layer(cors)
        .with_state(state);

    let endereco = "0.0.0.0:3000";
    println!("=========================================================");
    println!("   BAIXOU API v6 ONLINE (PORTA 3000)                     ");
    println!("   • VTEX API + Shopify JSON + Google Shopping + Direct  ");
    println!("   • 12 Lojas Tech & Eletrônicos + 14 Lojas Streetwear   ");
    println!("   • Worker de Alertas de Preço-Alvo: ATIVO              ");
    println!("=========================================================");

    let listener = tokio::net::TcpListener::bind(endereco).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "online",
        "servico": "Baixou API v6 (VTEX + Shopify + Google Shopping + Multi-Lojas)"
    }))
}

// =========================================================================
// MOTOR DE ALERTAS E WORKER EM SEGUNDO PLANO
// =========================================================================

async fn api_forcar_verificacao_alertas(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let total = executar_ciclo_monitoramento(&state).await;
    Json(serde_json::json!({
        "ok": true,
        "alertas_disparados": total,
        "mensagem": format!("Varredura concluída. {} alerta(s) disparado(s).", total)
    }))
}

async fn executar_ciclo_monitoramento(state: &Arc<AppState>) -> usize {
    let tarefas: Vec<TarefaMonitoramento> = {
        let conn = state.db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, usuario_id, modelo_chave, titulo_loja, loja, url, preco_alvo, webhook_url
             FROM favoritos
             WHERE preco_alvo IS NOT NULL AND preco_alvo > 0 AND alerta_disparado = 0",
        ) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let rows = stmt
            .query_map([], |row| {
                Ok(TarefaMonitoramento {
                    favorito_id: row.get(0)?,
                    usuario_id: row.get(1)?,
                    modelo_chave: row.get(2)?,
                    titulo_loja: row.get(3)?,
                    loja: row.get(4)?,
                    url: row.get(5)?,
                    preco_alvo: row.get(6)?,
                    webhook_url: row.get(7)?,
                })
            })
            .ok();

        match rows {
            Some(iter) => iter.filter_map(Result::ok).collect(),
            None => Vec::new(),
        }
    };

    let mut alertas_disparados = 0;

    for tarefa in tarefas {
        let nicho = detectar_nicho(&tarefa.url);
        let loja = identificar_loja(&tarefa.url);

        if let Ok(atualizado) =
            extrair_produto_da_url(&state.client, &tarefa.url, loja, &nicho).await
        {
            {
                let conn = state.db.lock().unwrap();
                let _ = inserir_registro(&conn, &atualizado);
            }

            if atualizado.preco <= tarefa.preco_alvo {
                disparar_alerta_preco(state, &tarefa, atualizado.preco, &atualizado.url).await;
                alertas_disparados += 1;
            }
        }
    }

    alertas_disparados
}

async fn disparar_alerta_preco(
    state: &Arc<AppState>,
    tarefa: &TarefaMonitoramento,
    preco_encontrado: f64,
    url_oferta: &str,
) {
    let msg = format!(
        "Meta atingida! {} baixou para R$ {:.2} na {} (seu alvo era R$ {:.2}).",
        tarefa.modelo_chave, preco_encontrado, tarefa.loja, tarefa.preco_alvo
    );
    let agora = Utc::now().to_rfc3339();

    {
        let conn = state.db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO notificacoes (usuario_id, modelo_chave, titulo_loja, loja, url, preco_encontrado, preco_alvo, mensagem, lida, criado_em)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
            params![
                tarefa.usuario_id,
                tarefa.modelo_chave,
                tarefa.titulo_loja,
                tarefa.loja,
                url_oferta,
                preco_encontrado,
                tarefa.preco_alvo,
                msg,
                agora
            ],
        );

        let _ = conn.execute(
            "UPDATE favoritos SET alerta_disparado = 1 WHERE id = ?1",
            params![tarefa.favorito_id],
        );
    }

    println!("  🔔 [ALERTA DISPARADO] {}", msg);

    if let Some(ref webhook) = tarefa.webhook_url {
        let wh = webhook.trim();
        if wh.starts_with("http") {
            let payload = serde_json::json!({
                "username": "Baixou Radar",
                "embeds": [{
                    "title": format!("🔥 Alerta de Preço-Alvo: {}", tarefa.modelo_chave),
                    "description": msg,
                    "url": url_oferta,
                    "color": 1096065,
                    "fields": [
                        { "name": "Loja", "value": tarefa.loja, "inline": true },
                        { "name": "Preço Atual", "value": format!("R$ {:.2}", preco_encontrado), "inline": true },
                        { "name": "Seu Alvo", "value": format!("R$ {:.2}", tarefa.preco_alvo), "inline": true }
                    ]
                }]
            });

            let _ = state.client.post(wh).json(&payload).send().await;
        }
    }
}

// =========================================================================
// ROTAS DE NOTIFICAÇÕES IN-APP
// =========================================================================

async fn api_listar_notificacoes(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let Some(usuario) = extrair_usuario_autenticado(&headers, &conn) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErroResponse {
                erro: "Faça login para ver suas notificações.".to_string(),
            }),
        )
            .into_response();
    };

    let mut stmt = conn
        .prepare(
            "SELECT id, modelo_chave, titulo_loja, loja, url, preco_encontrado, preco_alvo, mensagem, lida, criado_em
             FROM notificacoes
             WHERE usuario_id = ?1
             ORDER BY id DESC
             LIMIT 25",
        )
        .unwrap();

    let rows = stmt
        .query_map(params![usuario.id], |row| {
            let lida_int: i64 = row.get(8)?;
            Ok(NotificacaoItem {
                id: row.get(0)?,
                modelo_chave: row.get(1)?,
                titulo_loja: row.get(2)?,
                loja: row.get(3)?,
                url: row.get(4)?,
                preco_encontrado: row.get(5)?,
                preco_alvo: row.get(6)?,
                mensagem: row.get(7)?,
                lida: lida_int != 0,
                criado_em: row.get(9)?,
            })
        })
        .unwrap();

    let lista: Vec<NotificacaoItem> = rows.filter_map(Result::ok).collect();
    (StatusCode::OK, Json(lista)).into_response()
}

async fn api_marcar_notificacao_lida(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Path(notif_id): Path<i64>,
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
        "UPDATE notificacoes SET lida = 1 WHERE id = ?1 AND usuario_id = ?2",
        params![notif_id, usuario.id],
    );

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn api_marcar_todas_lidas(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
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
        "UPDATE notificacoes SET lida = 1 WHERE usuario_id = ?1",
        params![usuario.id],
    );

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
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
// FAVORITOS / LISTA DE MONITORAMENTO E METAS DE PREÇO
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
            "SELECT id, modelo_chave, titulo_loja, loja, url, preco_salvo, preco_alvo, webhook_url, alerta_disparado, criado_em
             FROM favoritos
             WHERE usuario_id = ?1
             ORDER BY id DESC",
        )
        .unwrap();

    let rows = stmt
        .query_map(params![usuario.id], |row| {
            let modelo: String = row.get(1)?;
            let preco_salvo: f64 = row.get(5)?;
            let disparado_int: i64 = row.get(8)?;

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
                preco_alvo: row.get(6)?,
                webhook_url: row.get(7)?,
                alerta_disparado: disparado_int != 0,
                menor_preco_atual: menor_atual,
                criado_em: row.get(9)?,
            })
        })
        .unwrap();

    let lista: Vec<FavoritoItem> = rows.filter_map(Result::ok).collect();
    (StatusCode::OK, Json(lista)).into_response()
}

async fn api_salvar_favorito(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<NovoFavoritoRequest>,
) -> impl IntoResponse {
    let tarefa_imediata = {
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
        let webhook_limpo = payload
            .webhook_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);

        if let Err(e) = conn.execute(
            "INSERT INTO favoritos (usuario_id, modelo_chave, titulo_loja, loja, url, preco_salvo, preco_alvo, webhook_url, alerta_disparado, criado_em)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
            params![
                usuario.id,
                payload.modelo_chave,
                payload.titulo_loja,
                payload.loja,
                payload.url,
                payload.preco_salvo,
                payload.preco_alvo,
                webhook_limpo,
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

        let fav_id = conn.last_insert_rowid();

        if let Some(alvo) = payload.preco_alvo {
            if alvo > 0.0 {
                let menor_no_banco: f64 = conn
                    .query_row(
                        "SELECT MIN(preco) FROM historico_precos WHERE modelo_chave = ?1",
                        params![payload.modelo_chave],
                        |r| r.get::<_, Option<f64>>(0),
                    )
                    .unwrap_or(None)
                    .unwrap_or(payload.preco_salvo);

                if menor_no_banco <= alvo {
                    Some((
                        TarefaMonitoramento {
                            favorito_id: fav_id,
                            usuario_id: usuario.id,
                            modelo_chave: payload.modelo_chave.clone(),
                            titulo_loja: payload.titulo_loja.clone(),
                            loja: payload.loja.clone(),
                            url: payload.url.clone(),
                            preco_alvo: alvo,
                            webhook_url: webhook_limpo,
                        },
                        menor_no_banco,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some((tarefa, preco_encontrado)) = tarefa_imediata {
        disparar_alerta_preco(&state, &tarefa, preco_encontrado, &tarefa.url).await;
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

async fn api_atualizar_alerta_favorito(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Path(fav_id): Path<i64>,
    Json(payload): Json<AtualizarAlertaRequest>,
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

    let webhook_limpo = payload
        .webhook_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);

    let _ = conn.execute(
        "UPDATE favoritos
         SET preco_alvo = ?1, webhook_url = ?2, alerta_disparado = 0
         WHERE id = ?3 AND usuario_id = ?4",
        params![payload.preco_alvo, webhook_limpo, fav_id, usuario.id],
    );

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
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
// ANÁLISE DE LINKS, BUSCA DIRETA POR NOME E SÉRIE TEMPORAL PARA GRÁFICOS
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

async fn api_buscar_por_nome(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BuscaNomeRequest>,
) -> impl IntoResponse {
    executar_busca_por_nome(&state, &payload.termo, payload.nicho.as_deref()).await
}

async fn executar_busca_por_nome(
    state: &Arc<AppState>,
    termo_raw: &str,
    nicho_hint: Option<&str>,
) -> axum::response::Response {
    let termo_limpo = termo_raw.trim();
    if termo_limpo.len() < 2 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErroResponse {
                erro: "Digite pelo menos 2 caracteres para buscar um produto.".to_string(),
            }),
        )
            .into_response();
    }

    let nicho = match nicho_hint.map(|s| s.to_lowercase()).as_deref() {
        Some("moda") => Nicho::Moda,
        Some("tech") | Some("tecnologia") => Nicho::Tecnologia,
        _ => detectar_nicho_por_termo(termo_limpo),
    };

    let nicho_str = match nicho {
        Nicho::Tecnologia => "Tecnologia & Eletrônicos",
        Nicho::Moda => "Moda & Streetwear",
    };

    let modelo_chave = extrair_modelo_inteligente(termo_limpo, &nicho);
    println!(
        "\n[API] Busca por Nome [{}]: '{}' -> Chave: '{}'",
        nicho_str, termo_limpo, modelo_chave
    );

    let alvo_ficticio = RegistroPreco {
        modelo_chave: modelo_chave.clone(),
        titulo_loja: termo_limpo.to_string(),
        loja: "Busca Direta".to_string(),
        url: String::new(),
        preco: 0.0,
        data: Utc::now(),
    };

    let mut ofertas_encontradas =
        buscar_nas_lojas_confiaveis(&state.client, &alvo_ficticio, &nicho).await;

    if ofertas_encontradas.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(ErroResponse {
                erro: format!(
                    "Nenhuma oferta encontrada nas lojas monitoradas para '{}'. Tente termos mais diretos (ex: 'RTX 4070', 'iPhone 15', 'Dunk Low', 'Adidas Samba').",
                    termo_limpo
                ),
            }),
        )
            .into_response();
    }

    ofertas_encontradas.sort_by(|a, b| {
        a.registro
            .preco
            .partial_cmp(&b.registro.preco)
            .unwrap_or(Ordering::Equal)
    });

    let melhor_oferta = ofertas_encontradas
        .iter()
        .find(|o| o.categoria_match == "MESMO MODELO")
        .unwrap_or(&ofertas_encontradas[0])
        .clone();

    let (registros_30d, historico_grafico, alertas_pendentes) = {
        let conn = state.db.lock().unwrap();
        for oferta in &ofertas_encontradas {
            if oferta.categoria_match == "MESMO MODELO" {
                let _ = inserir_registro(&conn, &oferta.registro);
            }
        }
        let r30 = buscar_historico_30_dias(&conn, &modelo_chave).unwrap_or_default();
        let graf = buscar_serie_grafico(&conn, &modelo_chave).unwrap_or_default();

        let menor_agora = melhor_oferta.registro.preco;
        let mut stmt = conn
            .prepare(
                "SELECT id, usuario_id, modelo_chave, titulo_loja, loja, url, preco_alvo, webhook_url
                 FROM favoritos
                 WHERE modelo_chave = ?1 AND preco_alvo IS NOT NULL AND preco_alvo >= ?2 AND alerta_disparado = 0",
            )
            .unwrap();

        let pendentes: Vec<(TarefaMonitoramento, f64)> = stmt
            .query_map(params![modelo_chave, menor_agora], |row| {
                Ok((
                    TarefaMonitoramento {
                        favorito_id: row.get(0)?,
                        usuario_id: row.get(1)?,
                        modelo_chave: row.get(2)?,
                        titulo_loja: row.get(3)?,
                        loja: row.get(4)?,
                        url: row.get(5)?,
                        preco_alvo: row.get(6)?,
                        webhook_url: row.get(7)?,
                    },
                    menor_agora,
                ))
            })
            .unwrap()
            .filter_map(Result::ok)
            .collect();

        (r30, graf, pendentes)
    };

    for (tarefa, preco_atingido) in alertas_pendentes {
        disparar_alerta_preco(state, &tarefa, preco_atingido, &melhor_oferta.registro.url).await;
    }

    let media_30d_bruta = if registros_30d.is_empty() {
        melhor_oferta.registro.preco
    } else {
        let soma: f64 = registros_30d.iter().map(|r| r.preco).sum();
        soma / registros_30d.len() as f64
    };

    let diff_media = melhor_oferta.registro.preco - media_30d_bruta;
    let perc_media_bruta = if media_30d_bruta > 0.0 {
        (diff_media / media_30d_bruta) * 100.0
    } else {
        0.0
    };

    let media_30d = (media_30d_bruta * 100.0).round() / 100.0;
    let perc_media = (perc_media_bruta * 10.0).round() / 10.0;

    let (status_veredito, mensagem_veredito) = if perc_media <= -5.0 {
        (
            "PROMOCAO".to_string(),
            format!(
                "Melhor oferta encontrada está {:.1}% abaixo da média histórica de 30 dias.",
                perc_media.abs()
            ),
        )
    } else if perc_media > 5.0 {
        (
            "INFLADO".to_string(),
            format!(
                "Atenção: o menor preço atual está {:.1}% acima da média dos últimos 30 dias.",
                perc_media
            ),
        )
    } else {
        (
            "NORMAL".to_string(),
            format!(
                "{} oferta(s) encontrada(s) nas lojas oficiais e marketplaces monitorados.",
                ofertas_encontradas.len()
            ),
        )
    };

    let maior_preco = ofertas_encontradas
        .iter()
        .map(|o| o.registro.preco)
        .fold(melhor_oferta.registro.preco, f64::max);

    let ofertas_web: Vec<OfertaWeb> = ofertas_encontradas
        .into_iter()
        .take(20)
        .map(|o| OfertaWeb {
            categoria_match: o.categoria_match.to_string(),
            loja: o.registro.loja.clone(),
            titulo_loja: o.registro.titulo_loja.clone(),
            preco: o.registro.preco,
            economia: ((maior_preco - o.registro.preco) * 100.0).round() / 100.0,
            url: o.registro.url.clone(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(AnaliseResponse {
            modo_busca: "NOME".to_string(),
            produto_alvo: melhor_oferta.registro,
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

async fn api_analisar_link(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AnaliseRequest>,
) -> impl IntoResponse {
    let entrada = payload.url.trim().to_string();
    if entrada.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErroResponse {
                erro: "Cole um link de produto ou digite o nome do item que deseja buscar."
                    .to_string(),
            }),
        )
            .into_response();
    }

    if !entrada.starts_with("http://") && !entrada.starts_with("https://") {
        return executar_busca_por_nome(&state, &entrada, None).await;
    }

    let url = entrada;
    let loja_origem = identificar_loja(&url);
    let nicho = detectar_nicho(&url);
    let nicho_str = match nicho {
        Nicho::Tecnologia => "Tecnologia & Eletrônicos",
        Nicho::Moda => "Moda & Streetwear",
    };

    println!("\n[API] Analisando Link: {} ({})", loja_origem, url);

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

    let (registros_30d, historico_grafico, alertas_pendentes) = {
        let conn = state.db.lock().unwrap();
        for oferta in &ofertas_encontradas {
            if oferta.categoria_match == "MESMO MODELO" {
                let _ = inserir_registro(&conn, &oferta.registro);
            }
        }
        let r30 = buscar_historico_30_dias(&conn, &produto_alvo.modelo_chave).unwrap_or_default();
        let graf = buscar_serie_grafico(&conn, &produto_alvo.modelo_chave).unwrap_or_default();

        let menor_agora = r30
            .iter()
            .map(|r| r.preco)
            .fold(produto_alvo.preco, f64::min);

        let mut stmt = conn
            .prepare(
                "SELECT id, usuario_id, modelo_chave, titulo_loja, loja, url, preco_alvo, webhook_url
                 FROM favoritos
                 WHERE modelo_chave = ?1 AND preco_alvo IS NOT NULL AND preco_alvo >= ?2 AND alerta_disparado = 0",
            )
            .unwrap();

        let pendentes: Vec<(TarefaMonitoramento, f64)> = stmt
            .query_map(params![produto_alvo.modelo_chave, menor_agora], |row| {
                Ok((
                    TarefaMonitoramento {
                        favorito_id: row.get(0)?,
                        usuario_id: row.get(1)?,
                        modelo_chave: row.get(2)?,
                        titulo_loja: row.get(3)?,
                        loja: row.get(4)?,
                        url: row.get(5)?,
                        preco_alvo: row.get(6)?,
                        webhook_url: row.get(7)?,
                    },
                    menor_agora,
                ))
            })
            .unwrap()
            .filter_map(Result::ok)
            .collect();

        (r30, graf, pendentes)
    };

    for (tarefa, preco_atingido) in alertas_pendentes {
        disparar_alerta_preco(&state, &tarefa, preco_atingido, &produto_alvo.url).await;
    }

    let media_30d_bruta = if registros_30d.is_empty() {
        produto_alvo.preco
    } else {
        let soma: f64 = registros_30d.iter().map(|r| r.preco).sum();
        soma / registros_30d.len() as f64
    };

    let diff_media = produto_alvo.preco - media_30d_bruta;
    let perc_media_bruta = if media_30d_bruta > 0.0 {
        (diff_media / media_30d_bruta) * 100.0
    } else {
        0.0
    };

    let media_30d = (media_30d_bruta * 100.0).round() / 100.0;
    let perc_media = (perc_media_bruta * 10.0).round() / 10.0;

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
        .take(15)
        .map(|o| OfertaWeb {
            categoria_match: o.categoria_match.to_string(),
            loja: o.registro.loja.clone(),
            titulo_loja: o.registro.titulo_loja.clone(),
            preco: o.registro.preco,
            economia: ((produto_alvo.preco - o.registro.preco) * 100.0).round() / 100.0,
            url: o.registro.url.clone(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(AnaliseResponse {
            modo_busca: "LINK".to_string(),
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
// CAMADA DE BANCO DE DADOS (SQLITE + MIGRAÇÃO AUTOMÁTICA)
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
            preco_alvo REAL,
            webhook_url TEXT,
            alerta_disparado INTEGER NOT NULL DEFAULT 0,
            criado_em TEXT NOT NULL
        )",
        [],
    )?;

    let _ = conn.execute("ALTER TABLE favoritos ADD COLUMN preco_alvo REAL", []);
    let _ = conn.execute("ALTER TABLE favoritos ADD COLUMN webhook_url TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE favoritos ADD COLUMN alerta_disparado INTEGER NOT NULL DEFAULT 0",
        [],
    );

    conn.execute(
        "CREATE TABLE IF NOT EXISTS notificacoes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            usuario_id INTEGER NOT NULL,
            modelo_chave TEXT NOT NULL,
            titulo_loja TEXT NOT NULL,
            loja TEXT NOT NULL,
            url TEXT NOT NULL,
            preco_encontrado REAL NOT NULL,
            preco_alvo REAL NOT NULL,
            mensagem TEXT NOT NULL,
            lida INTEGER NOT NULL DEFAULT 0,
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
        .timeout(StdDuration::from_secs(12))
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

        thread::sleep(StdDuration::from_secs(3));

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
            r"(?i)\b(air\s*jordan\s*\d{1,2}(\s*(low|mid|high))?|dunk\s*(low|high)(\s*retro|\s*sb)?|air\s*force\s*1(\s*(low|mid|high))?|air\s*max\s*\d{2,3}|samba\s*(og|xl)?|campus\s*00s|gazelle(\s*indoor)?|yeezy\s*(boost\s*)?\d{3,4}|new\s*balance\s*\d{3,4}[a-z]?|gel-[a-z0-9-]+|old\s*skool|sk8-hi|knu\s*skool)\b",
        )
        .unwrap();

        if let Some(m) = re_sneaker.find(titulo) {
            let mut silhueta = m
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
                silhueta = format!("{} {}", primeira_palavra, silhueta);
            }

            let ignorar_collab = [
                "tenis", "tênis", "sneaker", "masculino", "feminino", "unissex", "originals",
                "x", "by", "with", "for", "and", "de", "do", "da", "em", "preto", "branco",
                "azul", "verde", "vermelho", "cinza", "bege", "rosa",
            ];
            let extras: Vec<String> = titulo
                .split(|c: char| !c.is_alphanumeric() && c != '-')
                .filter(|w| !w.is_empty())
                .map(|w| w.to_uppercase())
                .filter(|w| {
                    !silhueta.split_whitespace().any(|s| s == w)
                        && !ignorar_collab.contains(&w.to_lowercase().as_str())
                })
                .take(3)
                .collect();

            if !extras.is_empty() {
                return format!("{} {}", silhueta, extras.join(" "));
            }
            return silhueta;
        }
    }

    let titulo_upper = titulo.to_uppercase();
    let plataforma = if titulo_upper.contains("SWITCH 2") {
        Some("SWITCH 2")
    } else if titulo_upper.contains("NINTENDO SWITCH") || titulo_upper.contains("SWITCH") {
        Some("SWITCH")
    } else if titulo_upper.contains("PS5") || titulo_upper.contains("PLAYSTATION 5") {
        Some("PS5")
    } else if titulo_upper.contains("PS4") || titulo_upper.contains("PLAYSTATION 4") {
        Some("PS4")
    } else if titulo_upper.contains("XBOX SERIES") {
        Some("XBOX SERIES")
    } else {
        None
    };

    let base_sem_virgula = titulo.split(',').next().unwrap_or(titulo);
    let base_limpa = base_sem_virgula
        .split(" - ")
        .next()
        .unwrap_or(base_sem_virgula);

    let ignorar = [
        "jogo", "game", "midia", "mídia", "fisica", "física", "digital", "edicao", "edição",
        "standard", "deluxe", "collector", "legendado", "dublado", "portugues", "português",
        "the", "of", "and", "for", "with", "in", "on", "to", "a", "an", "x", "by",
        "mouse", "mousepad", "gamer", "sem", "fio", "wireless", "teclado", "mecanico",
        "mecânico", "magnetico", "magnético", "headset", "fone", "ouvido", "monitor",
        "cadeira", "placa", "de", "vídeo", "video", "processador", "memória", "memoria",
        "ssd", "hd", "fonte", "gabinete", "water", "cooler", "tênis", "tenis", "sneaker",
        "sapato", "camiseta", "jaqueta", "moletom", "calça", "calca", "boné", "bone",
        "bolsa", "com", "para", "em", "e", "da", "do", "das", "dos", "na", "no", "preto",
        "branco", "rosa", "azul", "vermelho", "cinza", "verde", "bege", "rgb", "led", "usb",
        "bluetooth", "masculino", "feminino", "unissex", "especial", "cano", "baixo",
        "alto", "ultra", "leve", "esports", "sony", "microsoft", "nintendo", "switch",
    ];

    let mut palavras_uteis: Vec<String> = base_limpa
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty() && !ignorar.contains(&w.to_lowercase().as_str()))
        .take(8)
        .map(|w| w.to_uppercase())
        .collect();

    if let Some(plat) = plataforma {
        let plat_str = plat.to_string();
        if !palavras_uteis.join(" ").contains(plat) {
            palavras_uteis.push(plat_str);
        }
    }

    if palavras_uteis.is_empty() {
        base_limpa.chars().take(25).collect::<String>().to_uppercase()
    } else {
        palavras_uteis.join(" ")
    }
}

fn normalizar_equivalencia_token(token: &str) -> &str {
    match token {
        "VI" | "6" => "NUM_6",
        "V" | "5" => "NUM_5",
        "IV" | "4" => "NUM_4",
        "III" | "3" => "NUM_3",
        "II" | "2" => "NUM_2",
        "LONGSLEEVE" | "LONGA" => "LONGSLEEVE",
        "TEE" | "TSHIRT" | "CAMISETA" => "CAMISETA",
        "HOODIE" | "MOLETOM" => "MOLETOM",
        "JACKET" | "JAQUETA" => "JAQUETA",
        "SNEAKER" | "TENIS" | "TÊNIS" => "TENIS",
        _ => token,
    }
}

fn eh_token_versao_ou_plataforma(token: &str) -> bool {
    matches!(
        token,
        "NUM_6"
            | "NUM_5"
            | "NUM_4"
            | "NUM_3"
            | "NUM_2"
            | "PS5"
            | "PS4"
            | "PS3"
            | "XBOX"
            | "SWITCH"
    ) || token.chars().all(|c| c.is_ascii_digit())
}

fn avaliar_compatibilidade(modelo_chave: &str, titulo_candidato: &str) -> Option<&'static str> {
    let stopwords_match = [
        "THE", "OF", "AND", "A", "DE", "DO", "DA", "E", "JOGO", "GAME", "X", "BY", "ORIGINALS",
    ];

    let tokens_alvo: Vec<String> = modelo_chave
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !stopwords_match.contains(&w.to_uppercase().as_str()))
        .map(|w| normalizar_equivalencia_token(&w.to_uppercase()).to_string())
        .collect();

    if tokens_alvo.is_empty() {
        return None;
    }

    let tokens_cand: Vec<String> = titulo_candidato
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !stopwords_match.contains(&w.to_uppercase().as_str()))
        .map(|w| normalizar_equivalencia_token(&w.to_uppercase()).to_string())
        .collect();

    let bloqueados = [
        "CADARCO", "CADARÇO", "MEIA", "MEIAS", "LIMPADOR", "PALMILHA", "CHAVEIRO",
        "PROTETOR", "FEET", "SKATES", "GRIP", "TAPE", "DONGLE", "RECEPTOR", "CABO",
        "SWITCHES", "KEYCAPS", "CASE", "CAPA", "GUIA", "GUIDE", "WALKTHROUGH",
        "STRATEGY", "HANDBOOK", "COMPANION", "MANUAL", "BOOK", "LIVRO", "ARTBOOK",
        "POSTER", "ADESIVO", "AMIIBO", "PELICULA", "PELÍCULA", "CONTROLE", "JOYCON",
    ];
    let alvo_eh_acessorio = tokens_alvo.iter().any(|t| bloqueados.contains(&t.as_str()));
    let cand_eh_acessorio = tokens_cand.iter().any(|t| bloqueados.contains(&t.as_str()));
    if !alvo_eh_acessorio && cand_eh_acessorio {
        return None;
    }

    for alvo in &tokens_alvo {
        if eh_token_versao_ou_plataforma(alvo) && !tokens_cand.iter().any(|c| c == alvo) {
            return None;
        }
    }

    let versoes_alvo: Vec<&String> = tokens_alvo
        .iter()
        .filter(|t| t.starts_with("NUM_"))
        .collect();
    if !versoes_alvo.is_empty() {
        for cand in &tokens_cand {
            if cand.starts_with("NUM_") && !versoes_alvo.contains(&cand) {
                return None;
            }
        }
    } else if tokens_cand.iter().any(|c| c == "NUM_2") && tokens_alvo.iter().any(|t| t == "SWITCH") {
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
    if taxa_acerto < 0.75 {
        return None;
    }

    let variantes = [
        "HYPERSPEED", "LITE", "MINI", "TI", "SUPER", "SE", "NEO", "ESSENTIAL", "NEXT",
        "PRO", "SB", "MAX", "PLUS", "8K", "DELUXE", "COLLECTOR", "REMAKE", "REMASTERED",
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
// BUSCA MULTI-LOJAS PARALELA (VTEX API + SHOPIFY JSON + GOOGLE SHOPPING)
// =========================================================================

async fn buscar_nas_lojas_confiaveis(
    client: &reqwest::Client,
    alvo: &RegistroPreco,
    nicho: &Nicho,
) -> Vec<OfertaComparada> {
    let termo_encoded = urlencoding::encode(&alvo.modelo_chave).to_string();
    let slug_hifen = alvo.modelo_chave.to_lowercase().replace(' ', "-");

    let fontes: Vec<FonteBusca> = match nicho {
        Nicho::Tecnologia => vec![
            FonteBusca {
                nome: "Google Shopping",
                categoria: "Agregador Verificado",
                url_busca: format!(
                    "https://www.google.com.br/search?tbm=shop&hl=pt-BR&gl=br&q={}",
                    termo_encoded
                ),
            },
            FonteBusca {
                nome: "KaBuM!",
                categoria: "Varejo Tech",
                url_busca: format!("https://www.kabum.com.br/busca/{}", slug_hifen),
            },
            FonteBusca {
                nome: "Amazon BR",
                categoria: "Marketplace Oficial",
                url_busca: format!("https://www.amazon.com.br/s?k={}", termo_encoded),
            },
            FonteBusca {
                nome: "Mercado Livre",
                categoria: "Marketplace Oficial",
                url_busca: format!("https://lista.mercadolivre.com.br/{}", slug_hifen),
            },
            FonteBusca {
                nome: "Fast Shop",
                categoria: "Varejo Premium",
                url_busca: format!("https://site.fastshop.com.br/busca?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Magalu",
                categoria: "Grande Varejo",
                url_busca: format!("https://www.magazineluiza.com.br/busca/{}/", termo_encoded),
            },
            FonteBusca {
                nome: "Casas Bahia",
                categoria: "Grande Varejo",
                url_busca: format!("https://www.casasbahia.com.br/{}/b", slug_hifen),
            },
            FonteBusca {
                nome: "Ponto",
                categoria: "Grande Varejo",
                url_busca: format!("https://www.pontofrio.com.br/{}/b", slug_hifen),
            },
            FonteBusca {
                nome: "iPlace",
                categoria: "Apple Premium Reseller",
                url_busca: format!("https://www.iplace.com.br/busca?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Pichau",
                categoria: "Hardware & Gamer",
                url_busca: format!("https://www.pichau.com.br/search?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Terabyte",
                categoria: "Hardware & Gamer",
                url_busca: format!("https://www.terabyteshop.com.br/busca?str={}", termo_encoded),
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
                nome: "Google Shopping",
                categoria: "Agregador Verificado",
                url_busca: format!(
                    "https://www.google.com.br/search?tbm=shop&hl=pt-BR&gl=br&q={}",
                    termo_encoded
                ),
            },
            FonteBusca {
                nome: "Artwalk",
                categoria: "Revendedor Oficial",
                url_busca: format!("https://www.artwalk.com.br/busca?ft={}", termo_encoded),
            },
            FonteBusca {
                nome: "Guadalupe Store",
                categoria: "Revendedor Oficial",
                url_busca: format!("https://www.guadalupestore.com.br/busca?ft={}", termo_encoded),
            },
            FonteBusca {
                nome: "Your ID",
                categoria: "Revendedor Oficial",
                url_busca: format!("https://www.youridstore.com.br/busca?ft={}", termo_encoded),
            },
            FonteBusca {
                nome: "Maze",
                categoria: "Revendedor Oficial",
                url_busca: format!("https://www.maze.com.br/busca?ft={}", termo_encoded),
            },
            FonteBusca {
                nome: "Ambiente Skate Shop",
                categoria: "Revendedor Oficial SB",
                url_busca: format!("https://www.ambienteskateshop.com.br/search?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Pineapple Co.",
                categoria: "Streetwear Oficial",
                url_busca: format!("https://www.pineappleco.com.br/search?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Working Title",
                categoria: "Butique Curada",
                url_busca: format!("https://www.workingtitle.com.br/search?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Nike SNKRS",
                categoria: "Loja Oficial Marca",
                url_busca: format!("https://www.nike.com.br/busca?term={}", termo_encoded),
            },
            FonteBusca {
                nome: "Adidas Brasil",
                categoria: "Loja Oficial Marca",
                url_busca: format!("https://www.adidas.com.br/search?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "New Balance Brasil",
                categoria: "Loja Oficial Marca",
                url_busca: format!("https://www.newbalance.com.br/busca?ft={}", termo_encoded),
            },
            FonteBusca {
                nome: "Vans Brasil",
                categoria: "Loja Oficial Marca",
                url_busca: format!("https://www.vans.com.br/busca?ft={}", termo_encoded),
            },
            FonteBusca {
                nome: "Droper",
                categoria: "Resell Autenticado",
                url_busca: format!("https://droper.app/busca?q={}", termo_encoded),
            },
            FonteBusca {
                nome: "Farfetch Brasil",
                categoria: "Luxury & Streetwear",
                url_busca: format!(
                    "https://www.farfetch.com/br/shopping/men/search/items.aspx?q={}",
                    termo_encoded
                ),
            },
            FonteBusca {
                nome: "Mercado Livre",
                categoria: "Marketplace",
                url_busca: format!("https://lista.mercadolivre.com.br/{}", slug_hifen),
            },
        ],
    };

    let preco_minimo_valido = if alvo.preco > 0.0 {
        (alvo.preco * 0.35).max(45.0)
    } else {
        35.0
    };

    let mut set = JoinSet::new();
    for fonte in fontes {
        let client_clone = client.clone();
        let modelo_chave = alvo.modelo_chave.clone();
        let url_alvo = alvo.url.clone();

        set.spawn(async move {
            println!("  -> Varrendo [{}] {}...", fonte.categoria, fonte.nome);

            if matches!(
                fonte.nome,
                "Artwalk"
                    | "Guadalupe Store"
                    | "Your ID"
                    | "Maze"
                    | "Fast Shop"
                    | "iPlace"
                    | "New Balance Brasil"
                    | "Vans Brasil"
            ) {
                let vtex_items = buscar_api_vtex(&client_clone, fonte.nome, &modelo_chave).await;
                if !vtex_items.is_empty() {
                    return vtex_items
                        .into_iter()
                        .filter(|i| {
                            (url_alvo.is_empty() || i.registro.url != url_alvo)
                                && i.registro.preco >= preco_minimo_valido
                        })
                        .collect::<Vec<_>>();
                }
            }

            if matches!(
                fonte.nome,
                "Ambiente Skate Shop" | "Pineapple Co." | "Working Title"
            ) {
                let shopify_items =
                    buscar_shopify_suggest(&client_clone, fonte.nome, &modelo_chave).await;
                if !shopify_items.is_empty() {
                    return shopify_items
                        .into_iter()
                        .filter(|i| {
                            (url_alvo.is_empty() || i.registro.url != url_alvo)
                                && i.registro.preco >= preco_minimo_valido
                        })
                        .collect::<Vec<_>>();
                }
            }

            if fonte.nome == "Google Shopping" {
                let gshop_items =
                    buscar_google_shopping(&client_clone, &fonte.url_busca, &modelo_chave).await;
                return gshop_items
                    .into_iter()
                    .filter(|i| {
                        (url_alvo.is_empty() || i.registro.url != url_alvo)
                            && i.registro.preco >= preco_minimo_valido
                    })
                    .collect::<Vec<_>>();
            }

            if let Ok(html) = baixar_html_resiliente(&client_clone, &fonte.url_busca).await {
                let mut itens = match fonte.nome {
                    "KaBuM!" => extrair_resultados_kabum(&html, &modelo_chave),
                    "Amazon BR" => extrair_resultados_amazon(&html, &modelo_chave),
                    "Mercado Livre" => extrair_resultados_mercadolivre(&html, &modelo_chave),
                    _ => extrair_resultados_universais(
                        &html,
                        fonte.nome,
                        &fonte.url_busca,
                        &modelo_chave,
                    ),
                };

                itens.retain(|i| {
                    (url_alvo.is_empty() || i.registro.url != url_alvo)
                        && i.registro.preco >= preco_minimo_valido
                });
                itens
            } else {
                Vec::new()
            }
        });
    }

    let mut resultados = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(itens) = res {
            for item in itens {
                if !resultados.iter().any(|r: &OfertaComparada| {
                    r.registro.loja == item.registro.loja
                        && (r.registro.preco - item.registro.preco).abs() < 0.05
                        && r.registro.titulo_loja == item.registro.titulo_loja
                }) {
                    resultados.push(item);
                }
            }
        }
    }

    resultados
}

async fn buscar_api_vtex(
    client: &reqwest::Client,
    loja: &str,
    modelo_chave: &str,
) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let dominio = obter_dominio_base(loja);
    if dominio.is_empty() {
        return encontrados;
    }

    let endpoint = format!(
        "{}/api/catalog_system/pub/products/search/?ft={}&_from=0&_to=14",
        dominio,
        urlencoding::encode(modelo_chave)
    );

    let Ok(resp) = client.get(&endpoint).send().await else {
        return encontrados;
    };
    let Ok(json) = resp.json::<Value>().await else {
        return encontrados;
    };

    if let Some(produtos) = json.as_array() {
        for prod in produtos {
            let titulo = prod["productName"].as_str().unwrap_or("");
            let link = prod["link"].as_str().unwrap_or("");

            let mut menor_preco = f64::MAX;
            if let Some(items) = prod["items"].as_array() {
                for sku in items {
                    if let Some(sellers) = sku["sellers"].as_array() {
                        for seller in sellers {
                            let oferta = &seller["commertialOffer"];
                            let disp = oferta["AvailableQuantity"].as_i64().unwrap_or(0);
                            let p = oferta["Price"].as_f64().unwrap_or(0.0);
                            if disp > 0 && p > 0.0 && p < menor_preco {
                                menor_preco = p;
                            }
                        }
                    }
                }
            }

            if menor_preco < f64::MAX && menor_preco > 0.0 {
                if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, titulo) {
                    let url_final = if link.starts_with("http") {
                        link.to_string()
                    } else {
                        format!("{}{}", dominio, link)
                    };
                    encontrados.push(OfertaComparada {
                        registro: RegistroPreco {
                            modelo_chave: modelo_chave.to_string(),
                            titulo_loja: titulo.to_string(),
                            loja: loja.to_string(),
                            url: url_final,
                            preco: menor_preco,
                            data: Utc::now(),
                        },
                        categoria_match,
                    });
                }
            }
        }
    }

    encontrados
}

async fn buscar_google_shopping(
    client: &reqwest::Client,
    url_busca: &str,
    modelo_chave: &str,
) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let Ok(html) = baixar_html_resiliente(client, url_busca).await else {
        return encontrados;
    };

    let doc = ScraperHtml::parse_document(&html);

    let lojas_permitidas = [
        ("artwalk", "Artwalk"),
        ("droper", "Droper"),
        ("guadalupe", "Guadalupe Store"),
        ("your id", "Your ID"),
        ("yourid", "Your ID"),
        ("maze", "Maze"),
        ("ambiente", "Ambiente Skate Shop"),
        ("pineapple", "Pineapple Co."),
        ("working title", "Working Title"),
        ("nike", "Nike SNKRS"),
        ("adidas", "Adidas Brasil"),
        ("new balance", "New Balance Brasil"),
        ("vans", "Vans Brasil"),
        ("farfetch", "Farfetch Brasil"),
        ("kabum", "KaBuM!"),
        ("amazon", "Amazon BR"),
        ("mercado livre", "Mercado Livre"),
        ("fast shop", "Fast Shop"),
        ("magalu", "Magalu"),
        ("magazine luiza", "Magalu"),
        ("casas bahia", "Casas Bahia"),
        ("ponto", "Ponto"),
        ("iplace", "iPlace"),
        ("pichau", "Pichau"),
        ("terabyte", "Terabyte"),
    ];

    if let Ok(sel_cards) = Selector::parse(
        "div.sh-dgr__content, div.sh-dlr__list-result, div.KZmu8e, div.pla-unit",
    ) {
        let sel_titulo = Selector::parse("h3, h4, .tAxDx, .pla-unit-title").unwrap();
        let sel_preco = Selector::parse("span.a8Pemb, span.OFFNJ, .e10twf, .T14wmb").unwrap();
        let sel_loja = Selector::parse("div.aULzUe, div.IuHnof, .LbUacb, .mnIHsc").unwrap();
        let sel_link = Selector::parse("a[href]").unwrap();

        for card in doc.select(&sel_cards).take(20) {
            let titulo = card
                .select(&sel_titulo)
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .unwrap_or_default();

            let loja_bruta = card
                .select(&sel_loja)
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .unwrap_or_default();

            let loja_lower = loja_bruta.to_lowercase();
            let Some((_, loja_oficial)) = lojas_permitidas
                .iter()
                .find(|(chave, _)| loja_lower.contains(chave))
            else {
                continue;
            };

            if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, &titulo) {
                let preco_txt = card
                    .select(&sel_preco)
                    .map(|el| el.text().collect::<Vec<_>>().join(" "))
                    .collect::<Vec<_>>()
                    .join(" ");

                if let Some(preco) = extrair_numero_brl(&preco_txt) {
                    let href = card
                        .select(&sel_link)
                        .next()
                        .and_then(|a| a.value().attr("href"))
                        .unwrap_or("");

                    let url_final = if href.starts_with("http") {
                        href.to_string()
                    } else if href.starts_with("/url?q=") {
                        href.trim_start_matches("/url?q=")
                            .split('&')
                            .next()
                            .unwrap_or(url_busca)
                            .to_string()
                    } else {
                        url_busca.to_string()
                    };

                    encontrados.push(OfertaComparada {
                        registro: RegistroPreco {
                            modelo_chave: modelo_chave.to_string(),
                            titulo_loja: titulo,
                            loja: loja_oficial.to_string(),
                            url: url_final,
                            preco,
                            data: Utc::now(),
                        },
                        categoria_match,
                    });
                }
            }
        }
    }

    encontrados
}

async fn buscar_shopify_suggest(
    client: &reqwest::Client,
    loja: &str,
    modelo_chave: &str,
) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let dominio = obter_dominio_base(loja);
    if dominio.is_empty() {
        return encontrados;
    }

    let endpoint = format!(
        "{}/search/suggest.json?q={}&resources[type]=product&resources[limit]=10",
        dominio,
        urlencoding::encode(modelo_chave)
    );

    let Ok(resp) = client.get(&endpoint).send().await else {
        return encontrados;
    };
    let Ok(json) = resp.json::<Value>().await else {
        return encontrados;
    };

    if let Some(produtos) = json["resources"]["results"]["products"].as_array() {
        for prod in produtos {
            let titulo = prod["title"].as_str().unwrap_or("");
            let url_rel = prod["url"].as_str().unwrap_or("");
            let preco_str = prod["price"].as_str().unwrap_or("0");
            let preco = preco_str.replace(',', ".").parse::<f64>().unwrap_or(0.0);

            if preco > 0.0 {
                if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, titulo) {
                    let url_full = if url_rel.starts_with("http") {
                        url_rel.to_string()
                    } else {
                        format!("{}{}", dominio, url_rel)
                    };
                    encontrados.push(OfertaComparada {
                        registro: RegistroPreco {
                            modelo_chave: modelo_chave.to_string(),
                            titulo_loja: titulo.to_string(),
                            loja: loja.to_string(),
                            url: url_full,
                            preco,
                            data: Utc::now(),
                        },
                        categoria_match,
                    });
                }
            }
        }
    }

    encontrados
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
                                loja: "KaBuM!".to_string(),
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

fn extrair_resultados_universais(
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
        if let Ok(sel_scripts) = Selector::parse("script") {
            for script in doc.select(&sel_scripts) {
                let txt = script.inner_html();
                if txt.contains("productName")
                    || txt.contains("sellingPrice")
                    || txt.contains("lowestPrice")
                    || txt.contains("formattedPrice")
                {
                    if let Ok(json) = serde_json::from_str::<Value>(&txt) {
                        coletar_produtos_json_universal(
                            &json,
                            loja,
                            dominio_base,
                            modelo_chave,
                            &mut encontrados,
                        );
                    }
                }
            }
        }
    }

    if encontrados.is_empty() {
        if let Ok(sel_card) = Selector::parse(
            ".js-item-product, .item-product, .card-wrapper, .product-item, .product-card, [data-testid='product-card'], [data-testid='product-card-container'], .vtex-search-result-3-x-galleryItem, .product-grid-item",
        ) {
            let sel_nome = Selector::parse(
                ".js-item-name, .item-name, .card__heading, .product-title, .product-name, [data-testid='product-title'], .vtex-product-summary-2-x-productBrand, h2, h3",
            )
            .unwrap();
            let sel_preco = Selector::parse(
                ".js-price-display, .item-price, .price-item--sale, .price-item--regular, [data-testid='price-value'], .vtex-product-price-1-x-sellingPriceValue, .price",
            )
            .unwrap();
            let sel_link = Selector::parse("a[href]").unwrap();

            for card in doc.select(&sel_card).take(18) {
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
                if encontrados.len() >= 8 {
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
                                        titulo_loja: titulo_limpo.chars().take(100).collect(),
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

fn coletar_produtos_json_universal(
    val: &Value,
    loja: &str,
    dominio_base: &str,
    modelo_chave: &str,
    lista: &mut Vec<OfertaComparada>,
) {
    if lista.len() >= 10 {
        return;
    }
    if let Some(obj) = val.as_object() {
        let nome_opt = obj
            .get("productName")
            .or_else(|| obj.get("name"))
            .or_else(|| obj.get("title"))
            .and_then(|v| v.as_str());

        if let Some(nome) = nome_opt {
            let preco = obj
                .get("sellingPrice")
                .or_else(|| obj.get("lowestPrice"))
                .or_else(|| obj.get("spotPrice"))
                .or_else(|| obj.get("price"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            let preco_ajustado = if preco > 50000.0 {
                preco / 100.0
            } else {
                preco
            };

            if preco_ajustado > 35.0 {
                if let Some(categoria_match) = avaliar_compatibilidade(modelo_chave, nome) {
                    let link_raw = obj
                        .get("link")
                        .or_else(|| obj.get("linkText"))
                        .or_else(|| obj.get("slug"))
                        .or_else(|| obj.get("url"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    let url_produto = if link_raw.starts_with("http") {
                        link_raw.to_string()
                    } else if !link_raw.is_empty() {
                        let clean = link_raw.trim_start_matches('/');
                        if clean.ends_with("/p") {
                            format!("{}/{}", dominio_base, clean)
                        } else {
                            format!("{}/{}/p", dominio_base, clean)
                        }
                    } else {
                        dominio_base.to_string()
                    };

                    if !lista
                        .iter()
                        .any(|e| e.registro.url == url_produto && e.registro.titulo_loja == nome)
                    {
                        lista.push(OfertaComparada {
                            registro: RegistroPreco {
                                modelo_chave: modelo_chave.to_string(),
                                titulo_loja: nome.to_string(),
                                loja: loja.to_string(),
                                url: url_produto,
                                preco: preco_ajustado,
                                data: Utc::now(),
                            },
                            categoria_match,
                        });
                    }
                }
            }
        }

        for v in obj.values() {
            coletar_produtos_json_universal(v, loja, dominio_base, modelo_chave, lista);
        }
    } else if let Some(arr) = val.as_array() {
        for item in arr {
            coletar_produtos_json_universal(item, loja, dominio_base, modelo_chave, lista);
        }
    }
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
        "KaBuM!" | "Kabum" => "https://www.kabum.com.br",
        "Pichau" => "https://www.pichau.com.br",
        "Terabyte" => "https://www.terabyteshop.com.br",
        "Amazon BR" => "https://www.amazon.com.br",
        "Mercado Livre" => "https://www.mercadolivre.com.br",
        "Fast Shop" => "https://site.fastshop.com.br",
        "Magalu" => "https://www.magazineluiza.com.br",
        "Casas Bahia" => "https://www.casasbahia.com.br",
        "Ponto" => "https://www.pontofrio.com.br",
        "iPlace" => "https://www.iplace.com.br",
        "Doctor Mouse" => "https://www.doctormouse.com.br",
        "VTR Imports" => "https://www.vtrimports.com.br",
        "Guadalupe Store" => "https://www.guadalupestore.com.br",
        "Your ID" => "https://www.youridstore.com.br",
        "Maze" => "https://www.maze.com.br",
        "Artwalk" => "https://www.artwalk.com.br",
        "Ambiente Skate Shop" => "https://www.ambienteskateshop.com.br",
        "Pineapple Co." => "https://www.pineappleco.com.br",
        "Working Title" => "https://www.workingtitle.com.br",
        "Nike SNKRS" => "https://www.nike.com.br",
        "Adidas Brasil" => "https://www.adidas.com.br",
        "New Balance Brasil" => "https://www.newbalance.com.br",
        "Vans Brasil" => "https://www.vans.com.br",
        "Droper" => "https://droper.app",
        "Farfetch Brasil" | "Farfetch" => "https://www.farfetch.com",
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
        "[data-testid='price-value']",
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
        "KaBuM!"
    } else if u.contains("pichau") {
        "Pichau"
    } else if u.contains("terabyteshop") {
        "Terabyte"
    } else if u.contains("amazon") {
        "Amazon BR"
    } else if u.contains("mercadolivre") {
        "Mercado Livre"
    } else if u.contains("fastshop") {
        "Fast Shop"
    } else if u.contains("magazineluiza") || u.contains("magalu") {
        "Magalu"
    } else if u.contains("casasbahia") {
        "Casas Bahia"
    } else if u.contains("pontofrio") {
        "Ponto"
    } else if u.contains("iplace") {
        "iPlace"
    } else if u.contains("doctormouse") {
        "Doctor Mouse"
    } else if u.contains("vtrimports") {
        "VTR Imports"
    } else if u.contains("guadalupestore") {
        "Guadalupe Store"
    } else if u.contains("youridstore") {
        "Your ID"
    } else if u.contains("maze") {
        "Maze"
    } else if u.contains("artwalk") {
        "Artwalk"
    } else if u.contains("ambienteskateshop") {
        "Ambiente Skate Shop"
    } else if u.contains("pineappleco") {
        "Pineapple Co."
    } else if u.contains("workingtitle") {
        "Working Title"
    } else if u.contains("nike.com.br") {
        "Nike SNKRS"
    } else if u.contains("adidas.com.br") {
        "Adidas Brasil"
    } else if u.contains("newbalance.com.br") {
        "New Balance Brasil"
    } else if u.contains("vans.com.br") {
        "Vans Brasil"
    } else if u.contains("droper") {
        "Droper"
    } else if u.contains("farfetch") {
        "Farfetch Brasil"
    } else {
        "Loja Externa"
    }
}

fn detectar_nicho(url: &str) -> Nicho {
    let u = url.to_lowercase();
    if u.contains("farfetch")
        || u.contains("droper")
        || u.contains("maze")
        || u.contains("guadalupestore")
        || u.contains("youridstore")
        || u.contains("artwalk")
        || u.contains("ambienteskateshop")
        || u.contains("pineappleco")
        || u.contains("workingtitle")
        || u.contains("nike.com.br")
        || u.contains("adidas.com.br")
        || u.contains("newbalance.com.br")
        || u.contains("vans.com.br")
    {
        Nicho::Moda
    } else {
        Nicho::Tecnologia
    }
}

fn detectar_nicho_por_termo(termo: &str) -> Nicho {
    let t = termo.to_lowercase();
    let palavras_moda = [
        "tenis", "tênis", "sneaker", "nike", "adidas", "jordan", "dunk", "yeezy",
        "samba", "campus", "gazelle", "new balance", "asics", "vans", "jaqueta",
        "moletom", "camiseta", "hoodie", "streetwear", "bolsa", "chrome hearts",
        "bape", "off-white", "palm angels", "piet", "pace", "class",
    ];
    if palavras_moda.iter().any(|p| t.contains(p)) {
        Nicho::Moda
    } else {
        Nicho::Tecnologia
    }
}