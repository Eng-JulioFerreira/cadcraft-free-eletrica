// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Cálculo de demanda do quadro pelas tabelas das concessionárias.
//!
//! Cada ponto de carga é um aparelho com um tipo de carga (atributo TIPO_CARGA do bloco:
//! iluminação e TUG, aquecimento, eletrodomésticos, ar condicionado, bomba d'água, outras,
//! reserva). A demanda de cada tipo sai da tabela da concessionária escolhida nas
//! Configurações:
//! - iluminação e TUG: fator por faixa de potência acumulada, conforme o tipo de edificação;
//! - aquecimento e eletrodomésticos: fator pelo número de aparelhos, em duas colunas (potência
//!   individual até o limite ou acima dele), cada coluna com a sua contagem;
//! - ar condicionado e bombas: fator pelo número de aparelhos;
//! - outras cargas: 100 % (a norma deixa o fator ao projetista);
//! - reserva: fora da demanda.
//!
//! As potências do Free Elétrica já estão em VA (aparente): a demanda sai em VA sem dividir pelo
//! fator de potência, que nas normas converte potências dadas em W.
//!
//! As tabelas são transcritas das normas públicas das concessionárias e citadas em `fonte`.
//! Confira sempre a revisão em vigor na sua área de concessão.

use serde::{Deserialize, Serialize};

/// O tipo de carga de um circuito no cálculo de demanda.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TipoCarga {
    #[default]
    IluminacaoTug,
    /// Chuveiro, torneira, aquecedor, forno, fogão elétrico.
    Aquecimento,
    /// Geladeira, televisão, som, lava-roupas e semelhantes.
    Eletrodomesticos,
    ArCondicionado,
    /// Motores de bombas d'água.
    Bomba,
    Outros,
    /// Circuito reserva: não entra na demanda.
    Reserva,
}

impl TipoCarga {
    pub const TODOS: [TipoCarga; 7] =
        [Self::IluminacaoTug, Self::Aquecimento, Self::Eletrodomesticos, Self::ArCondicionado, Self::Bomba, Self::Outros, Self::Reserva];

    pub fn nome(self) -> &'static str {
        match self {
            Self::IluminacaoTug => "Iluminação e TUG",
            Self::Aquecimento => "Aquecimento",
            Self::Eletrodomesticos => "Eletrodomésticos",
            Self::ArCondicionado => "Ar condicionado",
            Self::Bomba => "Bomba d'água",
            Self::Outros => "Outras cargas",
            Self::Reserva => "Reserva",
        }
    }

    /// Código do atributo TIPO_CARGA dos blocos.
    pub fn codigo(self) -> &'static str {
        match self {
            Self::IluminacaoTug => "ILUM_TUG",
            Self::Aquecimento => "AQUECIMENTO",
            Self::Eletrodomesticos => "ELETRODOMESTICOS",
            Self::ArCondicionado => "AR",
            Self::Bomba => "BOMBA",
            Self::Outros => "OUTROS",
            Self::Reserva => "RESERVA",
        }
    }

    pub fn de_codigo(s: &str) -> Option<TipoCarga> {
        let s = s.trim();
        Self::TODOS.into_iter().find(|t| t.codigo().eq_ignore_ascii_case(s))
    }

    /// O valor do atributo TIPO_CARGA: os códigos e os nomes comuns ("chuveiro", "split",
    /// "geladeira"...), sem diferenciar maiúsculas e acentos.
    pub fn de_texto(s: &str) -> Option<TipoCarga> {
        if let Some(t) = Self::de_codigo(s) {
            return Some(t);
        }
        let n = crate::ponto::normalizar(s);
        let palavras: Vec<&str> = n.split(' ').filter(|p| !p.is_empty()).collect();
        let contem = |p: &str| n.contains(p);
        let palavra = |p: &str| palavras.contains(&p);
        Some(if contem("RESERVA") {
            Self::Reserva
        } else if contem("AQUEC") || contem("CHUVEIRO") || contem("TORNEIRA") || contem("FORNO") || contem("FOGAO") || contem("BOILER") {
            Self::Aquecimento
        } else if contem("ELETRODOM") || contem("GELADEIRA") || contem("REFRIGERADOR") || contem("LAVA") || contem("MICRO") {
            Self::Eletrodomesticos
        } else if palavra("AR") || contem("CONDICIONADO") || contem("SPLIT") {
            Self::ArCondicionado
        } else if contem("BOMBA") {
            Self::Bomba
        } else if contem("OUTR") {
            Self::Outros
        } else if contem("ILUM") || palavra("TUG") || palavra("ILUM TUG") {
            Self::IluminacaoTug
        } else {
            return None;
        })
    }

    /// Fator de potência sugerido: 1,00 em aquecimento resistivo, 0,92 no resto.
    pub fn fp_padrao(self) -> f64 {
        if self == Self::Aquecimento { 1.0 } else { 0.92 }
    }
}

