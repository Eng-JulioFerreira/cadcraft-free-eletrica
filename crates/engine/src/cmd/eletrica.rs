// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Free Elétrica: as ferramentas de projeto elétrico que trabalham no desenho.
//!
//! ELECIRCUITO (Criar circuito): selecione o quadro de destino, informe o número do circuito
//! (Enter aceita o próximo livre) e selecione os pontos, um a um, até Enter.
//! ELECOMANDO (Comandos e luminárias): selecione o interruptor e depois as luminárias que ele
//! comanda, até Enter. As luminárias recebem a letra do interruptor e o circuito dele.
//!
//! ELEFIACAO (Ajustar fiação): selecione o quadro (Enter: todos) e as marcas de fiação são
//! (re)desenhadas sobre os eletrodutos.
//!
//! Os valores vão para os atributos do bloco quando existem e, senão, para o XDATA do Free Elétrica
//! (`free_eletrica::editar`). Cada comando é um passo de desfazer.
//!
//! Os comandos `eletrica.*` dão em JSON tudo o que a paleta mostra (análise, roteamento,
//! configurações) e a sobrescrita manual: o motor de cálculo inteiro é acessível sem a interface
//! (linha de comando, scripts, canal de controle, `cadcraft-cli`, MCP).

use cadcraft_doc::{EntityKind, Handle};
use free_eletrica::{self as ele, PontoEletrico, TipoPonto};
use serde_json::{Value, json};

use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("elecircuito", "Electrical Circuit", run_circuito)
            .params("{quadro: hex, circuito?: string (default: the next free number), pontos: [hex]} → {circuito, pontos}")
            .interactive(|_| Ok(Box::new(CircuitoM::default()))),
        CommandSpec::new("elecomando", "Switches and Lamps", run_comando)
            .params("{interruptor: hex, luminarias: [hex], comando?: letter (default: the switch's, or the next free one)} → {comando, circuito, luminarias}")
            .interactive(|_| Ok(Box::new(ComandoM::default()))),
        CommandSpec::new("elefiacao", "Adjust Wiring", run_fiacao)
            .params("{quadro?: hex | TAG (default: every board), limpar?: bool (only erase the wiring marks)} → {quadros: [{quadro, tag, trechos, marcas, avisos}]}")
            .interactive(|_| Ok(Box::new(FiacaoM))),
        CommandSpec::new("eleeletroduto", "Insert Conduit", run_eletroduto)
            .params(
                "{de: hex | [x, y], para: hex | [x, y], piso?: bool, flecha?: number (arc bulge as a fraction of the chord: + left, - right, 0 straight; default 0.15)} → {eletroduto}",
            )
            .interactive(|_| Ok(Box::new(EletrodutoM::default()))),
        CommandSpec::new("eleponto", "Insert Electrical Point", run_ponto)
            .params("{bloco, pontos: [[x, y]], rotacao?: degrees, valores?: {TAG: value}} → {pontos: [hex]}")
            .interactive(|_| Ok(Box::new(PontoM::default()))),
        CommandSpec::new("eletrica.blocos", "Electrical Point Blocks", run_blocos)
            .params("{} → [{nome, tipo, inseridos, atributos: [{tag, padrao, prompt, constante}]}]")
            .noundo(),
        CommandSpec::new("eletrica.ponto.modelo", "Point Template", run_modelo)
            .params("{bloco, rotacao?: degrees, valores?: {TAG: value}} → the next ELEPONTO inserts this block with these values")
            .noundo(),
        CommandSpec::new("eletrica.ponto.excluir", "Delete Electrical Points", run_excluir_pontos).params("{pontos: [hex]} → {excluidos}"),
        CommandSpec::new("eletrica.analisar", "Electrical Analysis", run_analisar).params("{} → {pontos, circuitos}").noundo(),
        CommandSpec::new("eletrica.rotear", "Wiring Summary", run_rotear)
            .params("{quadro?: hex | TAG (default: the first board)} → the routing and sizing summary of the board's circuits")
            .noundo(),
        CommandSpec::new("eletrica.sobrescrever", "Override Circuit", run_sobrescrever)
            .params("{quadro?: hex | TAG, circuito, secao?: mm², disjuntor?: A} (no value: back to the automatic choice) → {quadro, circuito}"),
        CommandSpec::new("eletrica.quadro", "Board Settings", run_quadro)
            .params("{quadro?: hex | TAG, isolacao?: \"PVC\" | \"EPR\" | \"XLPE\" | null (null: back to the settings' default)} → {quadro, isolacao}"),
        CommandSpec::new("eletrica.cargas", "Load Schedule", run_cargas)
            .params("{quadro?: hex | TAG} → [{id, descricao, circuito}] (circuits with points and the ones created empty)")
            .noundo(),
        CommandSpec::new("eletrica.circuito.criar", "New Circuit", run_criar)
            .params("{quadro?: hex | TAG, circuito?: string (default: the next free number), descricao?} → {quadro, circuito}"),
        CommandSpec::new("eletrica.circuito.descrever", "Describe Circuit", run_descrever).params("{quadro?: hex | TAG, circuito, descricao} → {quadro, circuito}"),
        CommandSpec::new("eletrica.circuito.excluir", "Delete Circuit", run_excluir)
            .params("{quadro?: hex | TAG, circuito} (its points lose the circuit) → {quadro, circuito, pontos}"),
        CommandSpec::new("eletrica.circuito.juntar", "Merge Circuits", run_juntar)
            .params("{quadro?: hex | TAG, de, para} (the points of `de` go to `para`) → {quadro, de, para, pontos}"),
        CommandSpec::new("eletrica.circuito.definir", "Circuit Properties", run_definir).params(
            "{quadro?: hex | TAG, circuito, isolacao: \"PVC\" | \"EPR\" | \"XLPE\" | null (null: the board's)} → {quadro, circuito, isolacao}. Load type and power factor come from the block attributes TIPO_CARGA and FP.",
        ),
        CommandSpec::new("eletrica.demanda.tabelas", "Demand Tables", run_tabelas_demanda)
            .params("{} → the utilities' demand tables (id, nome, fonte, edificacoes, aparelhos, arCondicionado, bombas)")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eletrica.circuito.alvo", "Circuit Target", run_alvo)
            .params("{quadro?: hex | TAG, circuito} → the next ELECIRCUITO goes straight to picking points for this circuit")
            .noundo(),
        CommandSpec::new("eletrica.setup", "Electrical Settings", run_setup)
            .params("{} reads; {sistema?, temperaturaAmbiente?, peDireito?, quedaAdmissivel?, ...} writes → the settings")
            .enabled(always)
            .noundo(),
    ]
}

