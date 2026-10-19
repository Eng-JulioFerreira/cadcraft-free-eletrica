// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Free Elétrica sem interface: todo o motor de cálculo responde por comandos do motor, como na
//! linha de comando, em scripts, no `cadcraft-cli` e no MCP.

use std::sync::Arc;

use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Attrib, Block, Common, Drawing, EntityKind, HAlign, Handle, Insert, Line, Space, Text, VAlign};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn atributo(tag: &str, valor: &str) -> Attrib {
    Attrib {
        tag: tag.into(),
        text: Text {
            insert: Vec3::ZERO,
            align_pt: None,
            height: 0.05,
            value: valor.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        },
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    }
}

fn inserir(d: &mut Drawing, bloco: &str, x: f64, y: f64, attrs: &[(&str, &str)]) -> Handle {
    d.blocks.entry(bloco.to_string()).or_insert_with(|| Arc::new(Block::new(bloco)));
    let ins = Insert {
        block: bloco.into(),
        insert: Vec3::new(x, y, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: attrs.iter().map(|(t, v)| atributo(t, v)).collect(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap()
}

fn eletroduto(d: &mut Drawing, a: (f64, f64), b: (f64, f64)) {
    let c = Common { layer: "ELE-ELETRODUTO-PVC-MEDIA-TETO".into(), ..Default::default() };
    d.add(&Space::Model, c, EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })).unwrap();
}

/// Quadro em 0, luz em 3 (comando a), interruptor em (3,1), TUG em 6; eletrodutos ligando tudo.
fn planta() -> (Drawing, [Handle; 4]) {
    let mut d = Drawing::new_metric();
    d.header.set_i64("INSUNITS", 6);
    let qd = inserir(&mut d, "QDG", 0.0, 0.0, &[("TIPO_PONTO", "QUADRO"), ("TAG", "QDG"), ("NUMERO_FASES", "3"), ("ALTURA", "1,5")]);
    let luz = inserir(&mut d, "ILU-OCT", 3.0, 0.0, &[("TIPO_PONTO", "ILUM"), ("POTENCIA", "100VA"), ("TENSAO", "220V"), ("COMANDO", "")]);
    let int = inserir(&mut d, "INT-SIM", 3.0, 1.0, &[("TIPO_PONTO", "INTERRUPTOR"), ("POTENCIA", "0W"), ("ALTURA", "1,10")]);
    let tug = inserir(&mut d, "TOM-SIM-BAIXA", 6.0, 0.0, &[("TIPO_PONTO", "TUG"), ("POTENCIA", "100VA"), ("TENSAO", "220V"), ("ALTURA", "0,30")]);
    eletroduto(&mut d, (0.0, 0.0), (3.0, 0.0));
    eletroduto(&mut d, (3.0, 0.0), (6.0, 0.0));
    eletroduto(&mut d, (3.0, 0.0), (3.0, 1.0));
    (d, [qd, luz, int, tug])
}

fn circuito(s: &Session, h: Handle) -> Option<String> {
    let doc = s.doc().ok()?;
    free_eletrica::ponto::ler_ponto(doc, h)?.circuito
}

#[test]
fn projeto_inteiro_por_comandos_sem_interface() {
    let mut s = Session::new();
    let (d, [qd, luz, int, tug]) = planta();
    s.open_drawing(d, "Planta", None);

    // Criar circuito pelo JSON e pelo prompt interativo.
    let r = s.execute("elecircuito", &json!({ "quadro": qd.hex(), "pontos": [luz.hex()] })).unwrap();
    assert_eq!(r["circuito"], "1");
    s.start("elecircuito").unwrap();
    s.input(Input::Pick(vec![qd])).unwrap();
    s.input(Input::Enter).unwrap(); // aceita o próximo: 2
    s.input(Input::Pick(vec![tug])).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(circuito(&s, tug).as_deref(), Some("2"));

    // Comandos e luminárias: o interruptor sem circuito entra no da luz.
    let r = s.execute("elecomando", &json!({ "interruptor": int.hex(), "luminarias": [luz.hex()] })).unwrap();
    assert_eq!((r["comando"].as_str(), r["circuito"].as_str()), (Some("a"), Some("1")));

    // Análise e roteamento em JSON.
    let a = s.execute("eletrica.analisar", &json!({})).unwrap();
    assert_eq!(a["circuitos"].as_array().unwrap().len(), 2);
    let r = s.execute("eletrica.rotear", &json!({ "quadro": "QDG" })).unwrap();
    assert_eq!(r["tag"], "QDG");
    let c2: &Value = r["linhas"].as_array().unwrap().iter().find(|l| l["circuito"]["id"] == "2").unwrap();
    // 6 m + 2,50 de descida + 1,30 do quadro.
    assert!((c2["comprimento"].as_f64().unwrap() - 9.8).abs() < 1e-9);

    // Configurações pelo motor, com ou sem desenho.
    let st = s.execute("eletrica.setup", &json!({})).unwrap();
    assert_eq!(st["sistema"], "trifasico380220");

    // Sobrescrita e desfazer.
    s.execute("eletrica.sobrescrever", &json!({ "circuito": "2", "disjuntor": 16 })).unwrap();
    let r = s.execute("eletrica.rotear", &json!({})).unwrap();
    assert_eq!(r["linhas"].as_array().unwrap().iter().find(|l| l["circuito"]["id"] == "2").unwrap()["escolha"]["disjuntor"], 16.0);
    s.undo().unwrap();

    // Ajustar fiação: marca os trechos; rodar de novo refaz sem duplicar; limpar apaga.
    let r = s.execute("elefiacao", &json!({})).unwrap();
    let marcas = r["quadros"][0]["marcas"].as_u64().unwrap();
    assert!(marcas > 0, "{r}");
    assert_eq!(r["quadros"][0]["trechos"], 3);
    let entidades = s.doc().unwrap().model.len();
    s.execute("elefiacao", &json!({ "quadro": "QDG" })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), entidades);
    let r = s.execute("elefiacao", &json!({ "limpar": true })).unwrap();
    assert_eq!(r["quadros"][0]["apagadas"].as_u64(), Some(marcas));

    // ELEFIACAO digitado: Enter ajusta todos os quadros; um passo de desfazer.
    s.start("elefiacao").unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.len(), entidades);
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().model.len() as u64, entidades as u64 - marcas);
}

