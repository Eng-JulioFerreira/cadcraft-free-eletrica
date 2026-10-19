// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Pontos elétricos: blocos inseridos no espaço do modelo que carregam atributos elétricos
//! (padrão em `docs/padrao_blocos_eletricos.md` do Free Elétrica).

use cadcraft_doc::{Drawing, EntityKind, Handle, Insert, RawTag};
use serde::Serialize;

use crate::demanda::TipoCarga;

/// Atributos que marcam um bloco como ponto elétrico. Um bloco sem nenhum deles (porta, móvel,
/// carimbo) não entra na análise, mesmo que o nome lembre um ponto elétrico. A primeira tag de
/// cada lista é a que o Free Elétrica grava.
pub const TAGS_TIPO: &[&str] = &["TIPO_PONTO", "TIPO", "TYPE", "CATEGORIA"];
pub const TAGS_POTENCIA: &[&str] = &["POTENCIA", "POT", "POWER"];
pub const TAGS_TENSAO: &[&str] = &["TENSAO", "VOLTS", "VOLTAGEM"];
pub const TAGS_FASES: &[&str] = &["NUMERO_FASES", "FASES", "NUM_FASES", "FASE"];
pub const TAGS_CIRCUITO: &[&str] = &["CIRCUITO", "CIRC", "CIRCUIT"];
pub const TAGS_COMANDO: &[&str] = &["COMANDO", "CMD"];
pub const TAGS_ID: &[&str] = &["TAG", "ID", "NOME"];
pub const TAGS_ALTURA: &[&str] = &["ALTURA", "ALTURA_INSTALACAO", "H_INSTALACAO", "ALT"];
pub const TAGS_QUADRO: &[&str] = &["QUADRO"];
/// Fator de potência do ponto: "0,92", "0.92" ou "92%".
pub const TAGS_FP: &[&str] = &["FP", "FATOR_POTENCIA", "FATOR_DE_POTENCIA", "COS_FI"];
/// Tipo de carga na demanda: ILUM_TUG, AQUECIMENTO, ELETRODOMESTICOS, AR, BOMBA, OUTROS, RESERVA.
pub const TAGS_CARGA: &[&str] = &["TIPO_CARGA", "CARGA"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TipoPonto {
    Quadro,
    Iluminacao,
    /// Tomada de uso geral (TUG).
    Tomada,
    /// Tomada de uso específico (TUE).
    TomadaEspecifica,
    Interruptor,
    /// Caixa de passagem: não é carga.
    Caixa,
    /// Ponto com atributos elétricos mas tipo não reconhecido.
    Outro,
}

impl TipoPonto {
    pub fn nome(self) -> &'static str {
        match self {
            Self::Quadro => "Quadro",
            Self::Iluminacao => "Iluminação",
            Self::Tomada => "TUG",
            Self::TomadaEspecifica => "TUE",
            Self::Interruptor => "Interruptor",
            Self::Caixa => "Caixa de passagem",
            Self::Outro => "Outro",
        }
    }

    /// Consome potência do circuito.
    pub fn e_carga(self) -> bool {
        matches!(self, Self::Iluminacao | Self::Tomada | Self::TomadaEspecifica | Self::Outro)
    }

    /// O tipo descrito por um texto: o valor de TIPO_PONTO ou o nome do bloco.
    pub fn classificar(texto: &str) -> Option<Self> {
        let n = normalizar(texto);
        let palavras: Vec<&str> = n.split(' ').filter(|p| !p.is_empty()).collect();
        let palavra = |p: &str| palavras.contains(&p);
        let comeca = |p: &str| palavras.iter().any(|w| w.starts_with(p));
        let contem = |p: &str| n.contains(p);

        if contem("QUADRO") || palavra("QGBT") || comeca("QD") {
            return Some(Self::Quadro);
        }
        if contem("INTERRUPTOR") || comeca("INT") || contem("PULSADOR") || contem("DIMMER") {
            return Some(Self::Interruptor);
        }
        if comeca("TUE") || contem("ESPECIFIC") || contem("CHUVEIRO") {
            return Some(Self::TomadaEspecifica);
        }
        if contem("TOMADA") || comeca("TUG") || palavra("TOM") {
            return Some(Self::Tomada);
        }
        if contem("ILUM") || palavra("LUZ") || contem("LUMINARIA") || contem("LAMPADA") || contem("ARANDELA") || contem("PLAFON") || palavra("SPOT") {
            return Some(Self::Iluminacao);
        }
        if contem("PASSAGEM") || palavra("CP") || palavra("CAIXA") {
            return Some(Self::Caixa);
        }
        None
    }
}