/// O quadro por handle ou TAG; sem nada, o primeiro do desenho.
fn achar_quadro(a: &ele::Analise, v: Option<&Value>) -> Result<Handle> {
    let quadros = a.quadros();
    let achado = match v.and_then(Value::as_str) {
        None => quadros.first().copied(),
        Some(t) => quadros
            .iter()
            .find(|q| Handle::parse_hex(t) == Some(q.handle))
            .or_else(|| quadros.iter().find(|q| q.tag.as_deref().is_some_and(|tag| tag.eq_ignore_ascii_case(t))))
            .copied(),
    };
    achado.map(|q| q.handle).ok_or_else(|| erro("nenhum quadro (TIPO_PONTO = QUADRO) com esse nome no desenho"))
}

fn json_de<T: serde::Serialize>(v: &T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| erro(e.to_string()))
}

fn run_analisar(s: &mut Session, _p: &Value) -> Result<Value> {
    json_de(&analise(s)?)
}

fn run_rotear(s: &mut Session, p: &Value) -> Result<Value> {
    let a = analise(s)?;
    let q = achar_quadro(&a, p.get("quadro"))?;
    json_de(&ele::rotear_quadro(s.doc()?, &a, &ele::setup::atual(), q).map_err(erro)?)
}

fn run_sobrescrever(s: &mut Session, p: &Value) -> Result<Value> {
    let q = achar_quadro(&analise(s)?, p.get("quadro"))?;
    let circuito = p
        .get("circuito")
        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())))
        .ok_or_else(|| bad("eletrica.sobrescrever", "`circuito` is required"))?;
    let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    if let Some(sec) = num("secao")
        && ele::nbr5410::indice_secao(sec).is_none()
    {
        return Err(bad("eletrica.sobrescrever", format!("section {sec} mm² is not in the table")));
    }
    ele::editar::definir_sobrescrita(s.doc_mut()?, q, &circuito, num("secao"), num("disjuntor")).map_err(erro)?;
    Ok(json!({ "quadro": q.hex(), "circuito": circuito }))
}

/// Um texto obrigatório (aceita número: `"circuito": 3`).
fn texto(cmd: &str, p: &Value, k: &str) -> Result<String> {
    p.get(k)
        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| bad(cmd, format!("`{k}` is required")))
}

fn run_quadro(s: &mut Session, p: &Value) -> Result<Value> {
    let q = achar_quadro(&analise(s)?, p.get("quadro"))?;
    let isolacao = match p.get("isolacao") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .and_then(ele::nbr5410::Isolacao::de_codigo)
                .ok_or_else(|| bad("eletrica.quadro", "`isolacao` must be \"PVC\", \"EPR\" or \"XLPE\""))?,
        ),
    };
    ele::editar::definir_isolacao(s.doc_mut()?, q, isolacao).map_err(erro)?;
    Ok(json!({ "quadro": q.hex(), "isolacao": isolacao.map(|i| i.codigo()) }))
}

fn run_cargas(s: &mut Session, p: &Value) -> Result<Value> {
    let a = analise(s)?;
    let q = achar_quadro(&a, p.get("quadro"))?;
    json_de(&ele::quadro::circuitos_do_quadro(&a, q))
}

