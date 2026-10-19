// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Ajustar fiação: quais condutores de cada circuito passam em cada trecho de eletroduto, e as
//! marcas de fiação desenhadas sobre os trechos.
//!
//! Regras desta versão, por circuito, sobre os caminhos do roteamento:
//! - fase: do quadro às tomadas, aos interruptores e às luminárias sem interruptor;
//! - neutro: do quadro às cargas (não existe em circuito de duas fases sem neutro, 2F+T);
//! - proteção: do quadro às cargas;
//! - retorno: de cada interruptor às luminárias com a letra dele (`COMANDO`), uma por letra.
//!
//! As marcas são blocos (`FIACAO-FASE`, `FIACAO-NEUTRO`, `FIACAO-RETORNO`, `FIACAO-PROTECAO`)
//! perpendiculares ao trecho, agrupadas por circuito: o número do circuito em cima do grupo, a
//! letra de cada retorno em cima do retorno, a seção (quando não omitida) embaixo do grupo e o
//! diâmetro do eletroduto do trecho embaixo, no fim. O eletroduto é o menor do tipo do projeto
//! que comporta os condutores do trecho com a ocupação máxima da NBR 5410 (6.2.11.1.6).
//!
//! Os blocos são criados com um desenho padrão quando o desenho ainda não os tem; o projetista
//! pode redesenhá-los, e Ajustar fiação usa os dele. Tudo o que é gerado fica na camada
//! `ELE-FIACAO` (cor 7: branco no fundo escuro, preto na impressão) com o XDATA
//! `FIACAO=<handle do quadro>`, e Ajustar fiação apaga e refaz as marcas daquele quadro.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use cadcraft_doc::color::Color;
use cadcraft_doc::geom::Vec3;
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, HAlign, Handle, Insert, Line, Lineweight, RawTag, Space, Text, VAlign};
use serde::Serialize;

use crate::Analise;
use crate::nbr5410::Isolacao;
use crate::ponto::{PontoEletrico, TipoPonto, br};
use crate::quadro;
use crate::rota::Rede;
use crate::setup::{Setup, TamanhoEletroduto};
use crate::xdata;

/// Camada das marcas de fiação.
pub const CAMADA: &str = "ELE-FIACAO";
/// Chave do XDATA que marca o que Ajustar fiação gerou: `FIACAO=<handle do quadro>`.
pub const CHAVE: &str = "FIACAO";

/// XDATA das linhas dos blocos de marca criados pelo Free Elétrica: `FIACAO_BLOCO=<versão>`. Um
/// bloco de versão anterior é trocado pelo desenho atual; um sem a marca (redesenhado pelo
/// projetista) fica.
const CHAVE_BLOCO: &str = "FIACAO_BLOCO";
const VERSAO_BLOCOS: &str = "2";

/// O bloco no desenho é um desenho padrão de versão anterior do Free Elétrica.
fn bloco_padrao_antigo(b: &Block, tipo: TipoCondutor, m: f64) -> bool {
    let versoes: Vec<String> =
        b.entities.iter().filter_map(|e| xdata::ler(&e.common.xdata).into_iter().find(|(k, _)| k == CHAVE_BLOCO).map(|(_, v)| v)).collect();
    if !versoes.is_empty() {
        return versoes.iter().any(|v| v != VERSAO_BLOCOS);
    }
    // Sem marca: os blocos da primeira versão, reconhecidos pela geometria.
    let linhas: Vec<((f64, f64), (f64, f64))> = b
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some(((l.a.x * m, l.a.y * m), (l.b.x * m, l.b.y * m))),
            _ => None,
        })
        .collect();
    let perto = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
    linhas.len() == b.entities.len()
        && tipo
            .tracos_antigos()
            .iter()
            .any(|antigo| antigo.len() == linhas.len() && antigo.iter().zip(&linhas).all(|(x, y)| perto(x.0, y.0) && perto(x.1, y.1)))
}

