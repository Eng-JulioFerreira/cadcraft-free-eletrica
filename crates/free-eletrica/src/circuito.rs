// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Circuitos: os pontos agrupados pelo atributo CIRCUITO.

use cadcraft_doc::Handle;
use serde::Serialize;

use crate::ponto::{PontoEletrico, TipoPonto};

/// O que o circuito alimenta, pelos tipos dos seus pontos de carga.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Natureza {
    Iluminacao,
    Tug,
    Tue,
    /// Iluminação e tomadas no mesmo circuito.
    Misto,
    /// Só interruptores ou caixas: nenhuma carga.
    SemCarga,
}

impl Natureza {
    pub fn nome(self) -> &'static str {
        match self {
            Self::Iluminacao => "Iluminação",
            Self::Tug => "TUG",
            Self::Tue => "TUE",
            Self::Misto => "Misto",
            Self::SemCarga => "Sem carga",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Circuito {
    pub id: String,
    /// O quadro que alimenta o circuito. `None`: os pontos não dizem e há mais de um quadro.
    #[serde(serialize_with = "crate::ponto::handle_hex_opcional")]
    pub quadro: Option<Handle>,
    #[serde(skip)]
    pub pontos: Vec<Handle>,
    pub natureza: Natureza,
    /// Soma das potências dos pontos de carga, em VA.
    pub potencia: f64,
    /// Tensão dos pontos (a maior, se divergirem). `None`: nenhum ponto informou.
    pub tensao: Option<f64>,
    /// Fases do circuito: a maior entre os pontos, 1 se nenhum informou.
    pub fases: u8,
    pub cargas: usize,
    pub interruptores: usize,
    pub avisos: Vec<String>,
}

/// Agrupa os pontos por quadro e circuito: o circuito 1 de um quadro não é o do outro. Dentro
/// de cada quadro, em ordem natural ("2" antes de "10").
pub fn agrupar(pontos: &[PontoEletrico]) -> Vec<Circuito> {
    let mut chaves: Vec<(Option<Handle>, &str)> =
        pontos.iter().filter(|p| p.tipo != TipoPonto::Quadro).filter_map(|p| p.circuito.as_deref().map(|c| (p.quadro, c))).collect();
    chaves.sort_by(|a, b| a.0.map(|h| h.0).cmp(&b.0.map(|h| h.0)).then_with(|| ordem_natural(a.1, b.1)));
    chaves.dedup_by(|a, b| a.0 == b.0 && a.1.eq_ignore_ascii_case(b.1));
    chaves
        .into_iter()
        .map(|(quadro, id)| {
            let membros: Vec<&PontoEletrico> = pontos
                .iter()
                .filter(|p| p.tipo != TipoPonto::Quadro && p.quadro == quadro && p.circuito.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(id)))
                .collect();
            let mut c = montar(id, &membros);
            c.quadro = quadro;
            if quadro.is_none() {
                c.avisos.push("circuito sem quadro: use Criar circuito ou o atributo QUADRO com a TAG do quadro".into());
            }
            c
        })
        .collect()
}

/// A próxima letra de comando livre num circuito: "a", "b"... (as letras dos interruptores e
/// luminárias dele).
pub fn proxima_letra(pontos: &[PontoEletrico], quadro: Option<Handle>, circuito: Option<&str>) -> String {
    let usadas: Vec<String> = pontos
        .iter()
        .filter(|p| p.quadro == quadro && circuito.is_some_and(|c| p.circuito.as_deref().is_some_and(|pc| pc.eq_ignore_ascii_case(c))))
        .filter_map(|p| p.comando.as_ref().map(|c| c.to_ascii_lowercase()))
        .collect();
    ('a'..='z').map(|l| l.to_string()).find(|l| !usadas.contains(l)).unwrap_or_else(|| "a".into())
}

/// Quadro e circuito já escolhidos para o próximo ELECIRCUITO (o quadro de cargas usa para
/// "adicionar pontos" sem perguntar de novo), por desenho (`uid` do desenho aberto): outro
/// desenho ou outra sessão no mesmo processo não toma o alvo deste. Consumido pelo comando.
static ALVO: std::sync::Mutex<std::collections::BTreeMap<u64, (Handle, String)>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Escolhe (ou limpa) o quadro e o circuito do próximo ELECIRCUITO no desenho `desenho`.
pub fn definir_alvo(desenho: u64, alvo: Option<(Handle, String)>) {
    let mut g = ALVO.lock().unwrap_or_else(|e| e.into_inner());
    match alvo {
        Some(a) => {
            g.insert(desenho, a);
        }
        None => {
            g.remove(&desenho);
        }
    }
}

/// Toma o alvo escolhido para o desenho `desenho` (e o limpa).
pub fn tomar_alvo(desenho: u64) -> Option<(Handle, String)> {
    ALVO.lock().unwrap_or_else(|e| e.into_inner()).remove(&desenho)
}

/// O próximo número de circuito livre no quadro: o maior número usado mais um.
pub fn proximo_circuito(pontos: &[PontoEletrico], quadro: Handle) -> String {
    let dos_pontos = pontos.iter().filter(|p| p.quadro == Some(quadro)).filter_map(|p| p.circuito.as_deref()?.trim().parse::<u32>().ok());
    // Os criados no quadro de cargas (descritos), mesmo sem pontos.
    let criados: Vec<u32> = pontos
        .iter()
        .filter(|p| p.handle == quadro)
        .flat_map(crate::quadro::descricoes)
        .filter_map(|(id, _)| id.trim().parse::<u32>().ok())
        .collect();
    let maior = dos_pontos.chain(criados).max().unwrap_or(0);
    maior.saturating_add(1).to_string()
}

fn montar(id: &str, membros: &[&PontoEletrico]) -> Circuito {
    let cargas: Vec<&&PontoEletrico> = membros.iter().filter(|p| p.tipo.e_carga()).collect();
    let tem = |t: TipoPonto| cargas.iter().any(|p| p.tipo == t);
    let (ilum, tug, tue) = (tem(TipoPonto::Iluminacao), tem(TipoPonto::Tomada), tem(TipoPonto::TomadaEspecifica));
    let natureza = match (ilum, tug || tem(TipoPonto::Outro), tue) {
        (false, false, false) => Natureza::SemCarga,
        (true, false, false) => Natureza::Iluminacao,
        (false, true, false) => Natureza::Tug,
        (false, false, true) => Natureza::Tue,
        _ => Natureza::Misto,
    };
    let mut avisos = Vec::new();
    let tensoes: Vec<f64> = cargas.iter().filter_map(|p| p.tensao).collect();
    let tensao = tensoes.iter().copied().reduce(f64::max);
    if tensoes.iter().any(|t| Some(*t) != tensao) {
        avisos.push("pontos com tensões diferentes no mesmo circuito".into());
    }
    if natureza == Natureza::Misto {
        avisos.push("iluminação e tomadas no mesmo circuito: a NBR 5410 (9.5.3.2) pede circuitos separados, salvo as exceções da norma".into());
    }
    if natureza == Natureza::Tue && cargas.len() > 1 {
        avisos.push("TUE com mais de um ponto: equipamento acima de 10 A deve ter circuito próprio (NBR 5410, 9.5.3.3)".into());
    }
    let sem_potencia = cargas.iter().filter(|p| p.potencia.is_none()).count();
    if sem_potencia > 0 {
        avisos.push(format!("{sem_potencia} ponto(s) de carga sem POTENCIA, contados como 0 VA"));
    }
    Circuito {
        id: id.to_string(),
        quadro: None,
        pontos: membros.iter().map(|p| p.handle).collect(),
        natureza,
        potencia: cargas.iter().filter_map(|p| p.potencia).sum(),
        tensao,
        fases: membros.iter().filter_map(|p| p.fases).max().unwrap_or(1),
        cargas: cargas.len(),
        interruptores: membros.iter().filter(|p| p.tipo == TipoPonto::Interruptor).count(),
        avisos,
    }
}

/// Números pelo valor, depois o resto em ordem alfabética.
pub fn ordem_natural(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        _ => a.to_uppercase().cmp(&b.to_uppercase()),
    }
}