fn run_criar(s: &mut Session, p: &Value) -> Result<Value> {
    let a = analise(s)?;
    let q = achar_quadro(&a, p.get("quadro"))?;
    let id = match p.get("circuito") {
        None | Some(Value::Null) => ele::circuito::proximo_circuito(&a.pontos, q),
        _ => texto("eletrica.circuito.criar", p, "circuito")?,
    };
    if ele::quadro::circuitos_do_quadro(&a, q).iter().any(|c| c.id.eq_ignore_ascii_case(&id)) {
        return Err(bad("eletrica.circuito.criar", format!("circuit {id} already exists on this board")));
    }
    let descricao = p.get("descricao").and_then(Value::as_str).unwrap_or("");
    ele::editar::definir_descricao(s.doc_mut()?, q, &id, Some(descricao)).map_err(erro)?;
    Ok(json!({ "quadro": q.hex(), "circuito": id }))
}

fn run_descrever(s: &mut Session, p: &Value) -> Result<Value> {
    let q = achar_quadro(&analise(s)?, p.get("quadro"))?;
    let id = texto("eletrica.circuito.descrever", p, "circuito")?;
    let descricao = p.get("descricao").and_then(Value::as_str).unwrap_or("");
    ele::editar::definir_descricao(s.doc_mut()?, q, &id, Some(descricao)).map_err(erro)?;
    Ok(json!({ "quadro": q.hex(), "circuito": id }))
}

fn run_excluir(s: &mut Session, p: &Value) -> Result<Value> {
    let q = achar_quadro(&analise(s)?, p.get("quadro"))?;
    let id = texto("eletrica.circuito.excluir", p, "circuito")?;
    let n = ele::editar::excluir_circuito(s.doc_mut()?, q, &id).map_err(erro)?;
    Ok(json!({ "quadro": q.hex(), "circuito": id, "pontos": n }))
}

fn run_juntar(s: &mut Session, p: &Value) -> Result<Value> {
    let q = achar_quadro(&analise(s)?, p.get("quadro"))?;
    let de = texto("eletrica.circuito.juntar", p, "de")?;
    let para = texto("eletrica.circuito.juntar", p, "para")?;
    let n = ele::editar::juntar_circuitos(s.doc_mut()?, q, &de, &para).map_err(erro)?;
    Ok(json!({ "quadro": q.hex(), "de": de, "para": para, "pontos": n }))
}

fn run_definir(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eletrica.circuito.definir";
    let q = achar_quadro(&analise(s)?, p.get("quadro"))?;
    let id = texto(CMD, p, "circuito")?;
    // Tipo de carga e FP são atributos dos blocos (TIPO_CARGA e FP), não do circuito.
    if p.get("carga").is_some() || p.get("fp").is_some() {
        return Err(bad(CMD, "load type and power factor are block attributes (TIPO_CARGA, FP): edit the points"));
    }
    let isolacao = match p.get("isolacao") {
        None => return Err(bad(CMD, "give `isolacao`")),
        Some(Value::Null) => None,
        Some(v) => {
            Some(v.as_str().and_then(ele::nbr5410::Isolacao::de_codigo).ok_or_else(|| bad(CMD, "`isolacao` must be \"PVC\", \"EPR\" or \"XLPE\""))?)
        }
    };
    ele::editar::definir_isolacao_circuito(s.doc_mut()?, q, &id, isolacao).map_err(erro)?;
    Ok(json!({ "quadro": q.hex(), "circuito": id, "isolacao": isolacao.map(|i| i.codigo()) }))
}

fn run_tabelas_demanda(_s: &mut Session, _p: &Value) -> Result<Value> {
    json_de(&ele::demanda::tabelas())
}

fn run_alvo(s: &mut Session, p: &Value) -> Result<Value> {
    let q = achar_quadro(&analise(s)?, p.get("quadro"))?;
    let id = texto("eletrica.circuito.alvo", p, "circuito")?;
    ele::circuito::definir_alvo(s.state()?.uid, Some((q, id.clone())));
    Ok(json!({ "quadro": q.hex(), "circuito": id }))
}

fn run_setup(_s: &mut Session, p: &Value) -> Result<Value> {
    ele::setup::json(p).map_err(|e| bad("eletrica.setup", e))
}

/// Ajusta a fiação de um quadro: apaga as marcas antigas e desenha as novas.
fn ajustar(s: &mut Session, q: Handle, limpar: bool) -> Result<Value> {
    if limpar {
        let n = ele::fiacao::limpar(s.doc_mut()?, q);
        return Ok(json!({ "quadro": q.hex(), "apagadas": n }));
    }
    let a = analise(s)?;
    let setup = ele::setup::atual();
    let f = ele::fiacao::calcular(s.doc()?, &a, &setup, q).map_err(erro)?;
    let marcas = ele::fiacao::aplicar(s.doc_mut()?, &f, &setup).map_err(erro)?;
    Ok(json!({ "quadro": q.hex(), "tag": f.tag, "trechos": f.trechos.len(), "marcas": marcas, "avisos": f.avisos }))
}

