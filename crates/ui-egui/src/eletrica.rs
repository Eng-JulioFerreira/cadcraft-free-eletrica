// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Free Elétrica: a paleta Elétrica e as suas janelas.
//!
//! A paleta só tem ferramentas. Criar circuito, Comandos e luminárias e Ajustar fiação são
//! comandos do motor (ELECIRCUITO, ELECOMANDO, ELEFIACAO) que pedem as seleções no desenho. As
//! janelas mostram os dados do projeto: Configurações (o setup), Quadro de cargas (o menu central
//! dos circuitos de um quadro: criar, descrever, pontos, tipo de carga, FP, seção, disjuntor,
//! isolação, a fiação roteada e a demanda pela concessionária) e Conferência (pontos e
//! circuitos). Toda alteração passa por um comando do motor (`app.run`/`app.start`): a
//! interface não decide nada de engenharia.
//!
//! Os textos ficam em português: o Free Elétrica é um addon para projetos brasileiros e não passa
//! pelo catálogo de traduções do CADCraft. Só o nome do painel, o item de menu e os nomes dos
//! comandos passam.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cadcraft_doc::Handle;
use craft_ui::docking::{Action, Node};
use craft_ui::layout::SplitSize;
use egui::{Align, Color32, Layout, RichText, vec2};
use free_eletrica::nbr5410::{DISJUNTORES, Isolacao, SECOES};
use free_eletrica::ponto::br;
use free_eletrica::setup::{DiametroCondutor, TamanhoEletroduto, diametros_padrao, eletrodutos_padrao};
use free_eletrica::{self as ele, CaboTelecom, CategoriaTelecom, CorCabo, PontoEletrico, Setup, Sistema, TipoPonto, Unidade};
use serde_json::{Value, json};

use crate::CadApp;
use crate::docking::Panel;
use crate::theme::Tokens;

const COR_AVISO: Color32 = Color32::from_rgb(232, 176, 72);
/// Altura das linhas das tabelas.
const LINHA: f32 = 20.0;

/// Chave das configurações do Free Elétrica no armazenamento de preferências do CADCraft.
pub const PREFS_KEY: &str = "freeeletrica.setup";
/// A chave de quando o projeto se chamava CRAFT ELE: lida só quando a nova não existe.
pub const PREFS_KEY_ANTIGA: &str = "craftele.setup";

static JANELAS: Mutex<Janelas> = Mutex::new(Janelas { setup: None, conferencia: false, cargas: None, pontos: None });
static ABRIR_AO_INICIAR: AtomicBool = AtomicBool::new(false);

fn trava<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// As janelas abertas.
struct Janelas {
    /// Configurações: o rascunho até o OK.
    setup: Option<Rascunho>,
    conferencia: bool,
    /// Quadro de cargas (com o roteamento da fiação).
    cargas: Option<Cargas>,
    /// Inserir pontos.
    pontos: Option<Pontos>,
}

/// A janela Inserir pontos: o filtro por tipo, o bloco escolhido e os valores dos atributos.
#[derive(Clone, Default)]
struct Pontos {
    /// `None`: todos os tipos.
    filtro: Option<TipoPonto>,
    busca: String,
    bloco: Option<String>,
    /// Valores para a inserção: (tag, valor), os padrões do bloco ao escolhê-lo.
    valores: Vec<(String, String)>,
    rotacao: f64,
    /// Ponto com a exclusão pedida, ou `Some(None)` para todos os filtrados (espera a confirmação).
    excluir: Option<Option<Handle>>,
}

#[derive(Clone)]
struct Rascunho {
    setup: Setup,
    aba: Aba,
    novo_cabo: CaboTelecom,
    erro: Option<String>,
}

#[derive(Clone, Default)]
struct Cargas {
    quadro: Option<Handle>,
    /// Descrições em edição: (circuito, texto).
    descricoes: Vec<(String, String)>,
    /// Circuito com a exclusão pedida (espera a confirmação).
    excluir: Option<String>,
    nova_descricao: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Aba {
    Geral,
    Condutores,
    Eletrodutos,
    Agrupamento,
    Demanda,
    Telecom,
}

impl Aba {
    const TODAS: [Aba; 6] = [Aba::Geral, Aba::Condutores, Aba::Eletrodutos, Aba::Agrupamento, Aba::Demanda, Aba::Telecom];
    fn nome(self) -> &'static str {
        match self {
            Aba::Geral => "Geral",
            Aba::Condutores => "Condutores",
            Aba::Eletrodutos => "Eletrodutos",
            Aba::Agrupamento => "Agrupamento",
            Aba::Demanda => "Demanda",
            Aba::Telecom => "Telecom",
        }
    }
}

/// O que as janelas pedem para o motor fazer, executado depois de desenhá-las.
enum Acao {
    /// `app.run(comando, parâmetros)`.
    Rodar(&'static str, Value),
    /// `app.start(comando)`: começa um comando interativo.
    Iniciar(&'static str),
    /// Seleciona objetos no desenho.
    Selecionar(Vec<Handle>),
}

/// As configurações em uso (guardadas no motor: `free_eletrica::setup`).
pub fn setup() -> Setup {
    ele::setup::atual()
}

fn definir_setup(s: Setup) {
    ele::setup::definir(s);
}

/// As configurações como JSON, para o armazenamento do CADCraft ([`PREFS_KEY`]).
pub fn prefs_json() -> String {
    serde_json::to_string(&setup()).unwrap_or_default()
}

pub fn load_prefs(json: &str) {
    if let Ok(s) = serde_json::from_str::<Setup>(json) {
        definir_setup(s);
    }
}

/// Pede para a paleta abrir no primeiro quadro, se o usuário nunca a fechou. Só o executável
/// chama: o layout de fábrica (e os testes dele) continuam os do CADCraft.
pub fn abrir_ao_iniciar() {
    ABRIR_AO_INICIAR.store(true, Ordering::Relaxed);
}

/// Abre a paleta (comando ELETRICA, menu Janela). Na primeira vez ela fica acima das Tool Sets.
pub fn open(app: &mut CadApp) -> Result<Value, String> {
    let panel = Panel::Eletrica;
    let primeira = !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel);
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))?;
    if primeira && app.ui.docking.layout.contains(&Panel::ToolSets) {
        let mover = json!({ "operation": "move", "panel": panel.id(), "target": Panel::ToolSets.id(), "zone": "top" });
        if let Ok(r) = crate::docking::command(app, false, &mover) {
            // A paleta só tem botões: altura fixa, e o resto da coluna fica com as Tool Sets.
            let mut caminho = Vec::new();
            if let Some(raiz) = &app.ui.docking.layout.root
                && caminho_da_paleta(raiz, &mut caminho)
            {
                let acao = Action::<Panel>::ResizeSplit { path: caminho, size: SplitSize::FixedFirst(380.0) };
                if let Ok(acao) = serde_json::to_value(acao) {
                    let _ = crate::docking::command(app, false, &json!({ "action": acao }));
                }
            }
            return Ok(r);
        }
    }
    Ok(r)
}

/// O caminho (`true` = segundo filho) até a divisão cuja primeira parte é a paleta Elétrica.
fn caminho_da_paleta(no: &Node<Panel>, caminho: &mut Vec<bool>) -> bool {
    let Node::Split { first, second, .. } = no else { return false };
    if matches!(first.as_ref(), Node::Tabs { panels, .. } if panels.contains(&Panel::Eletrica)) {
        return true;
    }
    for (lado, filho) in [(false, first), (true, second)] {
        caminho.push(lado);
        if caminho_da_paleta(filho, caminho) {
            return true;
        }
        caminho.pop();
    }
    false
}

fn abrir_setup() -> Result<Value, String> {
    let rascunho = Rascunho {
        setup: setup(),
        aba: Aba::Geral,
        novo_cabo: CaboTelecom { categoria: CategoriaTelecom::Utp, quantidade: 4, ..CaboTelecom::default() },
        erro: None,
    };
    trava(&JANELAS).setup = Some(rascunho);
    Ok(Value::Null)
}

/// Comandos do Free Elétrica que só existem na interface: abrir a paleta e as janelas. `None`
/// quando `id` não é um deles.
///
/// Digitados: ELETRICA (paleta), ELECONFIG, ELECARGAS, ELECONFERIR e ELEROTEAR (janelas). Todo o
/// resto é comando do motor (`crates/engine/src/cmd/eletrica.rs`).
pub fn route(app: &mut CadApp, id: &str, _params: &Value) -> Option<Result<Value, String>> {
    match id {
        "ui.panel.eletrica" | "eletrica" | "freeeletrica" => Some(open(app)),
        "ui.eletrica.setup" | "eleconfig" => Some(abrir_setup()),
        "ui.eletrica.conferir" | "eleconferir" => {
            trava(&JANELAS).conferencia = true;
            Some(Ok(Value::Null))
        }
        // Rotear fiação vive dentro do Quadro de cargas; ELEROTEAR abre o quadro de cargas.
        "ui.eletrica.pontos" | "elepontos" => {
            let mut j = trava(&JANELAS);
            if j.pontos.is_none() {
                j.pontos = Some(Pontos::default());
            }
            Some(Ok(Value::Null))
        }
        "ui.eletrica.cargas" | "elecargas" | "ui.eletrica.rotear" | "elerotear" => {
            let mut j = trava(&JANELAS);
            if j.cargas.is_none() {
                j.cargas = Some(Cargas::default());
            }
            Some(Ok(Value::Null))
        }
        _ => None,
    }
}