/// Maiúsculas, sem acentos, e tudo o que não é letra ou dígito vira espaço.
pub(crate) fn normalizar(s: &str) -> String {
    s.chars()
        .map(|c| match c.to_uppercase().next().unwrap_or(c) {
            'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
            'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'Ç' => 'C',
            c if c.is_ascii_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PontoEletrico {
    #[serde(serialize_with = "handle_hex")]
    pub handle: Handle,
    pub bloco: String,
    pub camada: String,
    pub posicao: [f64; 2],
    pub tipo: TipoPonto,
    /// O tipo veio do atributo TIPO_PONTO (e não do nome do bloco).
    pub tipo_informado: bool,
    /// Potência em VA (W é tratado como VA por enquanto).
    pub potencia: Option<f64>,
    pub tensao: Option<f64>,
    pub fases: Option<u8>,
    pub circuito: Option<String>,
    pub comando: Option<String>,
    pub tag: Option<String>,
    /// Altura de instalação (m), do atributo ALTURA. `None`: usa a altura padrão do tipo.
    pub altura: Option<f64>,
    /// O quadro que alimenta o ponto (XDATA gravado pela ferramenta de circuitos, atributo
    /// QUADRO com a TAG do quadro, ou o único quadro do desenho).
    #[serde(serialize_with = "handle_hex_opcional")]
    pub quadro: Option<Handle>,
    /// A referência ao quadro como estava escrita, antes de ser resolvida.
    #[serde(skip)]
    pub quadro_ref: Option<String>,
    /// Fator de potência do atributo FP. `None`: o padrão do tipo de carga.
    pub fp: Option<f64>,
    /// Tipo de carga do atributo TIPO_CARGA. `None`: o padrão do tipo do ponto.
    pub tipo_carga: Option<TipoCarga>,
    /// Todos os atributos lidos, na ordem do bloco.
    pub atributos: Vec<(String, String)>,
    /// Valores que o Free Elétrica gravou no XDATA do bloco (valem quando o bloco não tem o atributo).
    pub extras: Vec<(String, String)>,
}

fn handle_hex<S: serde::Serializer>(h: &Handle, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&h.hex())
}

pub(crate) fn handle_hex_opcional<S: serde::Serializer>(h: &Option<Handle>, s: S) -> Result<S::Ok, S::Error> {
    match h {
        Some(h) => s.serialize_str(&h.hex()),
        None => s.serialize_none(),
    }
}

/// O primeiro valor não vazio entre `tags` (sem diferenciar maiúsculas): primeiro os atributos
/// do bloco, depois o XDATA do Free Elétrica.
fn buscar<'a>(atributos: &'a [(String, String)], extras: &'a [(String, String)], tags: &[&str]) -> Option<&'a str> {
    let achar = |lista: &'a [(String, String)]| {
        tags.iter().find_map(|t| lista.iter().find(|(tag, v)| tag.eq_ignore_ascii_case(t) && !v.trim().is_empty()).map(|(_, v)| v.trim()))
    };
    achar(atributos).or_else(|| achar(extras))
}

impl PontoEletrico {
    /// O valor de um atributo (ou do XDATA do Free Elétrica) entre `tags`.
    pub fn valor(&self, tags: &[&str]) -> Option<&str> {
        buscar(&self.atributos, &self.extras, tags)
    }

    /// O bloco tem um atributo de verdade (ATTRIB) com uma destas tags.
    pub fn tem_atributo(&self, tags: &[&str]) -> bool {
        self.atributos.iter().any(|(t, _)| tags.iter().any(|x| t.eq_ignore_ascii_case(x)))
    }

    /// Lê um bloco inserido. `None` quando ele não tem nenhum atributo elétrico.
    pub fn de_insert(doc: &Drawing, handle: Handle, camada: &str, ins: &Insert) -> Option<Self> {
        Self::de_entidade(doc, handle, camada, ins, &[])
    }

    /// Lê um bloco inserido com o XDATA da entidade.
    pub fn de_entidade(doc: &Drawing, handle: Handle, camada: &str, ins: &Insert, xdata: &[RawTag]) -> Option<Self> {
        let extras = crate::xdata::ler(xdata);
        let mut atributos: Vec<(String, String)> = ins.attribs.iter().map(|a| (a.tag.trim().to_string(), a.text.value.clone())).collect();
        // Atributos constantes ficam só na definição do bloco.
        if let Some(def) = doc.blocks.get(&ins.block) {
            for e in def.entities.iter() {
                if let EntityKind::AttDef(a) = &e.kind
                    && a.constant
                    && !atributos.iter().any(|(t, _)| t.eq_ignore_ascii_case(a.tag.trim()))
                {
                    atributos.push((a.tag.trim().to_string(), a.text.value.clone()));
                }
            }
        }
        let todas = [TAGS_TIPO, TAGS_POTENCIA, TAGS_TENSAO, TAGS_FASES, TAGS_CIRCUITO, TAGS_COMANDO, TAGS_ALTURA, TAGS_QUADRO];
        if !todas.iter().any(|tags| buscar(&atributos, &extras, tags).is_some()) {
            return None;
        }
        let valor = |tags| buscar(&atributos, &extras, tags);
        let tipo_attr = valor(TAGS_TIPO).and_then(TipoPonto::classificar);
        let tipo = tipo_attr.or_else(|| TipoPonto::classificar(&ins.block)).unwrap_or(TipoPonto::Outro);
        let texto = |tags| valor(tags).map(str::to_string);
        Some(PontoEletrico {
            handle,
            bloco: ins.block.clone(),
            camada: camada.to_string(),
            posicao: [ins.insert.x, ins.insert.y],
            tipo,
            tipo_informado: tipo_attr.is_some(),
            potencia: valor(TAGS_POTENCIA).and_then(potencia_va),
            tensao: valor(TAGS_TENSAO).and_then(numero),
            fases: valor(TAGS_FASES).and_then(fases),
            circuito: texto(TAGS_CIRCUITO),
            comando: texto(TAGS_COMANDO),
            tag: texto(TAGS_ID),
            altura: valor(TAGS_ALTURA).and_then(altura_m),
            quadro: None,
            quadro_ref: texto(TAGS_QUADRO),
            fp: valor(TAGS_FP).and_then(fator_potencia),
            tipo_carga: valor(TAGS_CARGA).and_then(TipoCarga::de_texto),
            atributos,
            extras,
        })
    }

    /// O tipo de carga na demanda: o do atributo TIPO_CARGA ou, sem ele, TUE em aquecimento e o
    /// resto em iluminação e TUG.
    pub fn tipo_carga_efetivo(&self) -> TipoCarga {
        self.tipo_carga.unwrap_or(if self.tipo == TipoPonto::TomadaEspecifica { TipoCarga::Aquecimento } else { TipoCarga::IluminacaoTug })
    }

    /// O fator de potência: o do atributo FP ou o padrão do tipo de carga.
    pub fn fp_efetivo(&self) -> f64 {
        self.fp.unwrap_or_else(|| self.tipo_carga_efetivo().fp_padrao())
    }

    /// Problemas de preenchimento segundo o padrão de blocos.
    pub fn avisos(&self) -> Vec<String> {
        let mut v = Vec::new();
        match self.tipo {
            TipoPonto::Interruptor => {
                if self.comando.is_none() {
                    v.push("interruptor sem COMANDO (letra da tecla)".into());
                }
                if self.potencia.is_some_and(|p| p > 0.0) {
                    v.push("interruptor deve ter POTENCIA 0W".into());
                }
                if self.circuito.is_none() {
                    v.push("interruptor sem CIRCUITO".into());
                }
            }
            TipoPonto::Quadro | TipoPonto::Caixa => {}
            TipoPonto::Iluminacao | TipoPonto::Tomada | TipoPonto::TomadaEspecifica | TipoPonto::Outro => {
                if self.tipo == TipoPonto::Outro {
                    v.push("tipo não reconhecido: preencha TIPO_PONTO (TUG, TUE, ILUM, INTERRUPTOR, QUADRO)".into());
                }
                if self.potencia.is_none() {
                    v.push("ponto de carga sem POTENCIA".into());
                }
                if self.tensao.is_none() {
                    v.push("ponto sem TENSAO: usando a do sistema das configurações".into());
                }
                if self.fases.is_none() {
                    v.push("ponto sem NUMERO_FASES: considerado monofásico".into());
                }
                if self.circuito.is_none() {
                    v.push("ponto sem CIRCUITO".into());
                }
                if let Some(t) = self.valor(TAGS_FP)
                    && self.fp.is_none()
                {
                    v.push(format!("FP \"{t}\" inválido: use um valor de 0 a 1 (ex.: 0,92)"));
                }
                if let Some(t) = self.valor(TAGS_CARGA)
                    && self.tipo_carga.is_none()
                {
                    v.push(format!("TIPO_CARGA \"{t}\" não reconhecido: use ILUM_TUG, AQUECIMENTO, ELETRODOMESTICOS, AR, BOMBA, OUTROS ou RESERVA"));
                }
            }
        }
        if self.altura.is_none() {
            match self.tipo {
                // Iluminação sem ALTURA fica no teto, que é o normal.
                TipoPonto::Caixa | TipoPonto::Iluminacao => {}
                t => v.push(format!("sem ALTURA de instalação: usando {} m", br(altura_padrao(t, None)))),
            }
        }
        if self.tipo != TipoPonto::Quadro
            && let Some(r) = &self.quadro_ref
            && self.quadro.is_none()
        {
            v.push(format!("QUADRO \"{r}\" não encontrado no desenho"));
        }
        v
    }

    /// Nome curto para listas: a TAG, ou o bloco e o handle.
    pub fn rotulo(&self) -> String {
        self.tag.clone().unwrap_or_else(|| format!("{} #{}", self.bloco, self.handle.hex()))
    }
}

/// Todos os pontos elétricos do espaço do modelo, cada um ligado ao seu quadro.
pub fn ler_pontos(doc: &Drawing) -> Vec<PontoEletrico> {
    let mut pontos: Vec<PontoEletrico> = doc
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(ins) => PontoEletrico::de_entidade(doc, e.handle, &e.common.layer, ins, &e.common.xdata),
            _ => None,
        })
        .collect();
    let quadros: Vec<(Handle, Option<String>)> = pontos.iter().filter(|p| p.tipo == TipoPonto::Quadro).map(|p| (p.handle, p.tag.clone())).collect();
    for p in pontos.iter_mut().filter(|p| p.tipo != TipoPonto::Quadro) {
        p.quadro = match &p.quadro_ref {
            // O handle gravado pela ferramenta de circuitos, ou a TAG do quadro escrita à mão.
            Some(r) => Handle::parse_hex(r)
                .filter(|h| quadros.iter().any(|(q, _)| q == h))
                .or_else(|| quadros.iter().find(|(_, tag)| tag.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(r))).map(|(q, _)| *q)),
            // Sem referência: o único quadro do desenho.
            None => (quadros.len() == 1).then(|| quadros[0].0),
        };
    }
    pontos
}