fn run_fiacao(s: &mut Session, p: &Value) -> Result<Value> {
    let a = analise(s)?;
    let alvos: Vec<Handle> = match p.get("quadro") {
        None | Some(Value::Null) => a.quadros().iter().map(|q| q.handle).collect(),
        v => vec![achar_quadro(&a, v)?],
    };
    if alvos.is_empty() {
        return Err(erro("nenhum quadro (TIPO_PONTO = QUADRO) no desenho"));
    }
    let limpar = p.get("limpar").and_then(Value::as_bool).unwrap_or(false);
    let mut quadros = Vec::new();
    for q in alvos {
        quadros.push(ajustar(s, q, limpar)?);
    }
    Ok(json!({ "quadros": quadros }))
}

/// ELEFIACAO digitado: um quadro selecionado, ou Enter para todos.
struct FiacaoM;

impl FiacaoM {
    fn executar(s: &mut Session, alvos: Vec<Handle>) -> Result<Step> {
        for q in alvos {
            match ajustar(s, q, false) {
                Ok(v) => {
                    s.echo(format!("Fiação de {}: {} marca(s) em {} trecho(s).", v["tag"].as_str().unwrap_or("?"), v["marcas"], v["trechos"]));
                    for a in v["avisos"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                        s.echo(a.to_string());
                    }
                }
                Err(e) => s.echo(e.to_string()),
            }
        }
        Ok(Step::Done)
    }
}

impl Interactive for FiacaoM {
    fn name(&self) -> &'static str {
        "ELEFIACAO"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Selecione o quadro", Accept::SELECT).default("Todos")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Cancel => Ok(Step::Done),
            Input::Enter => {
                let todos = analise(s)?.quadros().iter().map(|q| q.handle).collect();
                Self::executar(s, todos)
            }
            Input::Pick(hs) => {
                s.set_selection(Vec::new());
                match hs.into_iter().find(|h| quadro_valido(s, *h).ok().flatten().is_some()) {
                    Some(q) => Self::executar(s, vec![q]),
                    None => {
                        s.echo("Selecione um bloco de quadro (TIPO_PONTO = QUADRO).");
                        Ok(Step::Continue)
                    }
                }
            }
            _ => Ok(Step::Continue),
        }
    }
}

fn erro(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

fn analise(s: &Session) -> Result<ele::Analise> {
    Ok(ele::analisar(s.doc()?))
}

fn handle(cmd: &str, p: &Value, k: &str) -> Result<Handle> {
    p.get(k).and_then(Value::as_str).and_then(Handle::parse_hex).ok_or_else(|| bad(cmd, format!("`{k}` (handle) is required")))
}

fn handles(cmd: &str, p: &Value, k: &str) -> Result<Vec<Handle>> {
    let lista = p.get(k).and_then(Value::as_array).ok_or_else(|| bad(cmd, format!("`{k}` ([handle]) is required")))?;
    lista.iter().map(|v| v.as_str().and_then(Handle::parse_hex).ok_or_else(|| bad(cmd, format!("`{k}` holds handles, got {v}")))).collect()
}

/// Põe um ponto no circuito, avisando na linha de comando. `false`: não era um ponto válido.
fn atribuir(s: &mut Session, quadro: Handle, circuito: &str, h: Handle) -> Result<bool> {
    let Some(p) = ele::ponto::ler_ponto(s.doc()?, h) else {
        s.echo("O objeto não é um ponto elétrico (bloco com TIPO_PONTO, POTENCIA, CIRCUITO...).");
        return Ok(false);
    };
    if p.tipo == TipoPonto::Quadro {
        s.echo("Um quadro não entra num circuito.");
        return Ok(false);
    }
    ele::editar::atribuir_circuito(s.doc_mut()?, h, quadro, circuito).map_err(erro)?;
    s.echo(format!("{}: circuito {circuito}", p.rotulo()));
    Ok(true)
}

fn quadro_valido(s: &Session, h: Handle) -> Result<Option<PontoEletrico>> {
    Ok(ele::ponto::ler_ponto(s.doc()?, h).filter(|p| p.tipo == TipoPonto::Quadro))
}

fn run_circuito(s: &mut Session, p: &Value) -> Result<Value> {
    let quadro = handle("elecircuito", p, "quadro")?;
    if quadro_valido(s, quadro)?.is_none() {
        return Err(bad("elecircuito", "`quadro` is not a distribution board block (TIPO_PONTO = QUADRO)"));
    }
    let circuito = match p.get("circuito").and_then(Value::as_str).map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => c.to_string(),
        None => ele::circuito::proximo_circuito(&analise(s)?.pontos, quadro),
    };
    let mut feitos = Vec::new();
    for h in handles("elecircuito", p, "pontos")? {
        if atribuir(s, quadro, &circuito, h)? {
            feitos.push(h.hex());
        }
    }
    Ok(json!({ "circuito": circuito, "pontos": feitos }))
}