fn pequeno(texto: impl Into<String>, cor: Color32) -> RichText {
    RichText::new(texto).font(crate::theme::small()).color(cor)
}

/// Número com casas fixas e vírgula decimal.
fn fixo(v: f64, casas: usize) -> String {
    format!("{v:.casas$}").replace('.', ",")
}

/// Campo numérico com vírgula decimal ("2,80 m"); aceita digitar com vírgula ou ponto.
fn campo<'a>(valor: &'a mut f64, faixa: std::ops::RangeInclusive<f64>, passo: f64, casas: usize, sufixo: &'static str) -> egui::DragValue<'a> {
    egui::DragValue::new(valor)
        .range(faixa)
        .speed(passo)
        .custom_formatter(move |n, _| format!("{}{sufixo}", format!("{n:.casas$}").replace('.', ",")))
        .custom_parser(ele::ponto::numero)
}

/// Uma célula de tabela com largura fixa. Números à direita, texto à esquerda.
fn celula(ui: &mut egui::Ui, largura: f32, texto: impl Into<RichText>, direita: bool) -> egui::Response {
    // Pintada num retângulo de largura exata: a coluna nunca cresce nem invade a vizinha.
    let (rect, resposta) = ui.allocate_exact_size(vec2(largura, LINHA), egui::Sense::hover());
    let galley = egui::WidgetText::from(texto.into()).into_galley(ui, Some(egui::TextWrapMode::Truncate), largura, egui::TextStyle::Body);
    let meio = galley.size().y / 2.0;
    let pos = if direita { rect.right_center() - vec2(galley.size().x, meio) } else { rect.left_center() - vec2(0.0, meio) };
    let texto = galley.text().to_string();
    resposta.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &texto));
    ui.painter().galley(pos, galley, ui.visuals().text_color());
    resposta
}

/// Cabeçalho de coluna (texto pequeno, apagado).
fn cabecalho(ui: &mut egui::Ui, t: &Tokens, colunas: &[(&str, f32, bool)]) {
    for (nome, largura, direita) in colunas {
        celula(ui, *largura, pequeno(*nome, t.text_dim).strong(), *direita);
    }
    ui.end_row();
}

fn numero(texto: impl Into<String>) -> RichText {
    RichText::new(texto).monospace()
}

/// A isolação gravada no quadro (sem ela, vale a das configurações).
fn isolacao_gravada(q: &PontoEletrico) -> Option<Isolacao> {
    q.extras.iter().find(|(k, _)| k.eq_ignore_ascii_case(ele::quadro::CHAVE_ISOLACAO)).and_then(|(_, v)| Isolacao::de_codigo(v))
}

/// Escolha do quadro e da isolação dele (topo das janelas Quadro de cargas e Rotear fiação).
/// Devolve se o quadro mudou.
fn seletor_quadro(ui: &mut egui::Ui, id: &str, analise: &ele::Analise, s: &Setup, quadro: &mut Option<Handle>, acoes: &mut Vec<Acao>) -> bool {
    let quadros = analise.quadros();
    let mut mudou = false;
    if quadro.is_none_or(|h| !quadros.iter().any(|q| q.handle == h)) {
        *quadro = quadros.first().map(|q| q.handle);
        mudou = true;
    }
    ui.label("Quadro");
    let atual = quadros.iter().find(|q| Some(q.handle) == *quadro).map_or(String::new(), |q| q.rotulo());
    egui::ComboBox::from_id_salt((id, "quadro")).selected_text(atual).width(140.0).show_ui(ui, |ui| {
        for q in &quadros {
            if ui.selectable_label(*quadro == Some(q.handle), q.rotulo()).clicked() {
                *quadro = Some(q.handle);
                mudou = true;
            }
        }
    });
    if let Some(q) = quadro.and_then(|h| analise.ponto(h)) {
        ui.add_space(8.0);
        ui.label("Isolação");
        let gravada = isolacao_gravada(q);
        let texto = |i: Option<Isolacao>| match i {
            Some(i) => i.nome().to_string(),
            None => format!("Padrão do projeto ({})", s.isolacao.codigo()),
        };
        egui::ComboBox::from_id_salt((id, "isolacao")).selected_text(texto(gravada)).width(170.0).show_ui(ui, |ui| {
            for opcao in std::iter::once(None).chain(Isolacao::TODAS.into_iter().map(Some)) {
                if ui.selectable_label(gravada == opcao, texto(opcao)).clicked() && gravada != opcao {
                    acoes.push(Acao::Rodar("eletrica.quadro", json!({ "quadro": q.handle.hex(), "isolacao": opcao.map(|i| i.codigo()) })));
                }
            }
        });
    }
    mudou
}

// ---------------------------------------------------------------- paleta (só ferramentas)

pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let tem_desenho = app.session.state().is_ok();
    let mut comando: Option<&str> = None;
    egui::ScrollArea::vertical().id_salt("dock-eletrica").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            grupo(ui, &t, "Projeto");
            if ferramenta(ui, "Configurações...", true, "Setup do projeto: sistema, temperatura, isolação, cabos, eletrodutos, telecom (ELECONFIG)")
                .clicked()
            {
                comando = Some("ui.eletrica.setup");
            }
            grupo(ui, &t, "Pontos e eletrodutos");
            if ferramenta(ui, "Inserir pontos...", tem_desenho, "Blocos de ponto do desenho por tipo: inserir com os valores escolhidos, filtrar e excluir pontos (ELEPONTOS)")
                .clicked()
            {
                comando = Some("ui.eletrica.pontos");
            }
            if ferramenta(
                ui,
                "Inserir eletroduto",
                tem_desenho,
                "Selecione os objetos em sequência: cada par é ligado por um eletroduto em arco (Piso/Teto, Inverter, Flecha) (ELEELETRODUTO)",
            )
            .clicked()
            {
                comando = Some("eleeletroduto");
            }
            grupo(ui, &t, "Circuitos");
            if ferramenta(
                ui,
                "Quadro de cargas...",
                tem_desenho,
                "Menu central dos circuitos de um quadro: criar, editar, pontos, tipo de cabo, seção, disjuntor, fiação roteada e demanda (ELECARGAS)",
            )
            .clicked()
            {
                comando = Some("ui.eletrica.cargas");
            }
            if ferramenta(ui, "Criar circuito", tem_desenho, "Selecione o quadro e depois os pontos do circuito, um a um (ELECIRCUITO)").clicked() {
                comando = Some("elecircuito");
            }
            if ferramenta(ui, "Comandos e luminárias", tem_desenho, "Selecione o interruptor e depois as luminárias que ele comanda (ELECOMANDO)")
                .clicked()
            {
                comando = Some("elecomando");
            }
            if ferramenta(ui, "Conferir pontos e circuitos...", tem_desenho, "Pontos elétricos, circuitos e atributos que faltam (ELECONFERIR)")
                .clicked()
            {
                comando = Some("ui.eletrica.conferir");
            }
            grupo(ui, &t, "Fiação");
            if ferramenta(ui, "Ajustar fiação", tem_desenho, "Desenha a simbologia de fiação e o diâmetro dos eletrodutos (ELEFIACAO)").clicked()
            {
                comando = Some("elefiacao");
            }
        });
    });
    if let Some(c) = comando {
        if c.starts_with("ui.") {
            let _ = app.run(c, Value::Null);
        } else {
            app.start(c);
        }
    }
}

fn grupo(ui: &mut egui::Ui, t: &Tokens, nome: &str) {
    ui.add_space(4.0);
    ui.label(pequeno(nome, t.text_dim).strong());
}

fn ferramenta(ui: &mut egui::Ui, texto: &str, ativa: bool, dica: &str) -> egui::Response {
    let largura = ui.available_width();
    ui.add_enabled(ativa, egui::Button::new(texto).min_size(vec2(largura, 26.0))).on_hover_text(dica).on_disabled_hover_text(dica)
}

// ---------------------------------------------------------------- janelas

type ChaveResumo = ((u64, u64), Setup, Handle);

/// O que já foi lido do desenho aberto, refeito a cada edição.
#[derive(Clone, Default)]
struct Estado {
    /// (uid, revisão) do desenho analisado.
    chave: Option<(u64, u64)>,
    analise: Arc<ele::Analise>,
    /// Os blocos de ponto do desenho.
    blocos: Arc<Vec<ele::biblioteca::BlocoPonto>>,
    resumo: Option<(ChaveResumo, Arc<Result<ele::ResumoQuadro, String>>)>,
}

