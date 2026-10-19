// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Biblioteca de pontos: os blocos do desenho que são pontos elétricos (têm atributos elétricos
//! na definição), a inserção de um ponto com os valores escolhidos e a exclusão de pontos.

use std::sync::Arc;

use cadcraft_doc::geom::Vec3;
use cadcraft_doc::{Attrib, Block, Common, Drawing, EntityKind, Handle, Insert, Space};
use serde::Serialize;

use crate::ponto::{TAGS_ALTURA, TAGS_CIRCUITO, TAGS_COMANDO, TAGS_FASES, TAGS_POTENCIA, TAGS_QUADRO, TAGS_TENSAO, TAGS_TIPO, TipoPonto};

/// Camadas dos pontos inseridos (as do template).
pub const CAMADA_PONTOS: &str = "ELE-PONTOS";
pub const CAMADA_QUADROS: &str = "ELE-QUADROS";

/// Um atributo da definição do bloco.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtributoBloco {
    pub tag: String,
    pub padrao: String,
    pub prompt: String,
    /// Constante: o valor é da definição e não muda por inserção.
    pub constante: bool,
}

/// Um bloco de ponto elétrico disponível no desenho.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlocoPonto {
    pub nome: String,
    pub tipo: TipoPonto,
    /// Quantas vezes está inserido no espaço do modelo.
    pub inseridos: usize,
    pub atributos: Vec<AtributoBloco>,
}

fn definicoes(b: &Block) -> Vec<&Attrib> {
    b.entities.iter().filter_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a) } else { None }).collect()
}

/// Os blocos de ponto elétrico do desenho: os que têm algum atributo elétrico na definição
/// (o mesmo critério da análise). O tipo vem do TIPO_PONTO da definição ou do nome do bloco.
/// Em ordem de tipo e nome.
pub fn blocos(doc: &Drawing) -> Vec<BlocoPonto> {
    let eletricas = [TAGS_TIPO, TAGS_POTENCIA, TAGS_TENSAO, TAGS_FASES, TAGS_CIRCUITO, TAGS_COMANDO, TAGS_ALTURA, TAGS_QUADRO];
    let mut v: Vec<BlocoPonto> = doc
        .blocks
        .iter()
        .filter(|(nome, _)| !nome.starts_with('*') && !nome.to_uppercase().starts_with("FIACAO-"))
        .filter_map(|(nome, b)| {
            let defs = definicoes(b);
            let tem = |tags: &[&str]| defs.iter().find(|a| tags.iter().any(|t| a.tag.trim().eq_ignore_ascii_case(t)));
            if !eletricas.iter().any(|tags| tem(tags).is_some()) {
                return None;
            }
            let tipo = tem(TAGS_TIPO)
                .and_then(|a| TipoPonto::classificar(&a.text.value))
                .or_else(|| TipoPonto::classificar(nome))
                .unwrap_or(TipoPonto::Outro);
            let inseridos = doc.model.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(nome))).count();
            let atributos = defs
                .iter()
                .map(|a| AtributoBloco {
                    tag: a.tag.trim().to_string(),
                    padrao: a.text.value.clone(),
                    prompt: a.prompt.clone(),
                    constante: a.constant,
                })
                .collect();
            Some(BlocoPonto { nome: nome.clone(), tipo, inseridos, atributos })
        })
        .collect();
    v.sort_by(|a, b| a.tipo.cmp(&b.tipo).then_with(|| a.nome.to_uppercase().cmp(&b.nome.to_uppercase())));
    v
}

