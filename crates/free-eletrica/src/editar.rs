// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Escritas do Free Elétrica no desenho. Um valor vai para o atributo do bloco quando ele existe
//! (o projetista vê e edita) e, senão, para o XDATA do Free Elétrica (viaja com o bloco, invisível).

use cadcraft_doc::{Drawing, EntityKind, Handle};

use crate::nbr5410::Isolacao;
use crate::ponto::{TAGS_CIRCUITO, TAGS_COMANDO, TAGS_QUADRO, ler_ponto};
use crate::quadro::{CHAVE_DESCRICAO, CHAVE_DISJUNTOR, CHAVE_ISOLACAO, CHAVE_ISOLACAO_CIRCUITO, CHAVE_SECAO};
use crate::xdata;

/// Grava (ou apaga) uma chave do XDATA do Free Elétrica num bloco.
fn gravar_xdata(doc: &mut Drawing, h: Handle, chave: &str, valor: Option<&str>) -> Result<(), String> {
    let mut e = (**doc.model.get(h).ok_or("objeto não encontrado")?).clone();
    if !matches!(e.kind, EntityKind::Insert(_)) {
        return Err("o objeto não é um bloco".into());
    }
    xdata::gravar(&mut e.common.xdata, chave, valor);
    doc.model.replace(e);
    Ok(())
}

/// Os pontos do circuito `circuito` do quadro `quadro`.
fn pontos_do_circuito(doc: &Drawing, quadro: Handle, circuito: &str) -> Vec<Handle> {
    crate::analisar(doc)
        .pontos
        .iter()
        .filter(|p| p.quadro == Some(quadro) && p.circuito.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(circuito.trim())))
        .map(|p| p.handle)
        .collect()
}

/// Apaga do quadro as escolhas e a descrição de um circuito.
fn esquecer_circuito(doc: &mut Drawing, quadro: Handle, circuito: &str) -> Result<(), String> {
    let id = circuito.trim().to_uppercase();
    for prefixo in [CHAVE_SECAO, CHAVE_DISJUNTOR, CHAVE_DESCRICAO, CHAVE_ISOLACAO_CIRCUITO] {
        gravar_xdata(doc, quadro, &format!("{prefixo}{id}"), None)?;
    }
    Ok(())
}

fn chave_do_circuito(prefixo: &str, circuito: &str) -> Result<String, String> {
    let id = circuito.trim().to_uppercase();
    if id.is_empty() {
        return Err("informe o circuito".into());
    }
    Ok(format!("{prefixo}{id}"))
}

/// Isolação dos condutores de um circuito; `None` volta à do quadro.
pub fn definir_isolacao_circuito(doc: &mut Drawing, quadro: Handle, circuito: &str, isolacao: Option<Isolacao>) -> Result<(), String> {
    gravar_xdata(doc, quadro, &chave_do_circuito(CHAVE_ISOLACAO_CIRCUITO, circuito)?, isolacao.map(Isolacao::codigo))
}

/// Isolação dos condutores do quadro; `None` volta à das configurações.
pub fn definir_isolacao(doc: &mut Drawing, quadro: Handle, isolacao: Option<Isolacao>) -> Result<(), String> {
    gravar_xdata(doc, quadro, CHAVE_ISOLACAO, isolacao.map(Isolacao::codigo))
}

/// Descrição do circuito no quadro de cargas (`None` apaga). Um circuito descrito existe no
/// quadro mesmo sem pontos.
pub fn definir_descricao(doc: &mut Drawing, quadro: Handle, circuito: &str, descricao: Option<&str>) -> Result<(), String> {
    let id = circuito.trim().to_uppercase();
    if id.is_empty() {
        return Err("informe o circuito".into());
    }
    gravar_xdata(doc, quadro, &format!("{CHAVE_DESCRICAO}{id}"), descricao)
}

/// Exclui o circuito do quadro: os pontos dele ficam sem circuito e sem quadro, e as escolhas e a
/// descrição dele somem. Devolve quantos pontos foram liberados.
pub fn excluir_circuito(doc: &mut Drawing, quadro: Handle, circuito: &str) -> Result<usize, String> {
    let pontos = pontos_do_circuito(doc, quadro, circuito);
    for h in &pontos {
        definir(doc, *h, TAGS_CIRCUITO, None)?;
        definir(doc, *h, TAGS_QUADRO, None)?;
    }
    esquecer_circuito(doc, quadro, circuito)?;
    Ok(pontos.len())
}