/// Uma faixa de potência acumulada: o fator vale até `ate_kw` (acumulado desde zero); `None` é o
/// que excede a última faixa.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Faixa {
    pub ate_kw: Option<f64>,
    pub fator: f64,
}

const fn faixa(ate_kw: f64, fator: f64) -> Faixa {
    Faixa { ate_kw: Some(ate_kw), fator }
}

const fn resto(fator: f64) -> Faixa {
    Faixa { ate_kw: None, fator }
}

/// Iluminação e tomadas de uso geral de um tipo de edificação.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edificacao {
    pub nome: &'static str,
    pub faixas: &'static [Faixa],
}

/// Fator pelo número de aparelhos: vale até `ate` aparelhos (o último vale para mais).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PorQuantidade {
    pub ate: u32,
    pub fator: f64,
}

/// Fator pelo número de aparelhos em duas colunas: potência individual até o limite, e acima.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PorQuantidadeDupla {
    pub ate: u32,
    pub ate_limite: f64,
    pub acima_limite: f64,
}

/// As tabelas de demanda de uma concessionária.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabelaDemanda {
    pub id: &'static str,
    pub nome: &'static str,
    /// A norma, a revisão e as tabelas usadas.
    pub fonte: &'static str,
    pub edificacoes: &'static [Edificacao],
    /// Aquecimento e eletrodomésticos.
    pub aparelhos: &'static [PorQuantidadeDupla],
    /// Limite da potência individual entre as duas colunas de `aparelhos` (kW).
    pub limite_aparelho_kw: f64,
    pub ar_condicionado: &'static [PorQuantidade],
    pub bombas: &'static [PorQuantidade],
}