/// Leitura de um único ponto pelo handle (sem resolver o quadro).
pub fn ler_ponto(doc: &Drawing, h: Handle) -> Option<PontoEletrico> {
    let e = doc.model.get(h)?;
    match &e.kind {
        EntityKind::Insert(ins) => PontoEletrico::de_entidade(doc, e.handle, &e.common.layer, ins, &e.common.xdata),
        _ => None,
    }
}

/// Altura de instalação padrão quando o bloco não informa (m). Iluminação e caixas ficam no
/// teto: com o pé-direito, a descida é zero.
pub fn altura_padrao(tipo: TipoPonto, pe_direito: Option<f64>) -> f64 {
    let teto = pe_direito.unwrap_or(2.8);
    match tipo {
        TipoPonto::Iluminacao | TipoPonto::Caixa => teto,
        TipoPonto::Interruptor => 1.1,
        TipoPonto::Quadro => 1.5,
        TipoPonto::Tomada | TipoPonto::TomadaEspecifica | TipoPonto::Outro => 0.3,
    }
}

/// Fator de potência: "0,92", "0.92", "92%" ou "92" (de 10 a 100 é porcentagem). Fora de
/// (0, 1] não vale: "1,5" é erro, não 1,5 %.
pub fn fator_potencia(texto: &str) -> Option<f64> {
    let n = numero(texto)?;
    let f = if texto.contains('%') || (10.0..=100.0).contains(&n) { n / 100.0 } else { n };
    (f > 0.0 && f <= 1.0).then_some(f)
}