/// Junta o circuito `de` no circuito `para` do mesmo quadro: os pontos de `de` passam para `para`
/// e as escolhas e a descrição de `de` somem. Devolve quantos pontos mudaram.
pub fn juntar_circuitos(doc: &mut Drawing, quadro: Handle, de: &str, para: &str) -> Result<usize, String> {
    if de.trim().eq_ignore_ascii_case(para.trim()) {
        return Err("escolha dois circuitos diferentes".into());
    }
    if para.trim().is_empty() {
        return Err("informe o circuito de destino".into());
    }
    let pontos = pontos_do_circuito(doc, quadro, de);
    for h in &pontos {
        atribuir_circuito(doc, *h, quadro, para.trim())?;
    }
    esquecer_circuito(doc, quadro, de)?;
    Ok(pontos.len())
}

/// Grava (`Some`) ou limpa (`None`) o valor de `tags` no bloco `h`. A primeira tag é a chave
/// do XDATA.
pub fn definir(doc: &mut Drawing, h: Handle, tags: &[&str], valor: Option<&str>) -> Result<(), String> {
    let mut e = (**doc.model.get(h).ok_or("objeto não encontrado")?).clone();
    let chave = tags.first().copied().ok_or("sem tag")?;
    let EntityKind::Insert(ins) = &mut e.kind else { return Err("o objeto não é um bloco".into()) };
    match ins.attribs.iter_mut().find(|a| tags.iter().any(|t| a.tag.trim().eq_ignore_ascii_case(t))) {
        Some(a) => {
            a.text.value = valor.unwrap_or("").to_string();
            // O atributo manda: um valor antigo no XDATA não pode ficar por trás dele.
            xdata::gravar(&mut e.common.xdata, chave, None);
        }
        None => xdata::gravar(&mut e.common.xdata, chave, valor),
    }
    doc.model.replace(e);
    Ok(())
}

/// Liga o ponto ao quadro. No XDATA vai o handle do quadro; num atributo QUADRO, a TAG dele.
pub fn ligar_ao_quadro(doc: &mut Drawing, ponto: Handle, quadro: Handle) -> Result<(), String> {
    let p = ler_ponto(doc, ponto).ok_or("o objeto não é um ponto elétrico")?;
    let tag_quadro = ler_ponto(doc, quadro).and_then(|q| q.tag);
    let valor = match (p.tem_atributo(TAGS_QUADRO), tag_quadro) {
        (true, Some(tag)) => tag,
        _ => quadro.hex(),
    };
    definir(doc, ponto, TAGS_QUADRO, Some(&valor))
}

/// Põe o ponto no circuito `circuito` do quadro `quadro`.
pub fn atribuir_circuito(doc: &mut Drawing, ponto: Handle, quadro: Handle, circuito: &str) -> Result<(), String> {
    definir(doc, ponto, TAGS_CIRCUITO, Some(circuito))?;
    ligar_ao_quadro(doc, ponto, quadro)
}

/// Grava a letra de comando do ponto.
pub fn definir_comando(doc: &mut Drawing, ponto: Handle, letra: &str) -> Result<(), String> {
    definir(doc, ponto, TAGS_COMANDO, Some(letra))
}

/// Grava (ou limpa, com `None`) a seção e o disjuntor escolhidos à mão para um circuito, no
/// XDATA do quadro.
pub fn definir_sobrescrita(doc: &mut Drawing, quadro: Handle, circuito: &str, secao: Option<f64>, disjuntor: Option<f64>) -> Result<(), String> {
    let mut e = (**doc.model.get(quadro).ok_or("quadro não encontrado")?).clone();
    if !matches!(e.kind, EntityKind::Insert(_)) {
        return Err("o quadro não é um bloco".into());
    }
    let id = circuito.trim().to_uppercase();
    xdata::gravar(&mut e.common.xdata, &format!("{CHAVE_SECAO}{id}"), secao.map(|s| s.to_string()).as_deref());
    xdata::gravar(&mut e.common.xdata, &format!("{CHAVE_DISJUNTOR}{id}"), disjuntor.map(|d| d.to_string()).as_deref());
    doc.model.replace(e);
    Ok(())
}