/// Distância entre marcas no trecho e altura dos textos, em metros.
const PASSO: f64 = 0.04;
const ENTRE_CIRCUITOS: f64 = 0.05;
pub const ALTURA_TEXTO: f64 = 0.05;
/// O diâmetro do eletroduto é escrito menor que o circuito e a seção.
pub const ALTURA_DIAMETRO: f64 = 0.035;
/// Folga entre as marcas e os textos, e entre a seção e o diâmetro.
const FOLGA: f64 = 0.015;
/// Meia altura das marcas: elas vão de -0,05 a 0,05 m em torno do eletroduto.
const MEIA_MARCA: f64 = 0.05;
/// Centro dos textos de cima (circuito) e de baixo (seção), a partir do eixo do eletroduto.
const AFASTAMENTO_TEXTO: f64 = MEIA_MARCA + FOLGA + ALTURA_TEXTO / 2.0;
/// Centro do diâmetro: abaixo da seção, com folga.
const AFASTAMENTO_DIAMETRO: f64 = AFASTAMENTO_TEXTO + ALTURA_TEXTO / 2.0 + FOLGA + ALTURA_DIAMETRO / 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TipoCondutor {
    Fase,
    Neutro,
    Retorno,
    Protecao,
}

impl TipoCondutor {
    pub const TODOS: [TipoCondutor; 4] = [Self::Fase, Self::Neutro, Self::Retorno, Self::Protecao];

    /// O bloco da marca.
    pub fn bloco(self) -> &'static str {
        match self {
            Self::Fase => "FIACAO-FASE",
            Self::Neutro => "FIACAO-NEUTRO",
            Self::Retorno => "FIACAO-RETORNO",
            Self::Protecao => "FIACAO-PROTECAO",
        }
    }

    /// O desenho padrão da marca, em metros, com o eletroduto no eixo x: fase, traço que
    /// atravessa o eletroduto; neutro, o traço com uma bandeira para a esquerda no topo;
    /// retorno, um traço curto só acima do eletroduto; terra (proteção), o traço com uma barra
    /// no topo, em T.
    fn tracos(self) -> Vec<((f64, f64), (f64, f64))> {
        const H: f64 = 0.05;
        let haste = ((0.0, -H), (0.0, H));
        match self {
            Self::Fase => vec![haste],
            Self::Neutro => vec![haste, ((-0.02, H), (0.0, H))],
            Self::Retorno => vec![((0.0, 0.0), (0.0, H * 0.6))],
            Self::Protecao => vec![haste, ((-0.02, H), (0.02, H))],
        }
    }

    /// Os desenhos padrão das versões anteriores, para trocar por este quando o bloco no
    /// desenho ainda é um deles.
    fn tracos_antigos(self) -> Vec<Vec<((f64, f64), (f64, f64))>> {
        const H: f64 = 0.05;
        let haste = ((0.0, -H), (0.0, H));
        match self {
            Self::Fase => vec![vec![haste, ((-0.015, H - 0.015), (0.0, H))]],
            Self::Neutro => vec![vec![((0.0, 0.01), (0.0, H))]],
            Self::Retorno => vec![vec![haste, ((-0.025, H), (0.0, H))]],
            Self::Protecao => vec![],
        }
    }
}

/// Os condutores de um circuito num trecho.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Condutores {
    pub circuito: String,
    /// Fases (0 a 3).
    pub fases: u8,
    pub neutro: bool,
    pub protecao: bool,
    /// Letras dos retornos.
    pub retornos: BTreeSet<String>,
    /// Seção do circuito (mm²), a escolhida no quadro de cargas.
    pub secao: Option<f64>,
    /// Isolação dos condutores do circuito (entra no diâmetro externo).
    pub isolacao: Isolacao,
}

impl Condutores {
    /// As marcas, na ordem em que são desenhadas: fases, neutro, retornos, proteção.
    pub fn marcas(&self) -> Vec<(TipoCondutor, Option<&str>)> {
        let mut v: Vec<(TipoCondutor, Option<&str>)> = (0..self.fases).map(|_| (TipoCondutor::Fase, None)).collect();
        if self.neutro {
            v.push((TipoCondutor::Neutro, None));
        }
        v.extend(self.retornos.iter().map(|l| (TipoCondutor::Retorno, Some(l.as_str()))));
        if self.protecao {
            v.push((TipoCondutor::Protecao, None));
        }
        v
    }

    fn vazio(&self) -> bool {
        self.fases == 0 && !self.neutro && !self.protecao && self.retornos.is_empty()
    }
}

/// Um trecho de eletroduto com os condutores que passam nele.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trecho {
    /// Pontas, nas coordenadas do desenho.
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub piso: bool,
    pub circuitos: Vec<Condutores>,
    /// O eletroduto do trecho, pela ocupação. `None`: nenhum tamanho do tipo comporta (ou falta
    /// o diâmetro de alguma seção nas configurações).
    pub eletroduto: Option<TamanhoEletroduto>,
    /// Ocupação da seção interna do eletroduto escolhido (0 a 1).
    pub ocupacao: Option<f64>,
}

