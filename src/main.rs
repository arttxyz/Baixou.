use chrono::{DateTime, Duration, Utc};
use headless_chrome::{Browser, LaunchOptions};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::Duration as StdDuration;

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
    categoria_match: &'static str, // "MESMO MODELO" ou "SEMELHANTE"
}

#[derive(Debug, Clone, PartialEq)]
enum Nicho {
    Tecnologia,
    Moda,
}

const ARQUIVO_HISTORICO: &str = "historico_precos.json";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut historico = carregar_historico();
    let client = criar_cliente_http();

    println!("=========================================================");
    println!("   RADAR AUTOMÁTICO BLACK FRIDAY (TECH & FASHION)        ");
    println!("=========================================================");

    loop {
        print!("\nCole o link do produto (ou 'sair' para encerrar): ");
        io::stdout().flush()?;

        let mut url_input = String::new();
        io::stdin().read_line(&mut url_input)?;
        let url = url_input.trim();

        if url.eq_ignore_ascii_case("sair") || url.is_empty() {
            println!("Encerrando o radar. Até a próxima!");
            break;
        }

        let loja_origem = identificar_loja(url);
        let nicho = detectar_nicho(url);

        println!(
            "\n[1/3] Lendo produto na {} (Nicho: {:?})...",
            loja_origem, nicho
        );

        match extrair_produto_da_url(&client, url, loja_origem, &nicho).await {
            Ok(produto_alvo) => {
                println!(
                    "  ✓ Produto: {}\n  ✓ Modelo identificado: {}\n  ✓ Preço atual na {}: R$ {:.2}",
                    produto_alvo.titulo_loja,
                    produto_alvo.modelo_chave,
                    produto_alvo.loja,
                    produto_alvo.preco
                );

                historico.push(produto_alvo.clone());

                println!(
                    "\n[2/3] Varrendo outras lojas confiáveis por '{}'...",
                    produto_alvo.modelo_chave
                );

                let ofertas_encontradas =
                    buscar_nas_lojas_confiaveis(&client, &produto_alvo, &nicho).await;

                for oferta in &ofertas_encontradas {
                    if oferta.categoria_match == "MESMO MODELO" {
                        historico.push(oferta.registro.clone());
                    }
                }

                salvar_historico(&historico)?;

                println!("\n[3/3] Gerando veredito...");
                gerar_veredito_completo(&historico, &produto_alvo, &ofertas_encontradas);
            }
            Err(erro) => {
                println!("  [ERRO] Não foi possível analisar o link enviado: {}", erro);
            }
        }
    }

    Ok(())
}

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
                if !texto.contains("Just a moment...") && texto.len() > 2000 {
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

async fn extrair_produto_da_url(
    client: &reqwest::Client,
    url: &str,
    loja: &str,
    nicho: &Nicho,
) -> Result<RegistroPreco, String> {
    let html_content = baixar_html_resiliente(client, url).await?;
    let documento = Html::parse_document(&html_content);

    let titulo_loja = extrair_titulo_completo(&documento);

    let preco = extrair_preco_html(&documento).ok_or_else(|| {
        "Preço não localizado (item esgotado ou bloqueio Cloudflare ativo na loja).".to_string()
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

// Extrai o título completo mesmo quando a loja (como a Farfetch) coloca só a marca no <h1>
fn extrair_titulo_completo(documento: &Html) -> String {
    let seletor_h1 = Selector::parse("h1").unwrap();
    let titulo_h1 = documento
        .select(&seletor_h1)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .unwrap_or_default();

    // Tenta extrair marca + nome do produto direto do JSON-LD
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

    // Fallback via og:title (limpando sufixos como " | FARFETCH" ou " - Droper")
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

    // Escolhe o título mais descritivo (evita ficar preso só em "Nike" quando o H1 é curto)
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
        // 1. Placas de Vídeo
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

        // 2. Processadores
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
        // 3. Silhuetas clássicas de Sneakers & Streetwear
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

            // Mantém a marca junto da silhueta se ela estiver no início do título (ex: NIKE DUNK LOW)
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

    // 4. Fallback limpo para outros produtos de moda ou periféricos
    let base_sem_virgula = titulo.split(',').next().unwrap_or(titulo);
    let base_limpa = base_sem_virgula
        .split(" - ")
        .next()
        .unwrap_or(base_sem_virgula);

    let ignorar = [
        "mouse", "gamer", "sem", "fio", "wireless", "teclado", "mecanico", "mecânico",
        "headset", "fone", "ouvido", "monitor", "cadeira", "placa", "de", "vídeo", "video",
        "processador", "memória", "memoria", "ssd", "hd", "fonte", "gabinete", "water",
        "cooler", "tênis", "tenis", "sneaker", "sapato", "camiseta", "jaqueta", "moletom",
        "calça", "calca", "boné", "bone", "bolsa", "com", "para", "em", "e", "da", "do",
        "na", "no", "preto", "branco", "rosa", "azul", "vermelho", "cinza", "verde", "bege",
        "rgb", "led", "usb", "bluetooth", "masculino", "feminino", "unissex", "edicao",
        "edição", "especial", "cano", "baixo", "alto",
    ];

    let palavras_uteis: Vec<String> = base_limpa
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty() && !ignorar.contains(&w.to_lowercase().as_str()))
        .take(4)
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

    // Bloqueia acessórios (cadarços, meias, kits de limpeza) quando a busca não é por um acessório
    let acessorios = ["CADARCO", "CADARÇO", "MEIA", "MEIAS", "LIMPADOR", "PALMILHA", "CHAVEIRO", "PROTETOR"];
    let alvo_eh_acessorio = tokens_alvo.iter().any(|t| acessorios.contains(&t.as_str()));
    let cand_eh_acessorio = tokens_cand.iter().any(|t| acessorios.contains(&t.as_str()));
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
        "HYPERSPEED", "LITE", "MINI", "TI", "SUPER", "SE", "NEO", "ESSENTIAL", "NEXT", "PRO", "SB",
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

async fn buscar_nas_lojas_confiaveis(
    client: &reqwest::Client,
    alvo: &RegistroPreco,
    nicho: &Nicho,
) -> Vec<OfertaComparada> {
    let mut resultados = Vec::new();
    let termo_encoded = urlencoding::encode(&alvo.modelo_chave).to_string();
    let slug_kabum = alvo.modelo_chave.to_lowercase().replace(' ', "-");

    let lojas_busca: Vec<(&str, String)> = match nicho {
        Nicho::Tecnologia => vec![
            ("Kabum", format!("https://www.kabum.com.br/busca/{}", slug_kabum)),
            ("Pichau", format!("https://www.pichau.com.br/search?q={}", termo_encoded)),
            ("Terabyte", format!("https://www.terabyteshop.com.br/busca?str={}", termo_encoded)),
        ],
        Nicho::Moda => vec![
            ("Farfetch", format!("https://www.farfetch.com/br/shopping/men/search/items.aspx?q={}", termo_encoded)),
            ("Droper", format!("https://droper.app/busca?q={}", termo_encoded)),
            ("Maze", format!("https://www.maze.com.br/busca?q={}", termo_encoded)),
        ],
    };

    // Piso mínimo de 25% do valor original para evitar brindes/cadarços/caixas vazias
    let preco_minimo_valido = (alvo.preco * 0.25).max(40.0);

    for (loja, url_busca) in lojas_busca {
        print!("  -> Pesquisando na {}... ", loja);
        let _ = io::stdout().flush();

        match baixar_html_resiliente(client, &url_busca).await {
            Ok(html) => {
                let mut itens = if loja == "Kabum" {
                    extrair_resultados_kabum(&html, &alvo.modelo_chave)
                } else {
                    extrair_resultados_genericos(&html, loja, &url_busca, &alvo.modelo_chave)
                };

                itens.retain(|i| {
                    i.registro.url != alvo.url && i.registro.preco >= preco_minimo_valido
                });

                if itens.is_empty() {
                    println!("nenhum anúncio compatível encontrado.");
                } else {
                    println!("{} ofertas compatíveis encontradas!", itens.len());
                    resultados.extend(itens);
                }
            }
            Err(_) => {
                println!("indisponível no momento.");
            }
        }
    }

    resultados
}

fn extrair_resultados_kabum(html: &str, modelo_chave: &str) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let doc = Html::parse_document(html);

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

fn extrair_resultados_genericos(
    html: &str,
    loja: &str,
    url_base: &str,
    modelo_chave: &str,
) -> Vec<OfertaComparada> {
    let mut encontrados = Vec::new();
    let doc = Html::parse_document(html);
    let dominio_base = obter_dominio_base(loja);

    // 1. Tenta via JSON-LD ItemList da busca
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

    // 2. Fallback: varre os cards de links filtrando estritamente pela compatibilidade do modelo
    if encontrados.is_empty() {
        let re_preco = Regex::new(r"R\$\s*(\d{1,3}(?:\.\d{3})*,\d{2})").unwrap();

        if let Ok(sel_a) = Selector::parse("a[href]") {
            for a in doc.select(&sel_a) {
                if encontrados.len() >= 6 {
                    break;
                }
                let texto = a.text().collect::<Vec<_>>().join(" ");
                if let Some(caps) = re_preco.captures(&texto) {
                    let titulo_limpo = texto.split("R$").next().unwrap_or("").trim();

                    if let Some(categoria_match) =
                        avaliar_compatibilidade(modelo_chave, titulo_limpo)
                    {
                        let valor_limpo = caps[1].replace('.', "").replace(',', ".");
                        if let Ok(preco) = valor_limpo.parse::<f64>() {
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

fn obter_dominio_base(loja: &str) -> &'static str {
    match loja {
        "Kabum" => "https://www.kabum.com.br",
        "Pichau" => "https://www.pichau.com.br",
        "Terabyte" => "https://www.terabyteshop.com.br",
        "Farfetch" => "https://www.farfetch.com",
        "Droper" => "https://droper.app",
        "Maze" => "https://www.maze.com.br",
        _ => "",
    }
}

fn extrair_preco_html(documento: &Html) -> Option<f64> {
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

fn carregar_historico() -> Vec<RegistroPreco> {
    if Path::new(ARQUIVO_HISTORICO).exists() {
        if let Ok(conteudo) = fs::read_to_string(ARQUIVO_HISTORICO) {
            if let Ok(dados) = serde_json::from_str::<Vec<RegistroPreco>>(&conteudo) {
                return dados;
            }
        }
    }
    Vec::new()
}

fn salvar_historico(historico: &[RegistroPreco]) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(historico)?;
    fs::write(ARQUIVO_HISTORICO, json)?;
    Ok(())
}

fn gerar_veredito_completo(
    historico: &[RegistroPreco],
    alvo: &RegistroPreco,
    ofertas_outras_lojas: &[OfertaComparada],
) {
    println!("=========================================================");
    println!("  VEREDITO BLACK FRIDAY: {}", alvo.modelo_chave);
    println!("=========================================================");

    let limite_30_dias = Utc::now() - Duration::days(30);

    let registros_30d: Vec<&RegistroPreco> = historico
        .iter()
        .filter(|r| r.modelo_chave == alvo.modelo_chave && r.data >= limite_30_dias)
        .collect();

    let media_30d = if registros_30d.is_empty() {
        alvo.preco
    } else {
        let soma: f64 = registros_30d.iter().map(|r| r.preco).sum();
        soma / registros_30d.len() as f64
    };

    println!("• Seu link ({}): R$ {:.2}", alvo.loja, alvo.preco);
    println!(
        "• Média de 30 dias ({} registros): R$ {:.2}",
        registros_30d.len(),
        media_30d
    );

    let diff_media = alvo.preco - media_30d;
    let perc_media = (diff_media / media_30d) * 100.0;

    if perc_media <= -5.0 {
        println!(
            "  🔥 O PREÇO DO LINK VALE A PENA! Está {:.1}% abaixo da média.",
            perc_media.abs()
        );
    } else if perc_media > 5.0 {
        println!(
            "  ⚠️ ALERTA DE PREÇO INFLADO! Está {:.1}% acima da média.",
            perc_media
        );
    } else {
        println!("  ⚖️ PREÇO DENTRO DA MÉDIA RECENTE.");
    }

    let mut ofertas_mais_baratas: Vec<&OfertaComparada> = ofertas_outras_lojas
        .iter()
        .filter(|o| o.registro.preco < alvo.preco)
        .collect();

    ofertas_mais_baratas.sort_by(|a, b| {
        a.registro
            .preco
            .partial_cmp(&b.registro.preco)
            .unwrap_or(Ordering::Equal)
    });

    if ofertas_mais_baratas.is_empty() {
        println!(
            "\n🏆 MELHOR OPÇÃO ENCONTRADA! Nenhuma outra loja confiável apresentou preço menor para '{}' neste momento.",
            alvo.modelo_chave
        );
    } else {
        println!("\n💡 ACHAMOS OPÇÕES MAIS BARATAS EM LOJAS CONFIÁVEIS:");
        for (idx, oferta) in ofertas_mais_baratas.iter().take(4).enumerate() {
            println!(
                "  {}. [{}] [{}] R$ {:.2} (economia de R$ {:.2})\n     Produto: {}\n     Link: {}",
                idx + 1,
                oferta.categoria_match,
                oferta.registro.loja,
                oferta.registro.preco,
                alvo.preco - oferta.registro.preco,
                oferta.registro.titulo_loja,
                oferta.registro.url
            );
        }
    }
    println!("=========================================================\n");
}