impl Estado {
    fn atualizar(&mut self, app: &CadApp) {
        let chave = app.session.state().ok().map(|st| (st.uid, st.revision));
        if chave == self.chave {
            return;
        }
        self.chave = chave;
        self.analise = Arc::new(app.session.state().map(|st| ele::analisar(&st.doc)).unwrap_or_default());
        self.blocos = Arc::new(app.session.state().map(|st| ele::biblioteca::blocos(&st.doc)).unwrap_or_default());
    }

    fn resumo(&mut self, app: &CadApp, s: &Setup, quadro: Handle) -> Option<Arc<Result<ele::ResumoQuadro, String>>> {
        let chave = (self.chave?, s.clone(), quadro);
        if let Some((k, r)) = &self.resumo
            && *k == chave
        {
            return Some(r.clone());
        }
        let st = app.session.state().ok()?;
        let r = Arc::new(ele::rotear_quadro(&st.doc, &self.analise, s, quadro));
        self.resumo = Some((chave, r.clone()));
        Some(r)
    }
}

fn id_estado() -> egui::Id {
    egui::Id::new("free-eletrica-estado")
}

/// Desenha as janelas do Free Elétrica (chamado a cada quadro, com ou sem a paleta visível).
pub fn janelas(app: &mut CadApp, ctx: &egui::Context) {
    if ABRIR_AO_INICIAR.swap(false, Ordering::Relaxed) {
        let panel = Panel::Eletrica;
        if !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel) {
            let _ = open(app);
        }
    }
    let (setup_aberta, mut conferencia, cargas, pontos) = {
        let j = trava(&JANELAS);
        (j.setup.is_some(), j.conferencia, j.cargas.clone(), j.pontos.clone())
    };
    let t = Tokens::get();
    if setup_aberta {
        janela_setup(ctx, &t);
    }
    if !conferencia && cargas.is_none() && pontos.is_none() {
        return;
    }
    let mut st: Estado = ctx.data_mut(|d| d.get_temp(id_estado())).unwrap_or_default();
    st.atualizar(app);
    let tem_desenho = st.chave.is_some();
    let s = setup();
    let mut selecionar: Option<Vec<Handle>> = None;
    let mut acoes: Vec<Acao> = Vec::new();

    if conferencia {
        egui::Window::new("Free Elétrica · Conferência")
            .id(egui::Id::new("ele-janela-conferencia"))
            .open(&mut conferencia)
            .default_pos(egui::pos2(480.0, 140.0))
            .default_width(460.0)
            .show(ctx, |ui| {
                if !tem_desenho {
                    ui.label(pequeno("Nenhum desenho aberto.", t.text_dim));
                    return;
                }
                egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                    secao_pontos(ui, &t, &st.analise);
                    ui.separator();
                    secao_circuitos(ui, &t, &st.analise, &mut selecionar);
                });
            });
    }

    let cargas = cargas.and_then(|mut c| {
        let mut aberta = true;
        egui::Window::new("Free Elétrica · Quadro de cargas")
            .id(egui::Id::new("ele-janela-cargas"))
            .open(&mut aberta)
            .default_pos(egui::pos2(240.0, 100.0))
            .default_width(1320.0)
            .default_height(640.0)
            .show(ctx, |ui| {
                if !tem_desenho {
                    ui.label(pequeno("Nenhum desenho aberto.", t.text_dim));
                    return;
                }
                if st.analise.quadros().is_empty() {
                    ui.label(pequeno("Nenhum quadro no desenho (bloco com TIPO_PONTO = QUADRO).", t.text_dim));
                    return;
                }
                let analise = st.analise.clone();
                ui.horizontal(|ui| {
                    if seletor_quadro(ui, "ele-cargas", &analise, &s, &mut c.quadro, &mut acoes) {
                        c.descricoes.clear();
                        c.excluir = None;
                    }
                });
                let Some(q) = c.quadro else { return };
                let Some(resumo) = st.resumo(app, &s, q) else { return };
                match resumo.as_ref() {
                    Err(e) => {
                        ui.label(pequeno(e.as_str(), COR_AVISO));
                    }
                    Ok(r) => {
                        egui::ScrollArea::both().auto_shrink([false, true]).max_height(ui.available_height().max(200.0)).show(ui, |ui| {
                            quadro_de_cargas(ui, &t, &analise, &s, r, &mut c, &mut selecionar, &mut acoes);
                        });
                    }
                }
            });
        aberta.then_some(c)
    });

    let pontos = pontos.and_then(|mut p| {
        let mut aberta = true;
        egui::Window::new("Free Elétrica · Inserir pontos")
            .id(egui::Id::new("ele-janela-pontos"))
            .open(&mut aberta)
            .default_pos(egui::pos2(300.0, 120.0))
            .default_width(820.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                if !tem_desenho {
                    ui.label(pequeno("Nenhum desenho aberto.", t.text_dim));
                    return;
                }
                inserir_pontos(ui, &t, &st.analise, &st.blocos, &mut p, &mut selecionar, &mut acoes);
            });
        aberta.then_some(p)
    });

    {
        let mut j = trava(&JANELAS);
        j.conferencia = conferencia;
        j.cargas = cargas;
        j.pontos = pontos;
    }
    for a in acoes {
        match a {
            Acao::Rodar(id, p) => {
                if let Err(e) = app.run(id, p) {
                    app.set_status(format!("Free Elétrica: {e}"));
                }
            }
            Acao::Iniciar(id) => app.start(id),
            Acao::Selecionar(h) => selecionar = Some(h),
        }
    }
    if let Some(h) = selecionar {
        app.session.set_selection(h);
    }
    ctx.data_mut(|d| d.insert_temp(id_estado(), st));
}