/// Equatorial Energia, NT.00001.EQTL revisão 07 (homologada em 29/03/2023), item 7.1 e
/// tabelas 4, 5 e 10.
const EQUATORIAL_NT001_R07: TabelaDemanda = TabelaDemanda {
    id: "equatorial-nt001-r07",
    nome: "Equatorial (NT.00001.EQTL rev. 07)",
    fonte: "Equatorial Energia, NT.00001.EQTL revisão 07, 29/03/2023: item 7.1, tabelas 4 (aquecimento e eletrodomésticos), 5 (iluminação e tomadas) e 10 (condicionadores de ar); bombas: k = 1 para uma, 0,5 para mais de uma.",
    edificacoes: &[
        Edificacao { nome: "Residências e edifícios de apartamentos", faixas: &[faixa(10.0, 1.00), faixa(120.0, 0.35), resto(0.25)] },
        Edificacao { nome: "Auditórios, salões para exposição", faixas: &[resto(1.00)] },
        Edificacao { nome: "Bancos, lojas", faixas: &[resto(1.00)] },
        Edificacao { nome: "Barbearias, salões de beleza", faixas: &[resto(1.00)] },
        Edificacao { nome: "Clubes", faixas: &[resto(1.00)] },
        Edificacao { nome: "Escolas", faixas: &[faixa(12.0, 1.00), resto(0.50)] },
        Edificacao { nome: "Escritórios", faixas: &[faixa(20.0, 1.00), resto(0.70)] },
        Edificacao { nome: "Garagens comerciais, corredores, depósitos", faixas: &[resto(1.00)] },
        Edificacao { nome: "Hospitais", faixas: &[faixa(50.0, 0.40), resto(0.20)] },
        Edificacao { nome: "Hotéis", faixas: &[faixa(20.0, 0.50), faixa(100.0, 0.40), resto(0.30)] },
        Edificacao { nome: "Igrejas", faixas: &[resto(1.00)] },
        Edificacao { nome: "Restaurantes", faixas: &[resto(1.00)] },
    ],
    aparelhos: &[
        PorQuantidadeDupla { ate: 1, ate_limite: 0.80, acima_limite: 0.80 },
        PorQuantidadeDupla { ate: 2, ate_limite: 0.75, acima_limite: 0.65 },
        PorQuantidadeDupla { ate: 3, ate_limite: 0.70, acima_limite: 0.55 },
        PorQuantidadeDupla { ate: 4, ate_limite: 0.66, acima_limite: 0.50 },
        PorQuantidadeDupla { ate: 5, ate_limite: 0.62, acima_limite: 0.45 },
        PorQuantidadeDupla { ate: 6, ate_limite: 0.59, acima_limite: 0.43 },
        PorQuantidadeDupla { ate: 7, ate_limite: 0.56, acima_limite: 0.40 },
        PorQuantidadeDupla { ate: 8, ate_limite: 0.53, acima_limite: 0.36 },
        PorQuantidadeDupla { ate: 9, ate_limite: 0.51, acima_limite: 0.35 },
        PorQuantidadeDupla { ate: 10, ate_limite: 0.49, acima_limite: 0.34 },
        PorQuantidadeDupla { ate: 11, ate_limite: 0.47, acima_limite: 0.32 },
        PorQuantidadeDupla { ate: 12, ate_limite: 0.45, acima_limite: 0.32 },
        PorQuantidadeDupla { ate: 13, ate_limite: 0.43, acima_limite: 0.32 },
        PorQuantidadeDupla { ate: 14, ate_limite: 0.41, acima_limite: 0.32 },
        PorQuantidadeDupla { ate: 15, ate_limite: 0.40, acima_limite: 0.32 },
        PorQuantidadeDupla { ate: 16, ate_limite: 0.39, acima_limite: 0.28 },
        PorQuantidadeDupla { ate: 17, ate_limite: 0.38, acima_limite: 0.28 },
        PorQuantidadeDupla { ate: 18, ate_limite: 0.37, acima_limite: 0.28 },
        PorQuantidadeDupla { ate: 19, ate_limite: 0.36, acima_limite: 0.28 },
        PorQuantidadeDupla { ate: 20, ate_limite: 0.35, acima_limite: 0.28 },
        PorQuantidadeDupla { ate: 21, ate_limite: 0.34, acima_limite: 0.26 },
        PorQuantidadeDupla { ate: 22, ate_limite: 0.33, acima_limite: 0.26 },
        PorQuantidadeDupla { ate: 23, ate_limite: 0.31, acima_limite: 0.26 },
        PorQuantidadeDupla { ate: 24, ate_limite: 0.30, acima_limite: 0.26 },
        PorQuantidadeDupla { ate: 25, ate_limite: 0.30, acima_limite: 0.26 },
        PorQuantidadeDupla { ate: 30, ate_limite: 0.30, acima_limite: 0.24 },
        PorQuantidadeDupla { ate: 40, ate_limite: 0.30, acima_limite: 0.22 },
        PorQuantidadeDupla { ate: 50, ate_limite: 0.30, acima_limite: 0.20 },
        PorQuantidadeDupla { ate: 60, ate_limite: 0.30, acima_limite: 0.18 },
        PorQuantidadeDupla { ate: u32::MAX, ate_limite: 0.30, acima_limite: 0.16 },
    ],
    limite_aparelho_kw: 3.5,
    // Tabela 10: 1 e 2 aparelhos com 1,00 (célula única na norma); acima de 10, 0,70.
    ar_condicionado: &[
        PorQuantidade { ate: 2, fator: 1.00 },
        PorQuantidade { ate: 3, fator: 0.88 },
        PorQuantidade { ate: 4, fator: 0.82 },
        PorQuantidade { ate: 5, fator: 0.78 },
        PorQuantidade { ate: 6, fator: 0.76 },
        PorQuantidade { ate: 7, fator: 0.74 },
        PorQuantidade { ate: 8, fator: 0.72 },
        PorQuantidade { ate: 9, fator: 0.71 },
        PorQuantidade { ate: u32::MAX, fator: 0.70 },
    ],
    bombas: &[PorQuantidade { ate: 1, fator: 1.0 }, PorQuantidade { ate: u32::MAX, fator: 0.5 }],
};

const TABELAS: &[TabelaDemanda] = &[EQUATORIAL_NT001_R07];

/// As tabelas de demanda disponíveis.
pub fn tabelas() -> &'static [TabelaDemanda] {
    TABELAS
}

/// A tabela pelo id; sem ela, a primeira.
pub fn tabela(id: &str) -> &'static TabelaDemanda {
    TABELAS.iter().find(|t| t.id.eq_ignore_ascii_case(id.trim())).unwrap_or(&EQUATORIAL_NT001_R07)
}

impl TabelaDemanda {
    /// O tipo de edificação pelo nome; sem ele, o primeiro (residências).
    pub fn edificacao(&self, nome: &str) -> &'static Edificacao {
        let todas: &'static [Edificacao] = self.edificacoes;
        todas.iter().find(|e| e.nome.eq_ignore_ascii_case(nome.trim())).or_else(|| todas.first()).unwrap_or(&SEM_EDIFICACAO)
    }
}

static SEM_EDIFICACAO: Edificacao = Edificacao { nome: "", faixas: &[resto(1.0)] };