#[test]
fn comandos_eletricos_sem_quadro_dao_erro_e_nao_travam() {
    let mut s = Session::new();
    s.open_drawing(Drawing::new_metric(), "Vazio", None);
    assert!(s.execute("eletrica.rotear", &json!({})).is_err());
    assert!(s.execute("elefiacao", &json!({})).is_err());
    assert!(s.execute("eletrica.sobrescrever", &json!({ "circuito": "1" })).is_err());
    assert!(s.execute("eletrica.setup", &json!({ "sistema": 42 })).is_err());
    assert!(s.execute("eletrica.analisar", &json!({})).unwrap()["pontos"].as_array().unwrap().is_empty());
}

#[test]
fn quadro_de_cargas_e_isolacao_por_comandos() {
    let mut s = Session::new();
    let (d, [qd, luz, _int, tug]) = planta();
    s.open_drawing(d, "Planta", None);
    s.execute("elecircuito", &json!({ "quadro": qd.hex(), "pontos": [luz.hex()] })).unwrap();
    s.execute("elecircuito", &json!({ "quadro": qd.hex(), "pontos": [tug.hex()] })).unwrap();

    // Criar um circuito vazio com descrição; o próximo número livre já conta com ele.
    let r = s.execute("eletrica.circuito.criar", &json!({ "descricao": "Chuveiro" })).unwrap();
    assert_eq!(r["circuito"], "3");
    assert!(s.execute("eletrica.circuito.criar", &json!({ "circuito": "3" })).is_err(), "já existe");
    s.execute("eletrica.circuito.descrever", &json!({ "circuito": 2, "descricao": "TUG" })).unwrap();
    let cargas = s.execute("eletrica.cargas", &json!({})).unwrap();
    let ids: Vec<&str> = cargas.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["1", "2", "3"]);
    assert_eq!(cargas[2]["descricao"], "Chuveiro");
    assert!(cargas[2]["circuito"].is_null());

    // Adicionar pontos a um circuito existente: o alvo leva o ELECIRCUITO direto aos pontos.
    s.execute("eletrica.circuito.alvo", &json!({ "circuito": "3" })).unwrap();
    s.start("elecircuito").unwrap();
    s.input(Input::Pick(vec![tug])).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(circuito(&s, tug).as_deref(), Some("3"));

    // Juntar o 3 no 1 e excluir o 1.
    let r = s.execute("eletrica.circuito.juntar", &json!({ "de": "3", "para": "1" })).unwrap();
    assert_eq!(r["pontos"], 1);
    assert_eq!(circuito(&s, tug).as_deref(), Some("1"));
    let r = s.execute("eletrica.circuito.excluir", &json!({ "circuito": "1" })).unwrap();
    assert_eq!(r["pontos"], 2);
    assert_eq!(circuito(&s, tug), None);
    s.undo().unwrap();
    assert_eq!(circuito(&s, tug).as_deref(), Some("1"));

    // Isolação do quadro: aparece no roteamento; inválida dá erro; null volta ao padrão.
    s.execute("eletrica.quadro", &json!({ "isolacao": "XLPE" })).unwrap();
    assert_eq!(s.execute("eletrica.rotear", &json!({})).unwrap()["isolacao"], "xlpe");
    assert!(s.execute("eletrica.quadro", &json!({ "isolacao": "ouro" })).is_err());
    s.execute("eletrica.quadro", &json!({ "isolacao": null })).unwrap();
    assert_eq!(s.execute("eletrica.rotear", &json!({})).unwrap()["isolacao"], "pvc");

    // A fiação dimensiona o eletroduto de cada trecho.
    let r = s.execute("elefiacao", &json!({})).unwrap();
    assert!(r["quadros"][0]["marcas"].as_u64().unwrap() > 0);
    let textos: Vec<String> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
    assert!(textos.iter().any(|t| t == "%%c20"), "{textos:?}");
}