/// Uma coluna da tabela: título, largura e se o conteúdo é número (à direita).
type Coluna = (&'static str, f32, bool);

/// O quadro de cargas: o menu central dos circuitos de um quadro. Cada linha é um circuito, com
/// o que se edita (descrição, tipo de carga, FP, seção, disjuntor, isolação) e o que se calcula
/// (corrente, queda, potência por fase); embaixo, os totais e a demanda pela concessionária.
#[allow(clippy::too_many_arguments)]
fn quadro_de_cargas(
    ui: &mut egui::Ui,
    t: &Tokens,
    analise: &ele::Analise,
    s: &Setup,
    r: &ele::ResumoQuadro,
    c: &mut Cargas,
    selecionar: &mut Option<Vec<Handle>>,
    acoes: &mut Vec<Acao>,
) {
    let q = r.quadro;
    // Cabeçalho do painel.
    let alimentacao = match r.fases_quadro {
        1 => format!("{} V monofásico", fixo(r.tensao_fn, 0)),
        2 => format!("{}/{} V bifásico", fixo(r.tensao_ff, 0), fixo(r.tensao_fn, 0)),
        _ => format!("{}/{} V trifásico", fixo(r.tensao_ff, 0), fixo(r.tensao_fn, 0)),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("PAINEL: {}", r.tag)).strong().size(16.0));
        ui.add_space(12.0);
        ui.label(format!("Alimentação: {alimentacao}"));
        ui.add_space(12.0);
        ui.label(pequeno(format!("Demanda: {} · {}", r.demanda.tabela, r.demanda.edificacao), t.text_dim))
            .on_hover_text("Troque a concessionária e o tipo de edificação em Configurações > Demanda");
    });
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let proximo = ele::circuito::proximo_circuito(&analise.pontos, q);
        ui.add(egui::TextEdit::singleline(&mut c.nova_descricao).hint_text(format!("Descrição do circuito {proximo}")).desired_width(240.0));
        if ui.button("Novo circuito").on_hover_text("Cria o circuito vazio; depois use \"Adicionar pontos\" no menu ... da linha").clicked() {
            acoes.push(Acao::Rodar("eletrica.circuito.criar", json!({ "quadro": q.hex(), "descricao": c.nova_descricao.trim() })));
            c.nova_descricao.clear();
        }
        ui.separator();
        if ui.button("Ajustar fiação").on_hover_text("Desenha a simbologia de fiação deste quadro sobre os eletrodutos (ELEFIACAO)").clicked() {
            acoes.push(Acao::Rodar("elefiacao", json!({ "quadro": q.hex() })));
        }
        if ui.button("Copiar resumo").on_hover_text("Copia o resumo técnico em texto").clicked() {
            ui.ctx().copy_text(ele::quadro::texto(r, s));
        }
    });
    ui.add_space(4.0);

    let fases = usize::from(r.fases_quadro.clamp(1, 3));
    let mut colunas: Vec<Coluna> = vec![
        ("Circ.", 34.0, false),
        ("Descrição", 150.0, false),
        ("Tipo de carga", 124.0, false),
        ("Potência (VA)", 82.0, true),
        ("FP", 46.0, false),
        ("Potência (W)", 76.0, true),
        ("Tensão (V)", 62.0, true),
        ("Corrente (A)", 74.0, true),
        ("Queda (%)", 60.0, true),
        ("Seção (mm²)", 76.0, false),
        ("Disjuntor (A)", 78.0, false),
        ("Isolação", 70.0, false),
    ];
    for nome in ["Fase A (VA)", "Fase B (VA)", "Fase C (VA)"].into_iter().take(fases) {
        colunas.push((nome, 70.0, true));
    }
    colunas.push(("Situação", 64.0, false));
    colunas.push(("", 40.0, false));
    let largura = |i: usize| colunas.get(i).map_or(60.0, |c| c.1);
    let todos: Vec<(String, String)> = r
        .linhas
        .iter()
        .map(|l| (l.circuito.id.clone(), l.descricao.clone()))
        .chain(r.vazios.iter().map(|v| (v.id.clone(), v.descricao.clone())))
        .collect();

    egui::Grid::new("ele-cargas-tabela").num_columns(colunas.len()).striped(true).spacing(vec2(8.0, 2.0)).show(ui, |ui| {
        cabecalho(ui, t, &colunas);
        for l in &r.linhas {
            let k = &l.circuito;
            let e = &l.escolha;
            let id = &k.id;
            let link = ui.allocate_ui_with_layout(vec2(largura(0), LINHA), Layout::left_to_right(Align::Center), |ui| ui.link(id.as_str())).inner;
            if link.on_hover_text(format!("{} ponto(s) · {} · clique para selecionar no desenho", k.pontos.len(), k.natureza.nome())).clicked() {
                *selecionar = Some(k.pontos.clone());
            }
            campo_descricao(ui, c, q, id, &l.descricao, largura(1), acoes);
            // Tipo de carga e FP: o resultado dos atributos TIPO_CARGA e FP dos blocos.
            let tipos = if l.tipos_carga.is_empty() { "-".to_string() } else { l.tipos_texto() };
            celula(ui, largura(2), tipos.as_str(), false).on_hover_text(format!("{tipos}\n(atributo TIPO_CARGA de cada bloco)"));
            celula(ui, largura(3), numero(fixo(k.potencia, 0)), true);
            celula(ui, largura(4), numero(fixo(l.fp, 2)), true).on_hover_text("W / VA do circuito (atributo FP de cada bloco)");
            celula(ui, largura(5), numero(fixo(l.potencia_w, 0)), true);
            celula(ui, largura(6), numero(fixo(l.tensao, 0)), true);
            celula(ui, largura(7), numero(fixo(e.ib, 2)), true);
            let queda = e.queda.map_or("-".to_string(), |v| fixo(v, 2));
            let comprimento = l.comprimento.map_or("-".to_string(), |v| fixo(v, 1));
            let estimado = if l.fora_da_rede > 0 { format!(" (estimado: {} ponto(s) fora da rede)", l.fora_da_rede) } else { String::new() };
            celula(ui, largura(8), numero(queda), true)
                .on_hover_text(format!("Comprimento {comprimento} m{estimado} · admissível {} %", br(l.queda_admissivel)));
            // Seção e disjuntor: a pré-seleção, que o projetista pode trocar.
            let mut sob = l.sobrescrita;
            let antes = sob;
            ui.allocate_ui_with_layout(vec2(largura(9), LINHA), Layout::left_to_right(Align::Center), |ui| {
                escolher(ui, ("ele-secao", id), &mut sob.secao, l.automatico.secao, &SECOES, largura(9) - 4.0);
            });
            ui.allocate_ui_with_layout(vec2(largura(10), LINHA), Layout::left_to_right(Align::Center), |ui| {
                escolher(ui, ("ele-disj", id), &mut sob.disjuntor, l.automatico.disjuntor, &DISJUNTORES, largura(10) - 4.0);
            });
            if sob != antes {
                acoes.push(Acao::Rodar(
                    "eletrica.sobrescrever",
                    json!({ "quadro": q.hex(), "circuito": id, "secao": sob.secao, "disjuntor": sob.disjuntor }),
                ));
            }
            // Isolação (tipo de cabo) do circuito.
            ui.allocate_ui_with_layout(vec2(largura(11), LINHA), Layout::left_to_right(Align::Center), |ui| {
                let texto = if l.isolacao_propria { RichText::new(l.isolacao.codigo()).strong() } else { RichText::new(l.isolacao.codigo()) };
                egui::ComboBox::from_id_salt(("ele-isol", id.as_str())).selected_text(texto).width(largura(11) - 4.0).show_ui(ui, |ui| {
                    if ui.selectable_label(!l.isolacao_propria, format!("Do quadro ({})", r.isolacao.codigo())).clicked() && l.isolacao_propria {
                        acoes.push(Acao::Rodar("eletrica.circuito.definir", json!({ "quadro": q.hex(), "circuito": id, "isolacao": null })));
                    }
                    for i in Isolacao::TODAS {
                        if ui.selectable_label(l.isolacao_propria && l.isolacao == i, i.nome()).clicked() {
                            acoes
                                .push(Acao::Rodar("eletrica.circuito.definir", json!({ "quadro": q.hex(), "circuito": id, "isolacao": i.codigo() })));
                        }
                    }
                });
            });
            for f in 0..fases {
                let v = l.carga_fase.get(f).copied().unwrap_or(0.0);
                celula(ui, largura(12 + f), numero(if v > 0.0 { fixo(v, 0) } else { String::new() }), true);
            }
            let mut avisos = l.automatico.avisos.clone();
            avisos.extend(e.avisos.iter().cloned());
            avisos.extend(k.avisos.iter().cloned());
            avisos.sort();
            avisos.dedup();
            let detalhes = format!(
                "Iz {} A · FCT {} · FCA {} ({} circuito(s) no trecho mais cheio, tabela 42) · fases {}",
                e.iz.map_or("-".to_string(), |v| fixo(v, 1)),
                fixo(l.fct, 2),
                fixo(l.fca, 2),
                l.agrupados,
                l.fases_texto()
            );
            let situacao = if avisos.is_empty() {
                celula(ui, largura(12 + fases), "OK", false)
            } else {
                celula(ui, largura(12 + fases), RichText::new(format!("{} aviso(s)", avisos.len())).color(COR_AVISO), false)
            };
            situacao.on_hover_text(if avisos.is_empty() { detalhes } else { format!("{}\n\n{detalhes}", avisos.join("\n")) });
            acoes_do_circuito(ui, c, q, id, &k.pontos, &todos, acoes);
            ui.end_row();
        }
        // Circuitos criados ainda sem pontos.
        for v in &r.vazios {
            celula(ui, largura(0), v.id.as_str(), false);
            campo_descricao(ui, c, q, &v.id, &v.descricao, largura(1), acoes);
            celula(ui, largura(2), pequeno("vazio", t.text_dim), false);
            for i in 3..12 + fases + 1 {
                celula(ui, largura(i), "", false);
            }
            acoes_do_circuito(ui, c, q, &v.id, &[], &todos, acoes);
            ui.end_row();
        }
        // Linha de total.
        celula(ui, largura(0), "", false);
        celula(ui, largura(1), RichText::new("TOTAL").strong(), false);
        celula(ui, largura(2), "", false);
        celula(ui, largura(3), numero(fixo(r.potencia_total, 0)).strong(), true);
        celula(ui, largura(4), "", false);
        celula(ui, largura(5), numero(fixo(r.linhas.iter().map(|l| l.potencia_w).sum::<f64>(), 0)).strong(), true);
        for i in 6..12 {
            celula(ui, largura(i), "", false);
        }
        for f in 0..fases {
            celula(ui, largura(12 + f), numero(fixo(r.carga_por_fase.get(f).copied().unwrap_or(0.0), 0)).strong(), true);
        }
        celula(ui, largura(12 + fases), "", false);
        celula(ui, largura(13 + fases), "", false);
        ui.end_row();
    });
    if r.linhas.is_empty() && r.vazios.is_empty() {
        ui.label(pequeno("Nenhum circuito neste quadro: crie um acima ou use Criar circuito na paleta.", t.text_dim));
    }
    ui.label(pequeno(
        "Tipo de carga e FP vêm dos atributos TIPO_CARGA e FP de cada bloco (sem eles: TUE é aquecimento com FP 1,00, o resto iluminação e TUG com 0,92). Em negrito, o que foi escolhido à mão; seção e disjuntor: \"Auto\" volta à pré-seleção. Ctrl+Z desfaz.",
        t.text_dim,
    ));
    ui.add_space(8.0);

    // Demanda e totais.
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width(460.0);
            ui.label(RichText::new("Demanda").strong());
            egui::Grid::new("ele-demanda").num_columns(4).striped(true).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
                cabecalho(
                    ui,
                    t,
                    &[("Tipo de carga", 140.0, false), ("Instalada (VA)", 96.0, true), ("Fator", 64.0, true), ("Demandada (VA)", 104.0, true)],
                );
                for l in &r.demanda.linhas {
                    celula(ui, 140.0, l.tipo.nome(), false).on_hover_text(&l.regra);
                    celula(ui, 96.0, numero(fixo(l.instalada, 0)), true);
                    celula(ui, 64.0, numero(format!("{} %", fixo(l.fator * 100.0, 1))), true).on_hover_text(&l.regra);
                    celula(ui, 104.0, numero(fixo(l.demandada, 0)), true);
                    ui.end_row();
                }
            });
            ui.add(egui::Label::new(pequeno(format!("Fonte: {}", r.demanda.fonte), t.text_dim)).wrap());
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Totais do painel").strong());
            egui::Grid::new("ele-totais").num_columns(2).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
                let linha = |ui: &mut egui::Ui, nome: &str, valor: String| {
                    ui.label(nome);
                    celula(ui, 100.0, numero(valor), true);
                    ui.end_row();
                };
                linha(ui, "Potência instalada", format!("{} VA", fixo(r.demanda.instalada, 0)));
                linha(ui, "Potência demandada", format!("{} VA", fixo(r.demanda.demandada, 0)));
                linha(ui, "Corrente instalada", format!("{} A", fixo(r.corrente_instalada, 2)));
                linha(ui, "Corrente demandada", format!("{} A", fixo(r.corrente_demandada, 2)));
                if r.fases_quadro > 1 {
                    linha(ui, "Desequilíbrio", format!("{} %", fixo(r.desequilibrio, 1)));
                }
            });
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("Cabos (fator {})", br(s.fator_materiais))).strong());
            egui::Grid::new("ele-materiais").num_columns(2).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
                for (secao, m) in &r.materiais {
                    ui.label(format!("{} mm²", br(*secao)));
                    celula(ui, 70.0, numero(format!("{} m", fixo(m.ceil(), 0))), true);
                    ui.end_row();
                }
            });
        });
    });
    lista_avisos(ui, "ele-avisos-resumo", &r.avisos);
}