/// Potência demandada (VA) pelas faixas acumuladas: cada faixa aplica o seu fator só à parte da
/// potência dentro dela.
pub fn por_faixas(faixas: &[Faixa], instalada_va: f64) -> f64 {
    let total_kw = instalada_va.max(0.0) / 1000.0;
    let mut inicio = 0.0;
    let mut demanda = 0.0;
    for f in faixas {
        let fim = f.ate_kw.unwrap_or(f64::INFINITY);
        if total_kw > inicio {
            demanda += (total_kw.min(fim) - inicio) * f.fator;
        }
        inicio = fim;
        if total_kw <= fim {
            break;
        }
    }
    demanda * 1000.0
}

/// Fator para `n` aparelhos.
pub fn fator_quantidade(tabela: &[PorQuantidade], n: usize) -> f64 {
    let n = u32::try_from(n).unwrap_or(u32::MAX);
    tabela.iter().find(|q| n <= q.ate).or(tabela.last()).map_or(1.0, |q| q.fator)
}

/// Fatores (até o limite, acima do limite) para `n` aparelhos de cada coluna.
pub fn fator_aparelhos(tabela: &[PorQuantidadeDupla], n: usize, acima: bool) -> f64 {
    let n = u32::try_from(n).unwrap_or(u32::MAX);
    tabela.iter().find(|q| n <= q.ate).or(tabela.last()).map_or(1.0, |q| if acima { q.acima_limite } else { q.ate_limite })
}

/// As cargas de um tipo: a potência (VA) de cada aparelho (ponto de carga).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cargas {
    pub tipo: TipoCarga,
    pub aparelhos: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinhaDemanda {
    pub tipo: TipoCarga,
    pub aparelhos: usize,
    /// Potência instalada (VA).
    pub instalada: f64,
    /// Fator de demanda resultante (demandada / instalada).
    pub fator: f64,
    /// Potência demandada (VA).
    pub demandada: f64,
    /// A regra aplicada, para o projetista conferir.
    pub regra: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Demanda {
    pub tabela: String,
    pub fonte: String,
    pub edificacao: String,
    pub linhas: Vec<LinhaDemanda>,
    /// Potência instalada (VA), reservas fora.
    pub instalada: f64,
    /// Potência demandada (VA).
    pub demandada: f64,
}

/// Calcula a demanda das cargas pela tabela e pelo tipo de edificação.
pub fn calcular(tabela: &TabelaDemanda, edificacao: &str, cargas: &[Cargas]) -> Demanda {
    let ed = tabela.edificacao(edificacao);
    let mut linhas = Vec::new();
    for tipo in TipoCarga::TODOS {
        let aparelhos: Vec<f64> =
            cargas.iter().filter(|c| c.tipo == tipo).flat_map(|c| c.aparelhos.iter().copied()).filter(|p| p.is_finite() && *p > 0.0).collect();
        if aparelhos.is_empty() {
            continue;
        }
        let instalada: f64 = aparelhos.iter().sum();
        let n = aparelhos.len();
        let (demandada, regra) = match tipo {
            TipoCarga::IluminacaoTug => (por_faixas(ed.faixas, instalada), format!("faixas de potência: {}", ed.nome)),
            TipoCarga::Aquecimento | TipoCarga::Eletrodomesticos => {
                let limite = tabela.limite_aparelho_kw * 1000.0;
                let (ate, acima): (Vec<f64>, Vec<f64>) = aparelhos.iter().partition(|p| **p <= limite);
                let d = ate.iter().sum::<f64>() * fator_aparelhos(tabela.aparelhos, ate.len(), false)
                    + acima.iter().sum::<f64>() * fator_aparelhos(tabela.aparelhos, acima.len(), true);
                (d, format!("{} aparelho(s) até {} kW, {} acima", ate.len(), crate::ponto::br(tabela.limite_aparelho_kw), acima.len()))
            }
            TipoCarga::ArCondicionado => (instalada * fator_quantidade(tabela.ar_condicionado, n), format!("{n} aparelho(s)")),
            TipoCarga::Bomba => (instalada * fator_quantidade(tabela.bombas, n), format!("{n} bomba(s)")),
            TipoCarga::Outros => (instalada, "100 % (fator a critério do projetista)".to_string()),
            TipoCarga::Reserva => (0.0, "reserva: fora da demanda".to_string()),
        };
        linhas.push(LinhaDemanda {
            tipo,
            aparelhos: n,
            instalada,
            fator: if instalada > 0.0 { demandada / instalada } else { 0.0 },
            demandada,
            regra,
        });
    }
    Demanda {
        tabela: tabela.nome.to_string(),
        fonte: tabela.fonte.to_string(),
        edificacao: ed.nome.to_string(),
        instalada: linhas.iter().filter(|l| l.tipo != TipoCarga::Reserva).map(|l| l.instalada).sum(),
        demandada: linhas.iter().map(|l| l.demandada).sum(),
        linhas,
    }
}
