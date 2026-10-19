// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Dimensionamento de circuitos terminais pela NBR 5410.
//!
//! O que está aqui: corrente de projeto, seção mínima (tabela 47), capacidade de condução
//! (cobre, método de referência B1: tabela 36 para PVC 70 °C, tabela 37 para EPR e XLPE 90 °C),
//! fatores de correção de temperatura (tabela 40, por isolação) e de agrupamento (tabela 42,
//! pelo roteamento), queda de tensão pelo momento elétrico (6.2.7) e a escolha do disjuntor com
//! Ib ≤ In ≤ Iz (5.3.4.1).
//!
//! Ainda não: outros métodos de instalação, seções acima de 120 mm², DR e coordenação com
//! curto-circuito. Os valores das tabelas devem ser conferidos contra a norma vigente.

use serde::{Deserialize, Serialize};

use crate::circuito::{Circuito, Natureza};

/// Seções nominais de condutor, em mm².
pub const SECOES: [f64; 12] = [1.5, 2.5, 4.0, 6.0, 10.0, 16.0, 25.0, 35.0, 50.0, 70.0, 95.0, 120.0];

/// Tabela 36, método B1, cobre, PVC, 30 °C: dois condutores carregados (A).
const IZ_PVC_DOIS: [f64; 12] = [17.5, 24.0, 32.0, 41.0, 57.0, 76.0, 101.0, 125.0, 151.0, 192.0, 232.0, 269.0];
/// Tabela 36, método B1, cobre, PVC, 30 °C: três condutores carregados (A).
const IZ_PVC_TRES: [f64; 12] = [15.5, 21.0, 28.0, 36.0, 50.0, 68.0, 89.0, 110.0, 134.0, 171.0, 207.0, 239.0];
/// Tabela 37, método B1, cobre, EPR ou XLPE, 30 °C: dois condutores carregados (A).
const IZ_EPR_DOIS: [f64; 12] = [23.0, 31.0, 42.0, 54.0, 75.0, 100.0, 133.0, 164.0, 198.0, 253.0, 306.0, 354.0];
/// Tabela 37, método B1, cobre, EPR ou XLPE, 30 °C: três condutores carregados (A).
const IZ_EPR_TRES: [f64; 12] = [20.0, 28.0, 37.0, 48.0, 66.0, 88.0, 117.0, 144.0, 175.0, 222.0, 269.0, 312.0];

/// Tabela 40, temperatura ambiente (não no solo): (°C, fator PVC, fator EPR/XLPE).
const FCT_AMBIENTE: [(f64, f64, f64); 11] = [
    (10.0, 1.22, 1.15),
    (15.0, 1.17, 1.12),
    (20.0, 1.12, 1.08),
    (25.0, 1.06, 1.04),
    (30.0, 1.00, 1.00),
    (35.0, 0.94, 0.96),
    (40.0, 0.87, 0.91),
    (45.0, 0.79, 0.87),
    (50.0, 0.71, 0.82),
    (55.0, 0.61, 0.76),
    (60.0, 0.50, 0.71),
];

/// Correntes nominais de disjuntores termomagnéticos (A).
pub const DISJUNTORES: [f64; 12] = [6.0, 10.0, 16.0, 20.0, 25.0, 32.0, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0];

/// Resistividade do cobre a 70 °C (temperatura máxima do PVC em serviço), Ω·mm²/m.
pub const RESISTIVIDADE_COBRE_70C: f64 = 0.0206;
/// Resistividade do cobre a 90 °C (temperatura máxima do EPR e do XLPE em serviço), Ω·mm²/m.
pub const RESISTIVIDADE_COBRE_90C: f64 = 0.0219;

/// Isolação dos condutores.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Isolacao {
    /// PVC 70 °C (cabos de 450/750 V).
    #[default]
    Pvc,
    /// EPR 90 °C (cabos de 0,6/1 kV).
    Epr,
    /// XLPE 90 °C (cabos de 0,6/1 kV).
    Xlpe,
}

impl Isolacao {
    pub const TODAS: [Isolacao; 3] = [Self::Pvc, Self::Epr, Self::Xlpe];

    pub fn nome(self) -> &'static str {
        match self {
            Self::Pvc => "PVC 70 °C",
            Self::Epr => "EPR 90 °C",
            Self::Xlpe => "XLPE 90 °C",
        }
    }

    /// O código curto ("PVC", "EPR", "XLPE"), como fica gravado no quadro.
    pub fn codigo(self) -> &'static str {
        match self {
            Self::Pvc => "PVC",
            Self::Epr => "EPR",
            Self::Xlpe => "XLPE",
        }
    }

    pub fn de_codigo(t: &str) -> Option<Self> {
        Self::TODAS.into_iter().find(|i| i.codigo().eq_ignore_ascii_case(t.trim()))
    }

    /// Resistividade do cobre na temperatura máxima de serviço da isolação (Ω·mm²/m).
    pub fn resistividade(self) -> f64 {
        if self == Self::Pvc { RESISTIVIDADE_COBRE_70C } else { RESISTIVIDADE_COBRE_90C }
    }

    /// Fator de correção de temperatura (tabela 40). Entre duas linhas da tabela vale a
    /// temperatura de cima, que é o lado seguro.
    pub fn fct(self, temperatura: f64) -> f64 {
        let fator = |l: &(f64, f64, f64)| if self == Self::Pvc { l.1 } else { l.2 };
        FCT_AMBIENTE.iter().find(|l| temperatura <= l.0 + 1e-9).or(FCT_AMBIENTE.last()).map_or(1.0, fator)
    }
}