/// Descrição editável: grava ao sair do campo.
fn campo_descricao(ui: &mut egui::Ui, c: &mut Cargas, q: Handle, id: &str, gravada: &str, largura: f32, acoes: &mut Vec<Acao>) {
    let i = match c.descricoes.iter().position(|(k, _)| k == id) {
        Some(i) => i,
        None => {
            c.descricoes.push((id.to_string(), gravada.to_string()));
            c.descricoes.len() - 1
        }
    };
    if let Some((_, texto)) = c.descricoes.get_mut(i) {
        let r = ui.add_sized(vec2(largura, LINHA), egui::TextEdit::singleline(texto).hint_text("sem descrição"));
        if r.lost_focus() && texto.trim() != gravada {
            acoes.push(Acao::Rodar("eletrica.circuito.descrever", json!({ "quadro": q.hex(), "circuito": id, "descricao": texto.trim() })));
        }
    }
}

/// O menu "..." de um circuito (adicionar e selecionar pontos, juntar, excluir), com a confirmação
/// da exclusão na própria linha.
fn acoes_do_circuito(ui: &mut egui::Ui, c: &mut Cargas, q: Handle, id: &str, pontos: &[Handle], todos: &[(String, String)], acoes: &mut Vec<Acao>) {
    ui.horizontal(|ui| {
        if c.excluir.as_deref() == Some(id) {
            if ui.small_button(RichText::new("Excluir?").color(COR_AVISO)).on_hover_text("Os pontos ficam sem circuito (Ctrl+Z desfaz)").clicked() {
                acoes.push(Acao::Rodar("eletrica.circuito.excluir", json!({ "quadro": q.hex(), "circuito": id })));
                c.excluir = None;
            }
            if ui.small_button("Não").clicked() {
                c.excluir = None;
            }
            return;
        }
        ui.menu_button("...", |ui| {
            if ui.button("Adicionar pontos").on_hover_text("Clique nos pontos no desenho; Enter termina").clicked() {
                acoes.push(Acao::Rodar("eletrica.circuito.alvo", json!({ "quadro": q.hex(), "circuito": id })));
                acoes.push(Acao::Iniciar("elecircuito"));
                ui.close();
            }
            if ui.add_enabled(!pontos.is_empty(), egui::Button::new("Selecionar pontos")).clicked() {
                acoes.push(Acao::Selecionar(pontos.to_vec()));
                ui.close();
            }
            ui.menu_button("Juntar em", |ui| {
                for (outro, descricao) in todos.iter().filter(|(o, _)| o != id) {
                    if ui.button(format!("{outro}  {descricao}")).clicked() {
                        acoes.push(Acao::Rodar("eletrica.circuito.juntar", json!({ "quadro": q.hex(), "de": id, "para": outro })));
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.button("Excluir circuito").clicked() {
                c.excluir = Some(id.to_string());
                ui.close();
            }
        });
    });
}

/// Uma caixa de escolha com "Auto" (a pré-seleção) e os valores da tabela.
fn escolher(ui: &mut egui::Ui, id: (&str, &String), valor: &mut Option<f64>, automatico: Option<f64>, opcoes: &[f64], largura: f32) {
    let auto = automatico.map_or("-".to_string(), br);
    let texto = match valor {
        Some(v) => RichText::new(br(*v)).strong(),
        None => RichText::new(auto.clone()),
    };
    egui::ComboBox::from_id_salt(id).selected_text(texto).width(largura).show_ui(ui, |ui| {
        if ui.selectable_label(valor.is_none(), format!("Auto ({auto})")).clicked() {
            *valor = None;
        }
        for o in opcoes {
            if ui.selectable_label(*valor == Some(*o), br(*o)).clicked() {
                *valor = Some(*o);
            }
        }
    });
}

/// Os tipos do filtro da janela Inserir pontos, na ordem dos botões.
const TIPOS_FILTRO: [TipoPonto; 7] = [
    TipoPonto::Iluminacao,
    TipoPonto::Tomada,
    TipoPonto::TomadaEspecifica,
    TipoPonto::Interruptor,
    TipoPonto::Quadro,
    TipoPonto::Caixa,
    TipoPonto::Outro,
];

/// A janela Inserir pontos: filtro por tipo, os blocos de ponto do desenho, os valores para a
/// inserção e os pontos já inseridos (selecionar, excluir).
fn inserir_pontos(
    ui: &mut egui::Ui,
    t: &Tokens,
    analise: &ele::Analise,
    blocos: &[ele::biblioteca::BlocoPonto],
    p: &mut Pontos,
    selecionar: &mut Option<Vec<Handle>>,
    acoes: &mut Vec<Acao>,
) {
    // Filtro por tipo e busca pelo nome.
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(p.filtro.is_none(), format!("Todos ({})", analise.pontos.len())).clicked() {
            p.filtro = None;
            p.excluir = None;
        }
        for tipo in TIPOS_FILTRO {
            let n = analise.contar(tipo);
            let tem_bloco = blocos.iter().any(|b| b.tipo == tipo);
            if (n > 0 || tem_bloco) && ui.selectable_label(p.filtro == Some(tipo), format!("{} ({n})", tipo.nome())).clicked() {
                p.filtro = Some(tipo);
                p.excluir = None;
            }
        }
        ui.add_space(12.0);
        ui.add(egui::TextEdit::singleline(&mut p.busca).hint_text("Buscar bloco ou TAG").desired_width(170.0));
    });
    ui.separator();
    let busca = p.busca.trim().to_uppercase();
    let combina = |texto: &str| busca.is_empty() || texto.to_uppercase().contains(&busca);
    let filtro = p.filtro;
    let do_tipo = |tipo: TipoPonto| filtro.is_none_or(|f| f == tipo);

    if blocos.is_empty() {
        ui.label(pequeno(
            "O desenho não tem blocos de ponto elétrico (blocos com TIPO_PONTO, POTENCIA... na definição). Insira o template templates\\FREE_ELETRICA_BLOCOS.dxf no desenho para trazer os blocos.",
            t.text_dim,
        ));
    }
    if p.bloco.as_ref().is_some_and(|nome| !blocos.iter().any(|b| &b.nome == nome)) {
        p.bloco = None;
        p.valores.clear();
    }

    ui.horizontal_top(|ui| {
        // Altura fixa: o separador vertical entre as colunas não pode crescer a janela.
        ui.set_max_height(250.0);
        // Blocos disponíveis.
        ui.vertical(|ui| {
            ui.set_width(300.0);
            ui.label(RichText::new("Blocos").strong());
            egui::ScrollArea::vertical().id_salt("ele-pontos-blocos").max_height(220.0).show(ui, |ui| {
                for b in blocos.iter().filter(|b| do_tipo(b.tipo) && combina(&b.nome)) {
                    let texto = format!("{}  ·  {}  ·  {} no desenho", b.nome, b.tipo.nome(), b.inseridos);
                    if ui.selectable_label(p.bloco.as_deref() == Some(b.nome.as_str()), texto).clicked() {
                        p.bloco = Some(b.nome.clone());
                        p.valores = b.atributos.iter().filter(|a| !a.constante).map(|a| (a.tag.clone(), a.padrao.clone())).collect();
                        p.excluir = None;
                    }
                }
            });
        });
        ui.separator();
        // Valores da inserção.
        ui.vertical(|ui| {
            let Some(nome) = p.bloco.clone() else {
                ui.label(pequeno("Escolha um bloco para inserir.", t.text_dim));
                return;
            };
            let prompts: Vec<(String, String)> = blocos
                .iter()
                .find(|b| b.nome == nome)
                .map(|b| b.atributos.iter().map(|a| (a.tag.clone(), a.prompt.clone())).collect())
                .unwrap_or_default();
            ui.label(RichText::new(format!("Inserir {nome}")).strong());
            egui::Grid::new("ele-pontos-valores").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                for (tag, valor) in &mut p.valores {
                    let dica =
                        prompts.iter().find(|(t, _)| t == tag).map(|(_, pr)| pr.clone()).filter(|pr| !pr.is_empty()).unwrap_or_else(|| tag.clone());
                    ui.label(tag.as_str()).on_hover_text(dica);
                    ui.add(egui::TextEdit::singleline(valor).desired_width(170.0));
                    ui.end_row();
                }
                ui.label("Rotação");
                ui.add(campo(&mut p.rotacao, -360.0..=360.0, 1.0, 0, "°"));
                ui.end_row();
            });
            ui.add_space(4.0);
            if ui.button("Inserir no desenho").on_hover_text("Clique no desenho onde vai cada ponto; Enter termina (ELEPONTO)").clicked() {
                let valores: serde_json::Map<String, Value> = p.valores.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
                acoes.push(Acao::Rodar("eletrica.ponto.modelo", json!({ "bloco": nome, "rotacao": p.rotacao, "valores": valores })));
                acoes.push(Acao::Iniciar("eleponto"));
            }
        });
    });

    // Pontos já inseridos, do tipo filtrado.
    ui.separator();
    let lista: Vec<&PontoEletrico> =
        analise.pontos.iter().filter(|q| do_tipo(q.tipo) && (combina(&q.bloco) || q.tag.as_deref().is_some_and(combina))).collect();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Pontos no desenho ({})", lista.len())).strong());
        if !lista.is_empty() {
            if ui.small_button("Selecionar todos").clicked() {
                *selecionar = Some(lista.iter().map(|q| q.handle).collect());
            }
            if p.excluir == Some(None) {
                if ui.small_button(RichText::new(format!("Excluir os {}?", lista.len())).color(COR_AVISO)).on_hover_text("Ctrl+Z desfaz").clicked() {
                    let hs: Vec<String> = lista.iter().map(|q| q.handle.hex()).collect();
                    acoes.push(Acao::Rodar("eletrica.ponto.excluir", json!({ "pontos": hs })));
                    p.excluir = None;
                }
                if ui.small_button("Não").clicked() {
                    p.excluir = None;
                }
            } else if ui.small_button("Excluir todos").clicked() {
                p.excluir = Some(None);
            }
        }
    });
    let colunas: [(&str, f32, bool); 6] = [
        ("Ponto", 150.0, false),
        ("Bloco", 150.0, false),
        ("Tipo", 100.0, false),
        ("Circuito", 60.0, false),
        ("Potência (VA)", 90.0, true),
        ("", 90.0, false),
    ];
    egui::ScrollArea::vertical().id_salt("ele-pontos-lista").max_height(220.0).show(ui, |ui| {
        egui::Grid::new("ele-pontos-tabela").num_columns(colunas.len()).striped(true).spacing(vec2(10.0, 2.0)).show(ui, |ui| {
            cabecalho(ui, t, &colunas);
            for q in &lista {
                let link =
                    ui.allocate_ui_with_layout(vec2(colunas[0].1, LINHA), Layout::left_to_right(Align::Center), |ui| ui.link(q.rotulo())).inner;
                if link.on_hover_text("Selecionar no desenho").clicked() {
                    *selecionar = Some(vec![q.handle]);
                }
                celula(ui, colunas[1].1, q.bloco.as_str(), false);
                celula(ui, colunas[2].1, q.tipo.nome(), false);
                celula(ui, colunas[3].1, q.circuito.as_deref().unwrap_or("-"), false);
                celula(ui, colunas[4].1, numero(q.potencia.map_or("-".to_string(), |v| fixo(v, 0))), true);
                ui.horizontal(|ui| {
                    if p.excluir == Some(Some(q.handle)) {
                        if ui.small_button(RichText::new("Excluir?").color(COR_AVISO)).clicked() {
                            acoes.push(Acao::Rodar("eletrica.ponto.excluir", json!({ "pontos": [q.handle.hex()] })));
                            p.excluir = None;
                        }
                        if ui.small_button("Não").clicked() {
                            p.excluir = None;
                        }
                    } else if ui.small_button("Excluir").clicked() {
                        p.excluir = Some(Some(q.handle));
                    }
                });
                ui.end_row();
            }
        });
    });
}