#[derive(Default)]
struct CircuitoM {
    /// O quadro escolhido e a TAG dele.
    quadro: Option<(Handle, String)>,
    circuito: Option<String>,
    /// O próximo número livre, oferecido como padrão.
    sugestao: String,
    feitos: usize,
}

impl Interactive for CircuitoM {
    fn name(&self) -> &'static str {
        "ELECIRCUITO"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        // Escolhido antes (eletrica.circuito.alvo, o "adicionar pontos" do quadro de cargas):
        // vai direto à seleção dos pontos.
        if let Some((q, c)) = ele::circuito::tomar_alvo(s.state()?.uid)
            && let Some(p) = quadro_valido(s, q)?
        {
            self.quadro = Some((q, p.rotulo()));
            self.circuito = Some(c);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (&self.quadro, &self.circuito) {
            (None, _) => Prompt::new("Selecione o quadro de distribuição", Accept::SELECT),
            (Some(_), None) => Prompt::new("Número do circuito", Accept::TEXT).default(self.sugestao.clone()),
            (Some((_, tag)), Some(c)) => Prompt::new(format!("Selecione os pontos do circuito {c} ({tag})"), Accept::SELECT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.quadro, &self.circuito, i) {
            (_, _, Input::Cancel) => Ok(Step::Done),
            (None, _, Input::Pick(hs)) => {
                s.set_selection(Vec::new());
                for h in hs {
                    if let Some(q) = quadro_valido(s, h)? {
                        self.sugestao = ele::circuito::proximo_circuito(&analise(s)?.pontos, h);
                        s.echo(format!("Quadro {}.", q.rotulo()));
                        self.quadro = Some((h, q.rotulo()));
                        return Ok(Step::Continue);
                    }
                }
                s.echo("Selecione um bloco de quadro (TIPO_PONTO = QUADRO).");
                Ok(Step::Continue)
            }
            (None, _, Input::Enter) => Ok(Step::Done),
            (Some(_), None, Input::Text(t) | Input::Keyword(t)) if !t.trim().is_empty() => {
                self.circuito = Some(t.trim().to_string());
                Ok(Step::Continue)
            }
            (Some(_), None, Input::Enter | Input::Text(_)) => {
                self.circuito = Some(self.sugestao.clone());
                Ok(Step::Continue)
            }
            (Some((q, _)), Some(c), Input::Pick(hs)) => {
                let (q, c) = (*q, c.clone());
                s.set_selection(Vec::new());
                for h in hs {
                    if atribuir(s, q, &c, h)? {
                        self.feitos += 1;
                    }
                }
                Ok(Step::Continue)
            }
            (Some(_), Some(c), Input::Enter) => {
                s.echo(format!("{} ponto(s) no circuito {c}.", self.feitos));
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}

/// O interruptor escolhido em ELECOMANDO.
struct Interruptor {
    h: Handle,
    rotulo: String,
    letra: String,
    circuito: Option<String>,
    quadro: Option<Handle>,
}

/// Lê o interruptor e garante a letra dele (a que tem, ou a próxima livre no circuito).
fn preparar_interruptor(s: &mut Session, h: Handle, letra: Option<&str>) -> Result<Option<Interruptor>> {
    let a = analise(s)?;
    let Some(p) = a.ponto(h).filter(|p| p.tipo == TipoPonto::Interruptor) else { return Ok(None) };
    let letra = match (letra, &p.comando) {
        (Some(l), _) => l.trim().to_string(),
        (None, Some(l)) => l.clone(),
        (None, None) => ele::circuito::proxima_letra(&a.pontos, p.quadro, p.circuito.as_deref()),
    };
    if p.comando.as_deref() != Some(letra.as_str()) {
        ele::editar::definir_comando(s.doc_mut()?, h, &letra).map_err(erro)?;
    }
    Ok(Some(Interruptor { h, rotulo: p.rotulo(), letra, circuito: p.circuito.clone(), quadro: p.quadro }))
}

/// Liga uma luminária ao interruptor: mesma letra e mesmo circuito. Se o interruptor ainda não
/// tem circuito, ele fica com o da luminária.
fn ligar_luminaria(s: &mut Session, sw: &mut Interruptor, h: Handle) -> Result<bool> {
    let a = analise(s)?;
    let Some(lum) = a.ponto(h).filter(|p| p.tipo == TipoPonto::Iluminacao).cloned() else {
        s.echo("O objeto não é uma luminária (TIPO_PONTO = ILUM).");
        return Ok(false);
    };
    ele::editar::definir_comando(s.doc_mut()?, h, &sw.letra).map_err(erro)?;
    match (&sw.circuito, sw.quadro, &lum.circuito, lum.quadro) {
        (Some(c), Some(q), lc, lq) => {
            if lc.as_deref() != Some(c.as_str()) || lq != Some(q) {
                ele::editar::atribuir_circuito(s.doc_mut()?, h, q, c).map_err(erro)?;
            }
        }
        (None, _, Some(c), Some(q)) => {
            ele::editar::atribuir_circuito(s.doc_mut()?, sw.h, q, c).map_err(erro)?;
            sw.circuito = Some(c.clone());
            sw.quadro = Some(q);
            s.echo(format!("{} entrou no circuito {c}.", sw.rotulo));
        }
        _ => s.echo("Interruptor e luminária ainda sem circuito: use Criar circuito."),
    }
    s.echo(format!("{}: comando {}", lum.rotulo(), sw.letra));
    Ok(true)
}

fn run_comando(s: &mut Session, p: &Value) -> Result<Value> {
    let h = handle("elecomando", p, "interruptor")?;
    let letra = p.get("comando").and_then(Value::as_str).filter(|l| !l.trim().is_empty());
    let mut sw =
        preparar_interruptor(s, h, letra)?.ok_or_else(|| bad("elecomando", "`interruptor` is not a switch block (TIPO_PONTO = INTERRUPTOR)"))?;
    let mut feitos = Vec::new();
    for l in handles("elecomando", p, "luminarias")? {
        if ligar_luminaria(s, &mut sw, l)? {
            feitos.push(l.hex());
        }
    }
    Ok(json!({ "comando": sw.letra, "circuito": sw.circuito, "luminarias": feitos }))
}

#[derive(Default)]
struct ComandoM {
    interruptor: Option<Interruptor>,
    feitos: usize,
}

impl Interactive for ComandoM {
    fn name(&self) -> &'static str {
        "ELECOMANDO"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.interruptor {
            None => Prompt::new("Selecione o interruptor", Accept::SELECT),
            Some(sw) => Prompt::new(format!("Selecione as luminárias do comando {}", sw.letra), Accept::SELECT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Cancel => Ok(Step::Done),
            Input::Pick(hs) => {
                s.set_selection(Vec::new());
                match &mut self.interruptor {
                    None => {
                        for h in hs {
                            if let Some(sw) = preparar_interruptor(s, h, None)? {
                                s.echo(format!("Interruptor {}: comando {}.", sw.rotulo, sw.letra));
                                self.interruptor = Some(sw);
                                return Ok(Step::Continue);
                            }
                        }
                        s.echo("Selecione um interruptor (TIPO_PONTO = INTERRUPTOR).");
                    }
                    Some(sw) => {
                        for h in hs {
                            if ligar_luminaria(s, sw, h)? {
                                self.feitos += 1;
                            }
                        }
                    }
                }
                Ok(Step::Continue)
            }
            Input::Enter => {
                if let Some(sw) = &self.interruptor {
                    s.echo(format!("{} luminária(s) no comando {}.", self.feitos, sw.letra));
                }
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------------------------------------------------------- eletroduto em arco

/// Uma ponta do eletroduto no JSON: o handle de um bloco (ou círculo) ou `[x, y]`.
fn ponta(s: &Session, cmd: &str, p: &Value, k: &str) -> Result<[f64; 2]> {
    match p.get(k) {
        Some(Value::String(t)) => {
            let h = Handle::parse_hex(t).ok_or_else(|| bad(cmd, format!("`{k}` is not a handle")))?;
            ele::eletroduto::posicao(s.doc()?, h).ok_or_else(|| bad(cmd, format!("`{k}` must be a block or a circle")))
        }
        Some(Value::Array(a)) => match (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64)) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok([x, y]),
            _ => Err(bad(cmd, format!("`{k}` must be [x, y]"))),
        },
        _ => Err(bad(cmd, format!("`{k}` (handle or [x, y]) is required"))),
    }
}

fn run_eletroduto(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eleeletroduto";
    let (a, b) = (ponta(s, CMD, p, "de")?, ponta(s, CMD, p, "para")?);
    let piso = p.get("piso").and_then(Value::as_bool).unwrap_or(false);
    let flecha = match p.get("flecha") {
        None | Some(Value::Null) => ele::eletroduto::FLECHA_PADRAO,
        Some(v) => v.as_f64().filter(|f| f.is_finite()).ok_or_else(|| bad(CMD, "`flecha` must be a number"))?,
    };
    let h = ele::eletroduto::inserir(s.doc_mut()?, a, b, piso, flecha).map_err(erro)?;
    Ok(json!({ "eletroduto": h.hex() }))
}

/// ELEELETRODUTO digitado: o primeiro objeto, depois os seguintes; cada um é ligado ao anterior
/// por um eletroduto em arco, até Enter.
struct EletrodutoM {
    anterior: Option<[f64; 2]>,
    piso: bool,
    /// Lado do arco: 1 à esquerda de quem vai do anterior ao próximo, -1 à direita.
    lado: f64,
    flecha: f64,
    pedindo_flecha: bool,
    feitos: usize,
}

impl Default for EletrodutoM {
    fn default() -> Self {
        EletrodutoM { anterior: None, piso: false, lado: 1.0, flecha: ele::eletroduto::FLECHA_PADRAO, pedindo_flecha: false, feitos: 0 }
    }
}

impl EletrodutoM {
    fn chaves(&self) -> Vec<&'static str> {
        let nivel = if self.piso { "Teto" } else { "Piso" };
        if self.anterior.is_some() { vec![nivel, "Inverter", "Flecha"] } else { vec![nivel, "Flecha"] }
    }
}

impl Interactive for EletrodutoM {
    fn name(&self) -> &'static str {
        "ELEELETRODUTO"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.pedindo_flecha {
            return Prompt::new("Flecha do arco (fração da corda, de 0 a 0,5)", Accept::TEXT).default(ele::ponto::br(self.flecha));
        }
        let onde = if self.piso { "piso" } else { "teto" };
        let msg = match self.anterior {
            None => format!("Selecione o primeiro objeto (eletroduto no {onde})"),
            Some(_) => format!("Selecione o próximo objeto (eletroduto no {onde}; Enter termina)"),
        };
        Prompt::new(msg, Accept::SELECT).kw(&self.chaves())
    }
    fn preview(&self, _s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        let Some(a) = self.anterior else { return Vec::new() };
        let pts: Vec<Vec2> =
            ele::eletroduto::pontos_do_arco(a, [cursor.x, cursor.y], self.lado * self.flecha, 9).into_iter().map(|p| Vec2::new(p[0], p[1])).collect();
        if (a[0] - cursor.x).hypot(a[1] - cursor.y) <= 0.0 {
            return Vec::new();
        }
        vec![EntityKind::Spline(cadcraft_doc::geom::Spline::from_fit_points(&pts))]
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.pedindo_flecha {
            self.pedindo_flecha = false;
            if let Input::Text(t) | Input::Keyword(t) = &i
                && let Some(f) = ele::ponto::numero(t)
            {
                self.flecha = f.clamp(0.0, ele::eletroduto::FLECHA_MAXIMA);
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Cancel | Input::Enter => {
                if self.feitos > 0 {
                    s.echo(format!("{} eletroduto(s) inserido(s).", self.feitos));
                }
                Ok(Step::Done)
            }
            Input::Keyword(k) | Input::Text(k) => {
                match k.to_ascii_lowercase().as_str() {
                    "piso" => self.piso = true,
                    "teto" => self.piso = false,
                    "inverter" => self.lado = -self.lado,
                    "flecha" => self.pedindo_flecha = true,
                    _ => s.echo("Opção inválida."),
                }
                Ok(Step::Continue)
            }
            Input::Pick(hs) => {
                s.set_selection(Vec::new());
                let Some(p) = hs.iter().find_map(|h| ele::eletroduto::posicao(s.doc().ok()?, *h)) else {
                    s.echo("Selecione um bloco (ponto, quadro, caixa) ou um círculo.");
                    return Ok(Step::Continue);
                };
                if let Some(a) = self.anterior {
                    match ele::eletroduto::inserir(s.doc_mut()?, a, p, self.piso, self.lado * self.flecha) {
                        Ok(_) => self.feitos += 1,
                        Err(e) => {
                            s.echo(e);
                            return Ok(Step::Continue);
                        }
                    }
                }
                self.anterior = Some(p);
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------------------------------------------------------- inserir pontos

fn valores_de(cmd: &str, p: &Value) -> Result<Vec<(String, String)>> {
    match p.get("valores") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, v)| match v {
                Value::String(t) => Ok((k.clone(), t.clone())),
                Value::Number(n) => Ok((k.clone(), n.to_string())),
                _ => Err(bad(cmd, format!("value of `{k}` must be text or a number"))),
            })
            .collect(),
        Some(_) => Err(bad(cmd, "`valores` must be an object {TAG: value}")),
    }
}

fn rotacao_de(cmd: &str, p: &Value) -> Result<f64> {
    match p.get("rotacao") {
        None | Some(Value::Null) => Ok(0.0),
        Some(v) => v.as_f64().filter(|r| r.is_finite()).ok_or_else(|| bad(cmd, "`rotacao` must be a number (degrees)")),
    }
}

fn run_ponto(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eleponto";
    let bloco = texto(CMD, p, "bloco")?;
    let (rotacao, valores) = (rotacao_de(CMD, p)?, valores_de(CMD, p)?);
    let lista = p.get("pontos").and_then(Value::as_array).ok_or_else(|| bad(CMD, "`pontos` ([[x, y]]) is required"))?;
    let mut posicoes = Vec::new();
    for v in lista {
        match (v.get(0).and_then(Value::as_f64), v.get(1).and_then(Value::as_f64)) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => posicoes.push([x, y]),
            _ => return Err(bad(CMD, format!("`pontos` holds [x, y], got {v}"))),
        }
    }
    let mut feitos = Vec::new();
    for pos in posicoes {
        feitos.push(ele::biblioteca::inserir(s.doc_mut()?, &bloco, pos, rotacao, &valores).map_err(erro)?.hex());
    }
    Ok(json!({ "pontos": feitos }))
}

fn run_blocos(s: &mut Session, _p: &Value) -> Result<Value> {
    json_de(&ele::biblioteca::blocos(s.doc()?))
}

fn run_modelo(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eletrica.ponto.modelo";
    let bloco = texto(CMD, p, "bloco")?;
    let blocos = ele::biblioteca::blocos(s.doc()?);
    let b =
        blocos.iter().find(|b| b.nome.eq_ignore_ascii_case(&bloco)).ok_or_else(|| bad(CMD, format!("the drawing has no point block `{bloco}`")))?;
    let modelo = ele::biblioteca::Modelo { bloco: b.nome.clone(), rotacao: rotacao_de(CMD, p)?, valores: valores_de(CMD, p)? };
    ele::biblioteca::definir_modelo(s.state()?.uid, Some(modelo.clone()));
    json_de(&modelo)
}

fn run_excluir_pontos(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = handles("eletrica.ponto.excluir", p, "pontos")?;
    let n = ele::biblioteca::excluir(s.doc_mut()?, &hs).map_err(erro)?;
    Ok(json!({ "excluidos": n }))
}

/// ELEPONTO digitado: o bloco (o escolhido na janela Inserir pontos, ou digitado), depois os
/// pontos de inserção, um por clique, até Enter.
#[derive(Default)]
struct PontoM {
    modelo: Option<ele::biblioteca::Modelo>,
    pedindo_rotacao: bool,
    feitos: usize,
}

impl Interactive for PontoM {
    fn name(&self) -> &'static str {
        "ELEPONTO"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let blocos = ele::biblioteca::blocos(s.doc()?);
        if blocos.is_empty() {
            s.echo("O desenho não tem blocos de ponto elétrico: insira o template FREE_ELETRICA_BLOCOS.dxf.");
            return Ok(Step::Done);
        }
        self.modelo = ele::biblioteca::modelo(s.state()?.uid).filter(|m| blocos.iter().any(|b| b.nome.eq_ignore_ascii_case(&m.bloco)));
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.modelo {
            None => {
                let primeiro = s.doc().ok().and_then(|d| ele::biblioteca::blocos(d).into_iter().next()).map(|b| b.nome).unwrap_or_default();
                Prompt::new("Nome do bloco do ponto", Accept::TEXT).default(primeiro)
            }
            Some(_) if self.pedindo_rotacao => Prompt::new("Rotação (graus)", Accept::TEXT),
            Some(m) => Prompt::new(format!("Ponto de inserção de {} (Enter termina)", m.bloco), Accept::POINT).kw(&["Rotacao"]),
        }
    }
    fn preview(&self, _s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        let Some(m) = &self.modelo else { return Vec::new() };
        if self.pedindo_rotacao {
            return Vec::new();
        }
        vec![EntityKind::Insert(cadcraft_doc::Insert {
            block: m.bloco.clone(),
            insert: cadcraft_doc::geom::Vec3::new(cursor.x, cursor.y, 0.0),
            scale: cadcraft_doc::geom::Vec3::new(1.0, 1.0, 1.0),
            rotation: m.rotacao.to_radians(),
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        })]
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Done);
        }
        let Some(m) = self.modelo.clone() else {
            let nome = match &i {
                Input::Text(t) | Input::Keyword(t) if !t.trim().is_empty() => t.trim().to_string(),
                Input::Enter | Input::Text(_) => ele::biblioteca::blocos(s.doc()?).into_iter().next().map(|b| b.nome).unwrap_or_default(),
                _ => return Ok(Step::Continue),
            };
            match ele::biblioteca::blocos(s.doc()?).into_iter().find(|b| b.nome.eq_ignore_ascii_case(&nome)) {
                Some(b) => self.modelo = Some(ele::biblioteca::Modelo { bloco: b.nome, ..Default::default() }),
                None => s.echo(format!("O desenho não tem o bloco de ponto \"{nome}\".")),
            }
            return Ok(Step::Continue);
        };
        if self.pedindo_rotacao {
            self.pedindo_rotacao = false;
            if let Input::Text(t) | Input::Keyword(t) = &i
                && let Ok(r) = t.trim().replace(',', ".").parse::<f64>()
                && r.is_finite()
                && let Some(md) = &mut self.modelo
            {
                md.rotacao = r;
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) => {
                match ele::biblioteca::inserir(s.doc_mut()?, &m.bloco, [p.x, p.y], m.rotacao, &m.valores) {
                    Ok(_) => self.feitos += 1,
                    Err(e) => s.echo(e),
                }
                Ok(Step::Continue)
            }
            Input::Keyword(k) | Input::Text(k) if k.trim().to_ascii_lowercase().starts_with('r') => {
                self.pedindo_rotacao = true;
                Ok(Step::Continue)
            }
            Input::Enter => {
                if self.feitos > 0 {
                    s.echo(format!("{} ponto(s) {} inserido(s).", self.feitos, m.bloco));
                }
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}