/// Um bloco de tomada com atributos na definição, como os do template.
fn bloco_tomada(d: &mut Drawing) {
    let mut b = Block::new("TOM-SIM-BAIXA");
    for (tag, valor, constante) in [("TIPO_PONTO", "TUG", true), ("POTENCIA", "100VA", false), ("CIRCUITO", "", false)] {
        let mut a = atributo(tag, valor);
        a.constant = constante;
        b.entities.push(cadcraft_engine::doc::Entity::new(d.new_handle(), EntityKind::AttDef(a)));
    }
    d.blocks.insert("TOM-SIM-BAIXA".into(), Arc::new(b));
}

#[test]
fn eletroduto_em_arco_por_comandos() {
    let mut s = Session::new();
    let (mut d, [qd, luz, int, tug]) = planta();
    // Sem os eletrodutos retos da planta: só os em arco.
    let retos: Vec<Handle> = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).map(|e| e.handle).collect();
    for h in retos {
        d.model.remove(h);
    }
    s.open_drawing(d, "Planta", None);
    // JSON: do quadro à luz, no teto; e por coordenadas, no piso.
    let r = s.execute("eleeletroduto", &json!({ "de": qd.hex(), "para": luz.hex() })).unwrap();
    let h = Handle::parse_hex(r["eletroduto"].as_str().unwrap()).unwrap();
    assert!(matches!(s.doc().unwrap().model.get(h).unwrap().kind, EntityKind::Spline(_)));
    s.execute("eleeletroduto", &json!({ "de": [10.0, 10.0], "para": [12.0, 10.0], "piso": true, "flecha": 0 })).unwrap();
    assert!(s.execute("eleeletroduto", &json!({ "de": qd.hex(), "para": qd.hex() })).is_err());
    assert!(s.execute("eleeletroduto", &json!({ "de": qd.hex() })).is_err());
    let camadas: Vec<String> =
        s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Spline(_))).map(|e| e.common.layer.clone()).collect();
    assert_eq!(camadas, ["ELE-ELETRODUTO-PVC-MEDIA-TETO", "ELE-ELETRODUTO-PVC-MEDIA-PISO"]);

    // Interativo: luz, interruptor e tomada em sequência; Inverter muda o lado do próximo arco;
    // um passo de desfazer.
    let antes = s.doc().unwrap().model.len();
    s.start("eleeletroduto").unwrap();
    s.input(Input::Pick(vec![luz])).unwrap();
    s.input(Input::Pick(vec![int])).unwrap();
    s.input(Input::Keyword("Inverter".into())).unwrap();
    s.input(Input::Pick(vec![tug])).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.len(), antes + 2);
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().model.len(), antes);
}

