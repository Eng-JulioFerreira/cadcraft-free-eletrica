// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Free Elétrica: projeto elétrico de baixa tensão (NBR 5410) sobre o desenho do CADCraft.
//!
//! Lê os pontos elétricos (blocos com atributos), agrupa em circuitos por quadro, roteia a
//! fiação pelos eletrodutos e dimensiona cada circuito. Não depende da interface: a paleta
//! Elétrica do `cadcraft-ui-egui` e os comandos do motor só usam o que está aqui.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod biblioteca;
pub mod circuito;
pub mod demanda;
pub mod editar;
pub mod eletroduto;
pub mod fiacao;
pub mod nbr5410;
pub mod ponto;
pub mod quadro;
pub mod rota;
pub mod setup;
pub mod xdata;

#[cfg(test)]
mod tests;

use cadcraft_doc::{Drawing, Handle};
use serde::Serialize;

pub use circuito::{Circuito, Natureza};
pub use nbr5410::Config;
pub use ponto::{PontoEletrico, TipoPonto};
pub use quadro::{ResumoQuadro, Sobrescrita};
pub use setup::{CaboTelecom, CategoriaTelecom, CorCabo, Setup, Sistema, Unidade};

/// O que o desenho tem de instalação elétrica.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analise {
    pub pontos: Vec<PontoEletrico>,
    pub circuitos: Vec<Circuito>,
}

impl Analise {
    pub fn contar(&self, tipo: TipoPonto) -> usize {
        self.pontos.iter().filter(|p| p.tipo == tipo).count()
    }

    /// Avisos de preenchimento de cada ponto: (rótulo do ponto, aviso).
    pub fn avisos_pontos(&self) -> Vec<(String, String)> {
        self.pontos.iter().flat_map(|p| p.avisos().into_iter().map(move |a| (p.rotulo(), a))).collect()
    }

    pub fn ponto(&self, h: Handle) -> Option<&PontoEletrico> {
        self.pontos.iter().find(|p| p.handle == h)
    }

    /// Os quadros do desenho.
    pub fn quadros(&self) -> Vec<&PontoEletrico> {
        quadro::quadros(self)
    }
}

pub fn analisar(doc: &Drawing) -> Analise {
    let pontos = ponto::ler_pontos(doc);
    let circuitos = circuito::agrupar(&pontos);
    Analise { pontos, circuitos }
}

/// Roteia e dimensiona os circuitos de um quadro (a tabela de resumo de Rotear fiação).
pub fn rotear_quadro(doc: &Drawing, analise: &Analise, setup: &Setup, quadro: Handle) -> Result<ResumoQuadro, String> {
    quadro::rotear(doc, analise, setup, quadro)
}