fn janela_setup(ctx: &egui::Context, t: &Tokens) {
    let Some(mut r) = trava(&JANELAS).setup.clone() else { return };
    let mut aberta = true;
    let mut fechar = false;
    egui::Window::new("Free Elétrica · Configurações")
        .id(egui::Id::new("ele-janela-setup"))
        .open(&mut aberta)
        .collapsible(false)
        .resizable(false)
        .default_pos(egui::pos2(420.0, 120.0))
        .default_width(520.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for aba in Aba::TODAS {
                    if ui.selectable_label(r.aba == aba, aba.nome()).clicked() {
                        r.aba = aba;
                    }
                }
            });
            ui.separator();
            match r.aba {
                Aba::Geral => aba_geral(ui, t, &mut r.setup),
                Aba::Condutores => aba_condutores(ui, t, &mut r.setup),
                Aba::Eletrodutos => aba_eletrodutos(ui, t, &mut r.setup),
                Aba::Agrupamento => aba_agrupamento(ui, t),
                Aba::Demanda => aba_demanda(ui, t, &mut r.setup),
                Aba::Telecom => aba_telecom(ui, t, &mut r),
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    definir_setup(r.setup.clone());
                    fechar = true;
                }
                if ui.button("Cancelar").clicked() {
                    fechar = true;
                }
            });
        });
    trava(&JANELAS).setup = (aberta && !fechar).then_some(r);
}

fn aba_geral(ui: &mut egui::Ui, t: &Tokens, s: &mut Setup) {
    egui::Grid::new("ele-setup-geral").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        ui.label("Sistema de distribuição");
        egui::ComboBox::from_id_salt("ele-sistema").selected_text(s.sistema.nome()).width(230.0).show_ui(ui, |ui| {
            for sistema in Sistema::TODOS {
                ui.selectable_value(&mut s.sistema, sistema, sistema.nome());
            }
        });
        ui.end_row();
        ui.label("Tensões");
        let ff = s.sistema.tensao_ff().map_or_else(|| "-".to_string(), |v| format!("{} V", br(v)));
        ui.label(format!("F-N {} V · F-F {ff} · {} fase(s)", br(s.sistema.tensao_fn()), s.sistema.fases()));
        ui.end_row();
        ui.label("Pé-direito padrão");
        ui.add(campo(&mut s.pe_direito, 2.0..=10.0, 0.01, 2, " m"));
        ui.end_row();
        ui.label("Unidade do desenho");
        egui::ComboBox::from_id_salt("ele-unidade").selected_text(s.unidade.nome()).width(230.0).show_ui(ui, |ui| {
            for u in Unidade::TODAS {
                ui.selectable_value(&mut s.unidade, u, u.nome());
            }
        });
        ui.end_row();
        ui.label("Camadas dos eletrodutos");
        ui.add(egui::TextEdit::singleline(&mut s.camadas_eletroduto).desired_width(230.0))
            .on_hover_text("Separadas por vírgula; * e ? valem como curingas");
        ui.end_row();
        ui.label("Eletrodutos no piso");
        ui.add(egui::TextEdit::singleline(&mut s.camadas_piso).desired_width(230.0))
            .on_hover_text("Entre as camadas de eletroduto, as que correm no piso; as demais correm no teto");
        ui.end_row();
        ui.label("Raio de captura");
        ui.add(campo(&mut s.raio_captura, 0.01..=5.0, 0.01, 2, " m")).on_hover_text("Distância máxima entre um ponto e o eletroduto que chega nele");
        ui.end_row();
    });
    ui.label(pequeno(
        "Tensões e fases valem quando o bloco não informa TENSAO ou o quadro não informa NUMERO_FASES. Os eletrodutos correm no pé-direito (ou no piso); cada ponto desce ou sobe até a sua ALTURA.",
        t.text_dim,
    ));
}