#[test]
fn inserir_e_excluir_pontos_por_comandos() {
    let mut s = Session::new();
    let (mut d, _) = planta();
    bloco_tomada(&mut d);
    s.open_drawing(d, "Planta", None);
    let blocos = s.execute("eletrica.blocos", &json!({})).unwrap();
    let nomes: Vec<&str> = blocos.as_array().unwrap().iter().map(|b| b["nome"].as_str().unwrap()).collect();
    assert!(nomes.contains(&"TOM-SIM-BAIXA"), "{blocos}");
    // JSON: duas tomadas de 600 VA no circuito 4.
    let r = s
        .execute(
            "eleponto",
            &json!({ "bloco": "TOM-SIM-BAIXA", "pontos": [[1.0, 2.0], [2.0, 2.0]], "valores": { "POTENCIA": "600VA", "CIRCUITO": 4 } }),
        )
        .unwrap();
    let novos: Vec<String> = r["pontos"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
    assert_eq!(novos.len(), 2);
    let a = s.execute("eletrica.analisar", &json!({})).unwrap();
    let c4 = a["circuitos"].as_array().unwrap().iter().find(|c| c["id"] == "4").unwrap();
    assert_eq!(c4["potencia"].as_f64(), Some(1200.0));
    assert!(s.execute("eleponto", &json!({ "bloco": "NENHUM", "pontos": [[0, 0]] })).is_err());
    assert!(s.execute("eleponto", &json!({ "bloco": "TOM-SIM-BAIXA", "pontos": [[0, 0]], "valores": { "TIPO_PONTO": "TUE" } })).is_err());

    // Interativo com o modelo escolhido antes (a janela Inserir pontos): dois cliques e Enter.
    s.execute("eletrica.ponto.modelo", &json!({ "bloco": "tom-sim-baixa", "rotacao": 90, "valores": { "CIRCUITO": "5" } })).unwrap();
    s.start("eleponto").unwrap();
    s.input(Input::Point(cadcraft_engine::doc::geom::Vec2::new(3.0, 3.0))).unwrap();
    s.input(Input::Point(cadcraft_engine::doc::geom::Vec2::new(4.0, 3.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    let a = s.execute("eletrica.analisar", &json!({})).unwrap();
    assert_eq!(a["pontos"].as_array().unwrap().iter().filter(|p| p["circuito"] == "5").count(), 2);
    assert!(s.execute("eletrica.ponto.modelo", &json!({ "bloco": "PORTA" })).is_err());

    // Excluir: os dois do circuito 4; um handle que não é ponto recusa tudo.
    let linha = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle.hex();
    assert!(s.execute("eletrica.ponto.excluir", &json!({ "pontos": [novos[0], linha] })).is_err());
    let r = s.execute("eletrica.ponto.excluir", &json!({ "pontos": novos })).unwrap();
    assert_eq!(r["excluidos"], 2);
    let a = s.execute("eletrica.analisar", &json!({})).unwrap();
    assert!(a["circuitos"].as_array().unwrap().iter().all(|c| c["id"] != "4"));
}

#[test]
fn propriedades_do_circuito_e_demanda_por_comandos() {
    let mut s = Session::new();
    let (d, [qd, luz, _int, tug]) = planta();
    s.open_drawing(d, "Planta", None);
    s.execute("elecircuito", &json!({ "quadro": qd.hex(), "pontos": [luz.hex()] })).unwrap();
    s.execute("elecircuito", &json!({ "quadro": qd.hex(), "pontos": [tug.hex()] })).unwrap();
    let linha = |s: &mut Session, id: &str| -> Value {
        let r = s.execute("eletrica.rotear", &json!({})).unwrap();
        r["linhas"].as_array().unwrap().iter().find(|l| l["circuito"]["id"] == id).unwrap().clone()
    };

    // Isolação só do circuito 2, num passo de desfazer; null volta à do quadro.
    s.execute("eletrica.circuito.definir", &json!({ "circuito": "2", "isolacao": "EPR" })).unwrap();
    assert_eq!(linha(&mut s, "2")["isolacao"], "epr");
    assert_eq!(linha(&mut s, "1")["isolacao"], "pvc");
    s.undo().unwrap();
    assert_eq!(linha(&mut s, "2")["isolacao"], "pvc");
    s.execute("eletrica.circuito.definir", &json!({ "circuito": "2", "isolacao": "XLPE" })).unwrap();
    s.execute("eletrica.circuito.definir", &json!({ "circuito": "2", "isolacao": null })).unwrap();
    assert_eq!(linha(&mut s, "2")["isolacao"], "pvc");
    // Tipo de carga e FP são atributos dos blocos: o comando recusa e não grava nada.
    assert!(s.execute("eletrica.circuito.definir", &json!({ "circuito": "2" })).is_err());
    assert!(s.execute("eletrica.circuito.definir", &json!({ "circuito": "2", "isolacao": "EPR", "fp": 0.8 })).is_err());
    assert!(s.execute("eletrica.circuito.definir", &json!({ "circuito": "2", "isolacao": "ouro" })).is_err());
    assert_eq!(linha(&mut s, "2")["isolacao"], "pvc");

    // TIPO_CARGA e FP no bloco da TUG: o quadro de cargas mostra o resultado.
    {
        let doc = s.doc_mut().unwrap();
        free_eletrica::editar::definir(doc, tug, free_eletrica::ponto::TAGS_CARGA, Some("RESERVA")).unwrap();
        free_eletrica::editar::definir(doc, tug, free_eletrica::ponto::TAGS_FP, Some("0,85")).unwrap();
    }
    let l2 = linha(&mut s, "2");
    assert_eq!((l2["tiposCarga"][0].as_str(), l2["fp"].as_f64()), (Some("reserva"), Some(0.85)));
    // A reserva sai da demanda: só a luz de 100 VA (100 %).
    let r = s.execute("eletrica.rotear", &json!({})).unwrap();
    assert_eq!(r["demanda"]["demandada"].as_f64(), Some(100.0));

    // Tabelas de demanda, também sem desenho.
    let t = s.execute("eletrica.demanda.tabelas", &json!({})).unwrap();
    assert_eq!(t[0]["id"], "equatorial-nt001-r07");
    assert!(t[0]["fonte"].as_str().unwrap().contains("NT.00001.EQTL"));
}