/// Altura em metros: "0,30", "1.10 m", "30cm", "300 mm". Sem unidade e acima de 10, é cm.
pub fn altura_m(texto: &str) -> Option<f64> {
    let n = numero(texto)?;
    let t = texto.to_ascii_lowercase();
    Some(if t.contains("mm") {
        n / 1000.0
    } else if t.contains("cm") || (!t.contains('m') && n > 10.0) {
        n / 100.0
    } else {
        n
    })
}

/// Número com vírgula decimal, sem zeros sobrando.
pub fn br(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.replace('.', ",")
}

/// O primeiro número do texto. Aceita vírgula decimal ("1,5") e ponto de milhar ("1.500").
pub fn numero(texto: &str) -> Option<f64> {
    let inicio = texto.find(|c: char| c.is_ascii_digit())?;
    let resto = &texto[inicio..];
    let fim = resto.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(resto.len());
    let bruto = resto[..fim].trim_end_matches(['.', ',']);
    let limpo = if bruto.contains(',') {
        bruto.replace('.', "").replace(',', ".")
    } else if bruto.split('.').skip(1).all(|g| g.len() == 3) && bruto.contains('.') {
        // "1.500": ponto de milhar.
        bruto.replace('.', "")
    } else {
        bruto.to_string()
    };
    limpo.parse().ok()
}

/// Potência em VA: "600VA", "100 W", "1,5kW", "5400W".
pub fn potencia_va(texto: &str) -> Option<f64> {
    let n = numero(texto)?;
    let depois = texto.trim_start_matches(|c: char| !c.is_ascii_digit()).trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ',');
    let kilo = depois.trim_start().starts_with(['k', 'K']);
    Some(if kilo { n * 1000.0 } else { n })
}

/// Número de fases: "1".."3", "MONO", "BI", "TRI".
pub fn fases(texto: &str) -> Option<u8> {
    let n = normalizar(texto);
    if n.contains("TRI") {
        return Some(3);
    }
    if n.contains("BI") {
        return Some(2);
    }
    if n.contains("MONO") {
        return Some(1);
    }
    numero(texto).map(|f| f as u8).filter(|f| (1..=3).contains(f))
}