fn aba_condutores(ui: &mut egui::Ui, t: &Tokens, s: &mut Setup) {
    egui::Grid::new("ele-setup-condutores").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        ui.label("Isolação padrão");
        egui::ComboBox::from_id_salt("ele-isolacao").selected_text(s.isolacao.nome()).width(160.0).show_ui(ui, |ui| {
            for i in Isolacao::TODAS {
                ui.selectable_value(&mut s.isolacao, i, i.nome());
            }
        });
        ui.end_row();
        ui.label("Material / método");
        ui.label("Cobre · B1: condutores isolados em eletroduto");
        ui.end_row();
        ui.label("Temperatura do projeto");
        ui.add(campo(&mut s.temperatura_ambiente, 10.0..=60.0, 1.0, 0, " °C"));
        ui.end_row();
        ui.label("FCT (tabela 40)");
        ui.label(fixo(s.fct(), 2));
        ui.end_row();
        ui.label("Queda de tensão admissível");
        ui.add(campo(&mut s.queda_admissivel, 0.5..=10.0, 0.1, 1, " %"));
        ui.end_row();
        ui.label("Fator para materiais");
        ui.add(campo(&mut s.fator_materiais, 1.0..=2.0, 0.01, 2, ""))
            .on_hover_text("Multiplica os comprimentos de cabo no levantamento (perdas, emendas, sobras)");
        ui.end_row();
        for (f, nome) in ["Cor da fase A", "Cor da fase B", "Cor da fase C"].into_iter().enumerate() {
            ui.label(nome);
            if let Some(atual) = s.cores_fase.get_mut(f) {
                egui::ComboBox::from_id_salt(("ele-cor", f)).selected_text(atual.nome()).width(120.0).show_ui(ui, |ui| {
                    for c in CorCabo::TODAS {
                        ui.selectable_value(atual, c, c.nome());
                    }
                });
            }
            ui.end_row();
        }
    });
    ui.label("Omitir nas indicações e no resumo as seções:");
    ui.horizontal_wrapped(|ui| {
        for secao in SECOES.iter().copied().take(6) {
            let mut on = s.omitida(secao);
            if ui.checkbox(&mut on, format!("{} mm²", br(secao))).changed() {
                if on {
                    s.secoes_omitidas.push(secao);
                } else {
                    s.secoes_omitidas.retain(|x| (x - secao).abs() > 1e-6);
                }
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Diâmetro externo dos condutores").strong());
        if ui.small_button("Valores típicos").on_hover_text("Volta à tabela de fábrica").clicked() {
            s.diametros_condutor = diametros_padrao();
        }
    });
    egui::ScrollArea::vertical().id_salt("ele-diametros").max_height(170.0).show(ui, |ui| {
        egui::Grid::new("ele-setup-diametros").num_columns(3).striped(true).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
            cabecalho(ui, t, &[("Seção (mm²)", 80.0, true), ("PVC 450/750 V (mm)", 130.0, false), ("EPR/XLPE 0,6/1 kV (mm)", 150.0, false)]);
            for d in &mut s.diametros_condutor {
                let DiametroCondutor { secao, pvc, epr_xlpe } = d;
                celula(ui, 80.0, numero(br(*secao)), true);
                ui.add(campo(pvc, 0.0..=100.0, 0.05, 1, ""));
                ui.add(campo(epr_xlpe, 0.0..=100.0, 0.05, 1, ""));
                ui.end_row();
            }
        });
    });
    ui.label(pequeno(
        "Valores típicos de catálogo; confira com o fabricante do cabo usado. Neutro azul-claro e proteção verde-amarelo são fixos pela NBR 5410.",
        t.text_dim,
    ));
}

fn aba_eletrodutos(ui: &mut egui::Ui, t: &Tokens, s: &mut Setup) {
    egui::Grid::new("ele-setup-eletrodutos").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        ui.label("Tipo de eletroduto");
        ui.add(egui::TextEdit::singleline(&mut s.tipo_eletroduto).desired_width(260.0));
        ui.end_row();
        ui.label("Diâmetro mínimo");
        let minimo = s.eletroduto_minimo;
        egui::ComboBox::from_id_salt("ele-minimo").selected_text(format!("{} mm", br(minimo))).width(120.0).show_ui(ui, |ui| {
            for e in &s.eletrodutos {
                if ui.selectable_label((e.nominal - minimo).abs() < 1e-6, format!("{} mm ({})", br(e.nominal), e.polegadas)).clicked() {
                    s.eletroduto_minimo = e.nominal;
                }
            }
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Tamanhos").strong());
        if ui.small_button("Valores típicos").on_hover_text("Volta à tabela de fábrica").clicked() {
            s.eletrodutos = eletrodutos_padrao();
        }
    });
    egui::Grid::new("ele-setup-tamanhos").num_columns(3).striped(true).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
        cabecalho(ui, t, &[("Nominal (mm)", 90.0, true), ("Polegadas", 80.0, false), ("Diâmetro interno (mm)", 140.0, false)]);
        for e in &mut s.eletrodutos {
            let TamanhoEletroduto { nominal, polegadas, interno } = e;
            celula(ui, 90.0, numero(br(*nominal)), true);
            celula(ui, 80.0, polegadas.as_str(), false);
            ui.add(campo(interno, 0.0..=500.0, 0.1, 1, ""));
            ui.end_row();
        }
    });
    ui.label(pequeno(
        "Ajustar fiação escolhe, em cada trecho, o menor eletroduto a partir do mínimo que comporta os condutores: ocupação máxima de 53 % com um condutor, 31 % com dois e 40 % com três ou mais (NBR 5410, 6.2.11.1.6). Diâmetros internos típicos de catálogo; confira com o fabricante.",
        t.text_dim,
    ));
}

fn aba_agrupamento(ui: &mut egui::Ui, t: &Tokens) {
    egui::Grid::new("ele-setup-agrupamento").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        ui.label("FCA (tabela 42)");
        ui.label("automático");
        ui.end_row();
    });
    ui.label(pequeno(
        "Ao rotear um quadro, cada circuito usa o fator do trecho de eletroduto mais cheio por onde passa (número de circuitos daquele quadro no trecho). Pontos fora da rede ficam com FCA 1,00.",
        t.text_dim,
    ));
}

fn aba_demanda(ui: &mut egui::Ui, t: &Tokens, s: &mut Setup) {
    let tabela = s.tabela_demanda();
    egui::Grid::new("ele-setup-demanda").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        ui.label("Concessionária");
        egui::ComboBox::from_id_salt("ele-concessionaria").selected_text(tabela.nome).width(300.0).show_ui(ui, |ui| {
            for tb in ele::demanda::tabelas() {
                if ui.selectable_label(tb.id == tabela.id, tb.nome).clicked() {
                    s.concessionaria = tb.id.to_string();
                }
            }
        });
        ui.end_row();
        ui.label("Tipo de edificação");
        let atual = tabela.edificacao(&s.edificacao).nome;
        egui::ComboBox::from_id_salt("ele-edificacao").selected_text(atual).width(300.0).show_ui(ui, |ui| {
            for e in tabela.edificacoes {
                if ui.selectable_label(e.nome == atual, e.nome).clicked() {
                    s.edificacao = e.nome.to_string();
                }
            }
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    let ed = tabela.edificacao(&s.edificacao);
    ui.label(RichText::new("Iluminação e tomadas de uso geral").strong());
    let mut inicio = 0.0;
    for f in ed.faixas {
        let faixa = match f.ate_kw {
            Some(ate) if inicio == 0.0 => format!("até {} kW", br(ate)),
            Some(ate) => format!("de {} a {} kW", br(inicio), br(ate)),
            None if inicio == 0.0 => "toda a potência".to_string(),
            None => format!("acima de {} kW", br(inicio)),
        };
        ui.label(format!("    {faixa}: {} %", br(f.fator * 100.0)));
        inicio = f.ate_kw.unwrap_or(inicio);
    }
    egui::CollapsingHeader::new("Aquecimento e eletrodomésticos (por número de aparelhos)").id_salt("ele-demanda-aparelhos").show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
            egui::Grid::new("ele-demanda-tab4").num_columns(3).striped(true).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
                let lim = br(tabela.limite_aparelho_kw);
                cabecalho(ui, t, &[("Aparelhos", 80.0, true), ("Até 3,5 kW", 90.0, true), ("Acima", 90.0, true)]);
                let mut de = 1u32;
                for q in tabela.aparelhos {
                    let n = match q.ate {
                        u32::MAX => format!("{de} ou mais"),
                        a if a == de => a.to_string(),
                        a => format!("{de} a {a}"),
                    };
                    celula(ui, 80.0, numero(n), true);
                    celula(ui, 90.0, numero(fixo(q.ate_limite, 2)), true).on_hover_text(format!("potência individual até {lim} kW"));
                    celula(ui, 90.0, numero(fixo(q.acima_limite, 2)), true);
                    ui.end_row();
                    de = q.ate.saturating_add(1);
                }
            });
        });
    });
    egui::CollapsingHeader::new("Ar condicionado e bombas").id_salt("ele-demanda-ar").show(ui, |ui| {
        let lista = |ui: &mut egui::Ui, nome: &str, tab: &[ele::demanda::PorQuantidade]| {
            let mut de = 1u32;
            let partes: Vec<String> = tab
                .iter()
                .map(|q| {
                    let n = match q.ate {
                        u32::MAX => format!("{de}+"),
                        a if a == de => a.to_string(),
                        a => format!("{de}-{a}"),
                    };
                    de = q.ate.saturating_add(1);
                    format!("{n}: {}", fixo(q.fator, 2))
                })
                .collect();
            ui.label(format!("{nome}: {}", partes.join(" · ")));
        };
        lista(ui, "Ar condicionado", tabela.ar_condicionado);
        lista(ui, "Bombas d'água", tabela.bombas);
    });
    ui.add_space(4.0);
    ui.label(pequeno(format!("Fonte: {}", tabela.fonte), t.text_dim));
    ui.label(pequeno(
        "O tipo de carga de cada ponto vem do atributo TIPO_CARGA do bloco (ILUM_TUG, AQUECIMENTO, ELETRODOMESTICOS, AR, BOMBA, OUTROS, RESERVA; sem ele, TUE é aquecimento). Outras cargas entram com 100 % e reservas ficam fora. Confira a revisão da norma em vigor na sua área de concessão.",
        t.text_dim,
    ));
}