/// Insere o bloco de ponto `nome` em `pos`, girado `rotacao` (graus), com os atributos da
/// definição: os valores de `valores` (tag, valor) por cima dos padrões. Quadros vão para a
/// camada ELE-QUADROS, o resto para ELE-PONTOS.
pub fn inserir(doc: &mut Drawing, nome: &str, pos: [f64; 2], rotacao: f64, valores: &[(String, String)]) -> Result<Handle, String> {
    if !(pos.iter().all(|v| v.is_finite()) && rotacao.is_finite()) {
        return Err("posição ou rotação inválida".into());
    }
    let info = blocos(doc)
        .into_iter()
        .find(|b| b.nome.eq_ignore_ascii_case(nome.trim()))
        .ok_or_else(|| format!("o desenho não tem o bloco de ponto \"{}\"", nome.trim()))?;
    let bloco: Arc<Block> = doc.blocks.get(&info.nome).cloned().ok_or("bloco não encontrado")?;
    for (tag, _) in valores {
        if !info.atributos.iter().any(|a| a.tag.eq_ignore_ascii_case(tag.trim()) && !a.constante) {
            return Err(format!("o bloco \"{}\" não tem o atributo {} (ou ele é constante)", info.nome, tag.trim()));
        }
    }
    let mut ins = Insert {
        block: info.nome.clone(),
        insert: Vec3::new(pos[0], pos[1], 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: rotacao.to_radians(),
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let m = ins.transform(bloco.base.xy());
    ins.attribs = definicoes(&bloco)
        .into_iter()
        .filter(|ad| !ad.constant)
        .map(|ad| {
            let mut k = EntityKind::Text(ad.text.clone());
            k.transform(&m);
            let mut texto = if let EntityKind::Text(t) = k { t } else { ad.text.clone() };
            if let Some((_, v)) = valores.iter().find(|(t, _)| t.trim().eq_ignore_ascii_case(ad.tag.trim())) {
                texto.value = v.clone();
            }
            Attrib {
                tag: ad.tag.clone(),
                text: texto,
                invisible: ad.invisible,
                constant: false,
                prompt: String::new(),
                props: Default::default(),
                multiline: ad.multiline.as_ref().map(|ml| ml.transformed(&m)),
                modes: ad.modes.for_reference(),
            }
        })
        .collect();
    let camada = if info.tipo == TipoPonto::Quadro { CAMADA_QUADROS } else { CAMADA_PONTOS };
    doc.add(&Space::Model, Common { layer: camada.into(), ..Common::default() }, EntityKind::Insert(ins)).map_err(|e| e.to_string())
}

/// O que o próximo ELEPONTO insere: o bloco, a rotação (graus) e os valores dos atributos.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Modelo {
    pub bloco: String,
    pub rotacao: f64,
    pub valores: Vec<(String, String)>,
}

/// O modelo de cada desenho aberto (`uid` do desenho): outro desenho ou outra sessão no mesmo
/// processo não usa o modelo deste.
static MODELO: std::sync::Mutex<std::collections::BTreeMap<u64, Modelo>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Escolhe (ou limpa) o que o ELEPONTO insere no desenho `desenho` (a janela Inserir pontos
/// escolhe antes de começar o comando).
pub fn definir_modelo(desenho: u64, m: Option<Modelo>) {
    let mut g = MODELO.lock().unwrap_or_else(|e| e.into_inner());
    match m {
        Some(m) => {
            g.insert(desenho, m);
        }
        None => {
            g.remove(&desenho);
        }
    }
}

/// O modelo escolhido para o desenho `desenho` (fica para as próximas vezes).
pub fn modelo(desenho: u64) -> Option<Modelo> {
    MODELO.lock().unwrap_or_else(|e| e.into_inner()).get(&desenho).cloned()
}

/// Exclui pontos elétricos do desenho. Recusa (sem excluir nada) se algum handle não for um
/// ponto elétrico. Devolve quantos saíram.
pub fn excluir(doc: &mut Drawing, pontos: &[Handle]) -> Result<usize, String> {
    for h in pontos {
        if crate::ponto::ler_ponto(doc, *h).is_none() {
            return Err(format!("o objeto {} não é um ponto elétrico", h.hex()));
        }
    }
    Ok(pontos.iter().filter(|h| doc.model.remove(**h).is_some()).count())
}