impl Trecho {
    /// Quantos condutores passam no trecho.
    pub fn condutores(&self) -> usize {
        self.circuitos.iter().map(|k| k.marcas().len()).sum()
    }
}

/// Escolhe o eletroduto do trecho pela ocupação dos condutores.
fn dimensionar_eletroduto(t: &mut Trecho, setup: &Setup, avisos: &mut Vec<String>) {
    let mut diametros = Vec::new();
    for k in &t.circuitos {
        let Some(secao) = k.secao else {
            avisos.push(format!("circuito {}: sem seção, eletroduto não dimensionado", k.circuito));
            return;
        };
        let Some(d) = setup.diametro_condutor(secao, k.isolacao) else {
            avisos.push(format!("sem diâmetro externo para {} mm² ({}) nas configurações", br(secao), k.isolacao.codigo()));
            return;
        };
        diametros.extend(std::iter::repeat_n(d, k.marcas().len()));
    }
    match setup.eletroduto_para(&diametros) {
        Some((e, oc)) => {
            t.eletroduto = Some(e);
            t.ocupacao = Some(oc);
        }
        None => avisos.push(format!("{} condutores num trecho: nenhum eletroduto do tipo do projeto comporta", diametros.len())),
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fiacao {
    #[serde(serialize_with = "hex")]
    pub quadro: Handle,
    pub tag: String,
    /// Isolação dos condutores do quadro (entra no diâmetro externo).
    pub isolacao: Isolacao,
    pub trechos: Vec<Trecho>,
    /// Metros por unidade do desenho.
    pub metros_por_unidade: f64,
    pub avisos: Vec<String>,
}

fn hex<S: serde::Serializer>(h: &Handle, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&h.hex())
}

/// As letras de um COMANDO: "a", "ab", "a,b" ou "a b".
fn letras(comando: &str) -> Vec<String> {
    comando.chars().filter(|c| c.is_alphabetic()).map(|c| c.to_lowercase().to_string()).collect()
}

/// Calcula os condutores em cada trecho dos circuitos do quadro.
pub fn calcular(doc: &Drawing, analise: &Analise, setup: &Setup, quadro: Handle) -> Result<Fiacao, String> {
    let resumo = quadro::rotear(doc, analise, setup, quadro)?;
    let q = analise.ponto(quadro).ok_or("quadro não encontrado")?;
    let rede = Rede::montar(doc, setup, &analise.pontos, resumo.metros_por_unidade);
    let lig_q = rede.ligacoes.get(&quadro).ok_or("o quadro não encosta em nenhum eletroduto (raio de captura): não há onde marcar a fiação")?;
    let arvore_q = rede.arvore(lig_q.no);
    let mut avisos = Vec::new();

    // aresta -> circuito -> condutores
    let mut por_aresta: BTreeMap<usize, BTreeMap<usize, Condutores>> = BTreeMap::new();
    for (ic, linha) in resumo.linhas.iter().enumerate() {
        let c = &linha.circuito;
        let membros: Vec<&PontoEletrico> = analise.pontos.iter().filter(|p| c.pontos.contains(&p.handle)).collect();
        let novo = || Condutores { circuito: c.id.clone(), secao: linha.escolha.secao, isolacao: linha.isolacao, ..Condutores::default() };
        let mut marcar = |arestas: &[usize], f: &dyn Fn(&mut Condutores)| {
            for e in arestas {
                f(por_aresta.entry(*e).or_default().entry(ic).or_insert_with(novo));
            }
        };
        let caminho_do_quadro = |p: &PontoEletrico| rede.ligacoes.get(&p.handle).and_then(|l| arvore_q.caminho(l.no));
        let interruptores: Vec<&&PontoEletrico> = membros.iter().filter(|p| p.tipo == TipoPonto::Interruptor).collect();
        let letras_com_interruptor: BTreeSet<String> = interruptores.iter().filter_map(|s| s.comando.as_deref()).flat_map(letras).collect();
        let fases = c.fases.max(1);
        for p in &membros {
            if matches!(p.tipo, TipoPonto::Caixa | TipoPonto::Quadro) {
                continue;
            }
            let Some(caminho) = caminho_do_quadro(p) else {
                avisos.push(format!("{}: fora da rede de eletrodutos, fiação não marcada", p.rotulo()));
                continue;
            };
            let comandada = p.tipo == TipoPonto::Iluminacao
                && fases < 2
                && p.comando.as_deref().is_some_and(|cmd| letras(cmd).iter().any(|l| letras_com_interruptor.contains(l)));
            match p.tipo {
                TipoPonto::Interruptor => marcar(&caminho, &|k: &mut Condutores| k.fases = k.fases.max(1)),
                _ if comandada => marcar(&caminho, &|k: &mut Condutores| {
                    k.neutro = true;
                    k.protecao = true;
                }),
                _ => marcar(&caminho, &|k: &mut Condutores| {
                    k.fases = k.fases.max(fases);
                    k.neutro |= fases != 2;
                    k.protecao = true;
                }),
            }
        }
        // Retornos: do interruptor a cada luminária com a letra dele.
        for s in &interruptores {
            let Some(lig_s) = rede.ligacoes.get(&s.handle) else { continue };
            let arvore_s = rede.arvore(lig_s.no);
            for letra in s.comando.as_deref().map(letras).unwrap_or_default() {
                for lum in
                    membros.iter().filter(|p| p.tipo == TipoPonto::Iluminacao && p.comando.as_deref().is_some_and(|cmd| letras(cmd).contains(&letra)))
                {
                    let caminho = rede.ligacoes.get(&lum.handle).and_then(|l| arvore_s.caminho(l.no));
                    match caminho {
                        Some(cam) => marcar(&cam, &|k: &mut Condutores| {
                            k.retornos.insert(letra.clone());
                        }),
                        None => avisos.push(format!("{}: sem caminho de eletroduto até o interruptor do comando {letra}", lum.rotulo())),
                    }
                }
            }
        }
    }

    let trechos = por_aresta
        .into_iter()
        .filter_map(|(e, circuitos)| {
            // Numa curva, só o pedaço do meio recebe as marcas.
            if let Some((_, pedaco, pedacos)) = rede.curvas.get(&e)
                && *pedaco != pedacos / 2
            {
                return None;
            }
            let (na, nb, _) = *rede.arestas.get(e)?;
            let (a, b) = (*rede.nos.get(na)?, *rede.nos.get(nb)?);
            // Subidas entre piso e teto não têm desenho para marcar.
            if (a[0] - b[0]).hypot(a[1] - b[1]) <= 0.0 {
                return None;
            }
            let piso = rede.elevacao.get(na).is_some_and(|z| *z < setup.pe_direito);
            let circuitos: Vec<Condutores> = circuitos.into_values().filter(|k| !k.vazio()).collect();
            (!circuitos.is_empty()).then_some(Trecho { a, b, piso, circuitos, eletroduto: None, ocupacao: None })
        })
        .collect::<Vec<_>>();
    let mut trechos = trechos;
    for t in &mut trechos {
        dimensionar_eletroduto(t, setup, &mut avisos);
    }
    avisos.sort();
    avisos.dedup();
    Ok(Fiacao { quadro, tag: q.rotulo(), isolacao: resumo.isolacao, trechos, metros_por_unidade: resumo.metros_por_unidade, avisos })
}

// ---------------------------------------------------------------- desenho

fn xdata_fiacao(quadro: Handle) -> Vec<RawTag> {
    let mut x = Vec::new();
    xdata::gravar(&mut x, CHAVE, Some(&quadro.hex()));
    x
}

fn comum(quadro: Handle) -> Common {
    Common { layer: CAMADA.into(), xdata: xdata_fiacao(quadro), ..Common::default() }
}

/// O que Ajustar fiação gerou para o quadro.
pub fn gerados(doc: &Drawing, quadro: Handle) -> Vec<Handle> {
    let alvo = quadro.hex();
    doc.model
        .iter()
        .filter(|e| xdata::ler(&e.common.xdata).iter().any(|(k, v)| k.eq_ignore_ascii_case(CHAVE) && v.eq_ignore_ascii_case(&alvo)))
        .map(|e| e.handle)
        .collect()
}

/// Apaga as marcas de fiação do quadro. Devolve quantas apagou.
pub fn limpar(doc: &mut Drawing, quadro: Handle) -> usize {
    let hs = gerados(doc, quadro);
    hs.iter().filter(|h| doc.model.remove(**h).is_some()).count()
}

/// Cria os blocos das marcas que o desenho ainda não tem, com o desenho padrão
/// ([`TipoCondutor`]), e a camada `ELE-FIACAO` (cor 7, traço 0,35 mm). `m` = metros por unidade.
pub fn garantir_blocos(doc: &mut Drawing, m: f64) {
    let u = |v: f64| v / m;
    for tipo in TipoCondutor::TODOS {
        if let Some(b) = doc.blocks.get(tipo.bloco())
            && !bloco_padrao_antigo(b, tipo, m)
        {
            // O desenho atual, ou um bloco redesenhado pelo projetista: fica.
            continue;
        }
        let mut b = Block::new(tipo.bloco());
        for (a, c) in tipo.tracos() {
            let h = doc.new_handle();
            let linha = Line { a: Vec3::new(u(a.0), u(a.1), 0.0), b: Vec3::new(u(c.0), u(c.1), 0.0) };
            let mut e = Entity::new(h, EntityKind::Line(linha));
            xdata::gravar(&mut e.common.xdata, CHAVE_BLOCO, Some(VERSAO_BLOCOS));
            b.entities.push(e);
        }
        doc.blocks.insert(tipo.bloco().to_string(), Arc::new(b));
    }
    let nova = doc.ensure_layer(CAMADA);
    if let Some(l) = doc.layer_mut(CAMADA) {
        // Camada nova, ou a das versões antigas (vermelha): cor 7 e traço mais grosso.
        if nova || l.color == Color::Index(1) {
            l.color = Color::Index(7);
            l.lineweight = Lineweight::Mm100(35);
        }
    }
}

fn texto(valor: String, em: [f64; 2], altura: f64, rotacao: f64) -> EntityKind {
    let p = Vec3::new(em[0], em[1], 0.0);
    EntityKind::Text(Text {
        insert: p,
        align_pt: Some(p),
        height: altura,
        value: valor,
        rotation: rotacao,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Center,
        valign: VAlign::Middle,
    })
}

/// Largura estimada de um texto (m) na fonte Standard: 0,8 da altura por caractere (`%%c`, o
/// símbolo de diâmetro, conta como um).
pub fn largura_texto(texto: &str, altura: f64) -> f64 {
    texto.replace("%%c", "Ø").chars().count() as f64 * 0.8 * altura
}

/// O que um grupo de marcas ocupa ao longo do trecho (m), a partir da primeira marca: as marcas,
/// o número do circuito sobre a primeira, as letras sobre os retornos e a seção centrada embaixo.
fn faixa_do_grupo(k: &Condutores, grupo: &[(TipoCondutor, Option<&str>)], setup: &Setup) -> (f64, f64) {
    let vao = grupo.len().saturating_sub(1) as f64 * PASSO;
    let (mut lo, mut hi) = (0.0f64, vao);
    let mut cobrir = |centro: f64, texto: &str| {
        let meia = largura_texto(texto, ALTURA_TEXTO) / 2.0;
        lo = lo.min(centro - meia);
        hi = hi.max(centro + meia);
    };
    for (i, (_, letra)) in grupo.iter().enumerate() {
        let x = i as f64 * PASSO;
        match (i, letra) {
            (0, Some(l)) => cobrir(x, &format!("{}{l}", k.circuito)),
            (0, None) => cobrir(x, &k.circuito),
            (_, Some(l)) => cobrir(x, l),
            (_, None) => {}
        }
    }
    if let Some(x) = k.secao.filter(|x| !setup.omitida(*x)) {
        cobrir(vao / 2.0, &format!("#{}", br(x)));
    }
    (lo, hi)
}

/// Desenha as marcas da fiação (apagando as anteriores do mesmo quadro). Devolve quantas
/// entidades criou.
pub fn aplicar(doc: &mut Drawing, f: &Fiacao, setup: &Setup) -> Result<usize, String> {
    let m = if f.metros_por_unidade > 0.0 { f.metros_por_unidade } else { 1.0 };
    limpar(doc, f.quadro);
    garantir_blocos(doc, m);
    let mut criadas = 0usize;
    for t in &f.trechos {
        // Sempre da esquerda para a direita (ou de baixo para cima), para o texto ler direito.
        let (a, b) = if t.b[0] < t.a[0] || (t.b[0] == t.a[0] && t.b[1] < t.a[1]) { (t.b, t.a) } else { (t.a, t.b) };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let comp = dx.hypot(dy);
        if comp <= 0.0 {
            continue;
        }
        let (d, n) = ([dx / comp, dy / comp], [-dy / comp, dx / comp]);
        let rot = dy.atan2(dx);
        let meio = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let (passo, entre) = (PASSO / m, ENTRE_CIRCUITOS / m);
        let grupos: Vec<Vec<(TipoCondutor, Option<&str>)>> = t.circuitos.iter().map(Condutores::marcas).collect();
        // Cada grupo ocupa a largura das marcas ou dos textos dele (o que for maior), para os
        // textos de circuitos vizinhos não ficarem um sobre o outro.
        let faixas: Vec<(f64, f64)> = t.circuitos.iter().zip(&grupos).map(|(k, g)| faixa_do_grupo(k, g, setup)).collect();
        let largura: f64 = faixas.iter().map(|(lo, hi)| (hi - lo) / m).sum::<f64>() + faixas.len().saturating_sub(1) as f64 * entre;
        let mut borda = -largura / 2.0;
        let acima = AFASTAMENTO_TEXTO / m;
        let ponto = |s: f64, desvio: f64| [meio[0] + d[0] * s + n[0] * desvio, meio[1] + d[1] * s + n[1] * desvio];
        for ((k, grupo), (lo, hi)) in t.circuitos.iter().zip(&grupos).zip(&faixas) {
            // A primeira marca fica a `-lo` da borda esquerda do grupo.
            let mut s = borda - lo / m;
            borda += (hi - lo) / m + entre;
            let inicio = s;
            for (i, (tipo, letra)) in grupo.iter().enumerate() {
                let p = ponto(s, 0.0);
                let ins = Insert {
                    block: tipo.bloco().into(),
                    insert: Vec3::new(p[0], p[1], 0.0),
                    scale: Vec3::new(1.0, 1.0, 1.0),
                    rotation: rot,
                    attribs: Vec::new(),
                    cols: 1,
                    rows: 1,
                    col_spacing: 0.0,
                    row_spacing: 0.0,
                };
                doc.add(&Space::Model, comum(f.quadro), EntityKind::Insert(ins)).map_err(|e| e.to_string())?;
                criadas += 1;
                // Em cima: o número do circuito sobre a primeira marca e a letra sobre cada
                // retorno (lado a lado leem "1b").
                let rotulo = match (i, letra) {
                    (0, Some(l)) => Some(format!("{}{l}", k.circuito)),
                    (0, None) => Some(k.circuito.clone()),
                    (_, Some(l)) => Some(l.to_string()),
                    (_, None) => None,
                };
                if let Some(r) = rotulo {
                    doc.add(&Space::Model, comum(f.quadro), texto(r, ponto(s, acima), ALTURA_TEXTO / m, rot)).map_err(|e| e.to_string())?;
                    criadas += 1;
                }
                s += passo;
            }
            // Embaixo do grupo: a seção, quando não omitida.
            if let Some(x) = k.secao.filter(|x| !setup.omitida(*x)) {
                let centro = (inicio + s - passo) / 2.0;
                doc.add(&Space::Model, comum(f.quadro), texto(format!("#{}", br(x)), ponto(centro, -acima), ALTURA_TEXTO / m, rot))
                    .map_err(|e| e.to_string())?;
                criadas += 1;
            }
        }
        // Na linha de baixo, centrado sob toda a simbologia: o diâmetro do eletroduto do trecho.
        let diametro = t.eletroduto.as_ref().map_or_else(|| "%%c?".to_string(), |e| format!("%%c{}", br(e.nominal)));
        let abaixo = AFASTAMENTO_DIAMETRO / m;
        doc.add(&Space::Model, comum(f.quadro), texto(diametro, ponto(0.0, -abaixo), ALTURA_DIAMETRO / m, rot)).map_err(|e| e.to_string())?;
        criadas += 1;
    }
    Ok(criadas)
}

/// Resumo por trecho para o JSON dos comandos: quantas marcas de cada tipo.
pub fn contagem(f: &Fiacao) -> HashMap<&'static str, usize> {
    let mut c: HashMap<&'static str, usize> = HashMap::new();
    for t in &f.trechos {
        for k in &t.circuitos {
            for (tipo, _) in k.marcas() {
                *c.entry(tipo.bloco()).or_default() += 1;
            }
        }
    }
    c
}