fn aba_telecom(ui: &mut egui::Ui, t: &Tokens, r: &mut Rascunho) {
    let mut remover: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("ele-telecom-lista").max_height(220.0).show(ui, |ui| {
        egui::Grid::new("ele-telecom").num_columns(6).striped(true).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
            cabecalho(
                ui,
                t,
                &[
                    ("Cabo", 130.0, false),
                    ("Tipo", 80.0, false),
                    ("Qtd.", 70.0, false),
                    ("Diâmetro (mm)", 90.0, false),
                    ("Observação", 100.0, false),
                    ("", 60.0, false),
                ],
            );
            for c in &mut r.setup.cabos_telecom {
                celula(ui, 130.0, c.nome.as_str(), false);
                celula(ui, 80.0, c.categoria.nome(), false);
                celula(ui, 70.0, format!("{} {}", c.quantidade, c.categoria.unidade()), false);
                ui.add(campo(&mut c.diametro, 0.0..=100.0, 0.05, 1, ""));
                celula(ui, 100.0, c.observacao.as_str(), false);
                if c.padrao {
                    celula(ui, 60.0, pequeno("padrão", t.text_dim), false);
                } else if ui.small_button("Remover").clicked() {
                    remover = Some(c.nome.clone());
                }
                ui.end_row();
            }
        });
    });
    if let Some(n) = remover {
        r.setup.remover_cabo(&n);
    }
    ui.separator();
    ui.label(RichText::new("Cadastrar novo cabo").strong());
    egui::Grid::new("ele-telecom-novo").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
        ui.label("Nome");
        ui.add(egui::TextEdit::singleline(&mut r.novo_cabo.nome).hint_text("Ex.: UTP Cat 6A").desired_width(200.0));
        ui.end_row();
        ui.label("Tipo");
        egui::ComboBox::from_id_salt("ele-telecom-tipo").selected_text(r.novo_cabo.categoria.nome()).show_ui(ui, |ui| {
            for c in CategoriaTelecom::TODAS {
                ui.selectable_value(&mut r.novo_cabo.categoria, c, c.nome());
            }
        });
        ui.end_row();
        ui.label(format!("Quantidade ({})", r.novo_cabo.categoria.unidade()));
        ui.add(egui::DragValue::new(&mut r.novo_cabo.quantidade).range(1..=10_000));
        ui.end_row();
        ui.label("Diâmetro externo");
        ui.add(campo(&mut r.novo_cabo.diametro, 0.0..=100.0, 0.05, 1, " mm"))
            .on_hover_text("Obrigatório: entra no cálculo de ocupação dos eletrodutos");
        ui.end_row();
        ui.label("Observação");
        ui.add(egui::TextEdit::singleline(&mut r.novo_cabo.observacao).desired_width(200.0));
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if ui.button("Cadastrar").clicked() {
            match r.setup.cadastrar_cabo(r.novo_cabo.clone()) {
                Ok(()) => {
                    r.erro = None;
                    r.novo_cabo.nome.clear();
                    r.novo_cabo.observacao.clear();
                    r.novo_cabo.diametro = 0.0;
                }
                Err(e) => r.erro = Some(e),
            }
        }
        if let Some(e) = &r.erro {
            ui.label(pequeno(e.as_str(), COR_AVISO));
        }
    });
    ui.label(pequeno(
        "Diâmetros típicos de catálogo; confira com o fabricante. O catálogo é gravado com as configurações ao clicar em OK.",
        t.text_dim,
    ));
}

fn lista_avisos(ui: &mut egui::Ui, id_salt: &str, avisos: &[String]) {
    if avisos.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(RichText::new(format!("{} aviso(s)", avisos.len())).color(COR_AVISO)).id_salt(id_salt).show(ui, |ui| {
        for a in avisos {
            ui.label(pequeno(a.as_str(), COR_AVISO));
        }
    });
}

fn titulo(ui: &mut egui::Ui, t: &Tokens, texto: &str) {
    ui.label(RichText::new(texto).strong().color(t.text));
}

fn secao_pontos(ui: &mut egui::Ui, t: &Tokens, a: &ele::Analise) {
    titulo(ui, t, "Pontos elétricos");
    if a.pontos.is_empty() {
        ui.label(pequeno("Nenhum bloco com atributos elétricos (TIPO_PONTO, POTENCIA, CIRCUITO...) no desenho.", t.text_dim));
        return;
    }
    let tipos = [
        TipoPonto::Quadro,
        TipoPonto::Iluminacao,
        TipoPonto::Tomada,
        TipoPonto::TomadaEspecifica,
        TipoPonto::Interruptor,
        TipoPonto::Caixa,
        TipoPonto::Outro,
    ];
    egui::Grid::new("ele-pontos").num_columns(2).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
        for tipo in tipos {
            let n = a.contar(tipo);
            if n > 0 {
                ui.label(tipo.nome());
                celula(ui, 40.0, numero(n.to_string()), true);
                ui.end_row();
            }
        }
    });
    let avisos: Vec<String> = a.avisos_pontos().into_iter().map(|(p, w)| format!("{p}: {w}")).collect();
    lista_avisos(ui, "ele-avisos-pontos", &avisos);
}

fn secao_circuitos(ui: &mut egui::Ui, t: &Tokens, a: &ele::Analise, selecionar: &mut Option<Vec<Handle>>) {
    titulo(ui, t, "Circuitos");
    if a.circuitos.is_empty() {
        ui.label(pequeno("Nenhum circuito: use Criar circuito ou o Quadro de cargas.", t.text_dim));
        return;
    }
    let tag_quadro = |h: Option<Handle>| h.and_then(|h| a.ponto(h)).map_or("?".to_string(), |q| q.rotulo());
    let colunas: [(&str, f32, bool); 6] =
        [("Quadro", 70.0, false), ("Circ.", 40.0, false), ("Tipo", 80.0, false), ("Pontos", 50.0, true), ("VA", 60.0, true), ("V", 40.0, true)];
    egui::Grid::new("ele-circuitos").num_columns(colunas.len()).striped(true).spacing(vec2(10.0, 2.0)).show(ui, |ui| {
        cabecalho(ui, t, &colunas);
        for c in &a.circuitos {
            celula(ui, 70.0, tag_quadro(c.quadro), false);
            let link = ui.allocate_ui_with_layout(vec2(40.0, LINHA), Layout::left_to_right(Align::Center), |ui| ui.link(c.id.as_str())).inner;
            if link.on_hover_text("Selecionar os pontos deste circuito").clicked() {
                *selecionar = Some(c.pontos.clone());
            }
            celula(ui, 80.0, c.natureza.nome(), false);
            celula(ui, 50.0, numero(c.pontos.len().to_string()), true);
            celula(ui, 60.0, numero(fixo(c.potencia, 0)), true);
            celula(ui, 40.0, numero(c.tensao.map_or_else(|| "?".to_string(), |v| fixo(v, 0))), true);
            ui.end_row();
        }
    });
    let avisos: Vec<String> = a.circuitos.iter().flat_map(|c| c.avisos.iter().map(move |w| format!("Circuito {}: {w}", c.id))).collect();
    lista_avisos(ui, "ele-avisos-circuitos", &avisos);
}