/// Tabela 42, condutores agrupados em feixe ou num mesmo eletroduto: (circuitos, fator).
const FCA_AGRUPAMENTO: [(usize, f64); 12] =
    [(1, 1.00), (2, 0.80), (3, 0.70), (4, 0.65), (5, 0.60), (6, 0.57), (7, 0.54), (8, 0.52), (11, 0.50), (15, 0.45), (19, 0.41), (usize::MAX, 0.38)];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Tensão fase-neutro, usada quando o circuito monofásico não informa tensão (V).
    pub tensao_fn: f64,
    /// Tensão fase-fase, usada quando o circuito de 2 ou 3 fases não informa tensão (V).
    pub tensao_ff: f64,
    /// Fator de correção de temperatura (tabela 40).
    pub fct: f64,
    /// Fator de correção de agrupamento (tabela 42).
    pub fca: f64,
    /// Fases do quadro quando o desenho não diz (sistema de distribuição do projeto).
    pub fases_padrao: u8,
    /// Queda de tensão admissível no circuito terminal (%).
    pub queda_admissivel: f64,
    /// Isolação dos condutores: escolhe a tabela de capacidade e a resistividade.
    pub isolacao: Isolacao,
}

impl Default for Config {
    fn default() -> Self {
        Config { tensao_fn: 127.0, tensao_ff: 220.0, fct: 1.0, fca: 1.0, fases_padrao: 2, queda_admissivel: 4.0, isolacao: Isolacao::Pvc }
    }
}

/// Seção mínima pela tabela 47: 1,5 mm² em iluminação, 2,5 mm² em força.
pub fn secao_minima(natureza: Natureza) -> f64 {
    if natureza == Natureza::Iluminacao { 1.5 } else { 2.5 }
}

/// Corrente de projeto Ib (A). Trifásico usa a tensão de linha.
pub fn corrente_projeto(potencia_va: f64, tensao: f64, fases: u8) -> f64 {
    if tensao <= 0.0 {
        return 0.0;
    }
    if fases >= 3 { potencia_va / (3f64.sqrt() * tensao) } else { potencia_va / tensao }
}

/// Condutores carregados: F+N ou F+F são dois, trifásico são três.
pub fn condutores_carregados(fases: u8) -> u8 {
    if fases >= 3 { 3 } else { 2 }
}

/// Condutores no eletroduto para o levantamento de material: fases, neutro (monofásico e
/// trifásico) e proteção.
pub fn condutores_total(fases: u8) -> u8 {
    if fases >= 3 { 5 } else { 3 }
}

/// Capacidade de condução (tabela 36 ou 37, B1) da seção de índice `i` em [`SECOES`].
pub fn capacidade(i: usize, carregados: u8, isolacao: Isolacao) -> Option<f64> {
    let tabela = match (isolacao, carregados >= 3) {
        (Isolacao::Pvc, false) => &IZ_PVC_DOIS,
        (Isolacao::Pvc, true) => &IZ_PVC_TRES,
        (_, false) => &IZ_EPR_DOIS,
        (_, true) => &IZ_EPR_TRES,
    };
    tabela.get(i).copied()
}

pub fn indice_secao(secao: f64) -> Option<usize> {
    SECOES.iter().position(|s| (s - secao).abs() < 1e-6)
}

/// Fator de agrupamento da tabela 42 para `n` circuitos no mesmo eletroduto.
pub fn fca_agrupamento(n: usize) -> f64 {
    FCA_AGRUPAMENTO.iter().find(|(ate, _)| n <= *ate).map_or(0.38, |(_, f)| *f)
}

/// A tensão do circuito: a dos pontos ou a do sistema (fase-neutro ou fase-fase). O `bool` diz
/// se veio dos pontos.
pub fn tensao_circuito(c: &Circuito, cfg: &Config) -> (f64, bool) {
    match c.tensao {
        Some(v) => (v, true),
        None if c.fases >= 2 => (cfg.tensao_ff, false),
        None => (cfg.tensao_fn, false),
    }
}

/// Queda de tensão (%) para o momento elétrico `momento` (soma de corrente × comprimento, A·m)
/// na seção `secao`: 2ρ·M/S em circuitos de uma ou duas fases, √3ρ·M/S em trifásicos, com ρ do
/// cobre na temperatura de serviço da isolação.
pub fn queda_percentual(momento: f64, secao: f64, tensao: f64, fases: u8, isolacao: Isolacao) -> f64 {
    if secao <= 0.0 || tensao <= 0.0 {
        return 0.0;
    }
    let k = if fases >= 3 { 3f64.sqrt() } else { 2.0 };
    k * isolacao.resistividade() * momento / secao / tensao * 100.0
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimensionamento {
    pub tensao: f64,
    /// Corrente de projeto (A).
    pub ib: f64,
    /// Seção do condutor (mm²). `None`: a corrente passa da tabela.
    pub secao: Option<f64>,
    /// Capacidade de condução já corrigida por FCT e FCA (A).
    pub iz: Option<f64>,
    /// Corrente nominal do disjuntor (A).
    pub disjuntor: Option<f64>,
    /// Queda de tensão na seção escolhida (%), quando o comprimento é conhecido.
    pub queda: Option<f64>,
    pub avisos: Vec<String>,
}

/// Escolhe seção e disjuntor só pela corrente (sem comprimento).
pub fn dimensionar(c: &Circuito, cfg: &Config) -> Dimensionamento {
    dimensionar_com(c, cfg, None)
}

/// Escolhe seção e disjuntor: a menor seção (acima da mínima) que conduz Ib corrigida, que tem
/// um disjuntor com Ib ≤ In ≤ Iz e, com o momento elétrico conhecido, cuja queda de tensão fica
/// dentro do admissível.
pub fn dimensionar_com(c: &Circuito, cfg: &Config, momento: Option<f64>) -> Dimensionamento {
    let mut avisos = Vec::new();
    let (tensao, informada) = tensao_circuito(c, cfg);
    if !informada {
        avisos.push(format!("circuito sem tensão nos pontos: usando {tensao} V do sistema"));
    }
    let ib = corrente_projeto(c.potencia, tensao, c.fases);
    let carregados = condutores_carregados(c.fases);
    let fator = cfg.fct * cfg.fca;
    let minima = secao_minima(c.natureza);
    for (i, secao) in SECOES.iter().copied().enumerate().filter(|(_, s)| *s >= minima) {
        let Some(iz) = capacidade(i, carregados, cfg.isolacao).map(|v| v * fator) else { continue };
        if iz < ib {
            continue;
        }
        // Ib ≤ In ≤ Iz; se nenhum disjuntor cabe nesta seção, a próxima seção resolve.
        let Some(disjuntor) = DISJUNTORES.iter().copied().find(|d| *d >= ib && *d <= iz) else { continue };
        let queda = momento.map(|m| queda_percentual(m, secao, tensao, c.fases, cfg.isolacao));
        if queda.is_some_and(|q| q > cfg.queda_admissivel) {
            continue;
        }
        return Dimensionamento { tensao, ib, secao: Some(secao), iz: Some(iz), disjuntor: Some(disjuntor), queda, avisos };
    }
    avisos.push("nenhuma seção até 120 mm² atende corrente e queda de tensão".into());
    Dimensionamento { tensao, ib, secao: None, iz: None, disjuntor: None, queda: None, avisos }
}

/// Confere uma escolha feita à mão (seção e disjuntor sobrescritos): calcula Iz e queda e avisa
/// o que a norma não aceita, sem impedir.
pub fn verificar(c: &Circuito, cfg: &Config, momento: Option<f64>, secao: f64, disjuntor: f64) -> Dimensionamento {
    let mut avisos = Vec::new();
    let (tensao, _) = tensao_circuito(c, cfg);
    let ib = corrente_projeto(c.potencia, tensao, c.fases);
    let iz = indice_secao(secao).and_then(|i| capacidade(i, condutores_carregados(c.fases), cfg.isolacao)).map(|v| v * cfg.fct * cfg.fca);
    let queda = momento.map(|m| queda_percentual(m, secao, tensao, c.fases, cfg.isolacao));
    if secao < secao_minima(c.natureza) {
        avisos.push(format!("seção abaixo da mínima da tabela 47 ({} mm²)", secao_minima(c.natureza)));
    }
    if disjuntor < ib {
        avisos.push("disjuntor menor que a corrente de projeto (In < Ib)".into());
    }
    match iz {
        Some(iz) if disjuntor > iz => avisos.push("disjuntor maior que a capacidade do condutor (In > Iz)".into()),
        Some(_) => {}
        None => avisos.push("seção fora da tabela implementada".into()),
    }
    if queda.is_some_and(|q| q > cfg.queda_admissivel) {
        avisos.push(format!("queda de tensão acima de {} %", cfg.queda_admissivel));
    }
    Dimensionamento { tensao, ib, secao: Some(secao), iz, disjuntor: Some(disjuntor), queda, avisos }
}
