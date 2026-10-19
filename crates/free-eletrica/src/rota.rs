// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Roteamento: a rede de eletrodutos do desenho como grafo, o caminho do quadro até cada ponto
//! e, dele, comprimentos (com subidas e descidas) e momentos elétricos para a queda de tensão.
//!
//! Premissas desta versão: os eletrodutos são linhas e polilinhas (arcos viram cordas) nas
//! camadas das configurações. Os de piso (camadas de piso das configurações) correm no chão, os
//! demais no teto, no pé-direito; cada ponto desce ou sobe do eletroduto até a sua altura de
//! instalação, e a passagem do piso ao teto num mesmo ponto soma um pé-direito. Linhas que se cruzam sem um vértice em comum não se ligam
//! (não há caixa ali); uma ponta que encosta no meio de outro trecho liga (derivação em T).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use cadcraft_doc::{Drawing, EntityKind, Handle};

use crate::ponto::{PontoEletrico, altura_padrao};
use crate::setup::Setup;

type P = [f64; 2];

fn dist(a: P, b: P) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Projeção de `p` no segmento `a`-`b`: (parâmetro de 0 a 1, distância).
fn projetar(p: P, a: P, b: P) -> (f64, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 <= 0.0 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    (t, dist(p, [a[0] + t * dx, a[1] + t * dy]))
}

fn em(a: P, b: P, t: f64) -> P {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
}

/// Como um ponto chega na rede: o nó e a distância horizontal até ele (m).
#[derive(Clone, Copy, Debug)]
pub struct Ligacao {
    pub no: usize,
    pub distancia: f64,
    /// Altura do eletroduto onde o ponto liga (m): o pé-direito no teto, 0 no piso.
    pub elevacao: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Rede {
    /// Nós, nas coordenadas do desenho.
    pub nos: Vec<P>,
    /// Altura de cada nó (m): o pé-direito no teto, 0 no piso. No mesmo lugar, um nó de teto e
    /// um de piso são nós diferentes, ligados por uma subida de um pé-direito.
    pub elevacao: Vec<f64>,
    /// Arestas: (nó, nó, comprimento em m).
    pub arestas: Vec<(usize, usize, f64)>,
    /// Arestas que vêm de uma curva (spline): aresta → (entidade, pedaço, pedaços). A fiação
    /// marca uma curva só no pedaço do meio.
    pub curvas: HashMap<usize, (Handle, usize, usize)>,
    adj: Vec<Vec<(usize, usize)>>,
    pub ligacoes: HashMap<Handle, Ligacao>,
    /// Trechos de eletroduto lidos do desenho.
    pub trechos: usize,
    /// Metros por unidade do desenho.
    pub metros: f64,
}

impl Rede {
    /// Monta a rede com os eletrodutos do espaço do modelo e liga os pontos a ela.
    pub fn montar(doc: &Drawing, setup: &Setup, pontos: &[PontoEletrico], metros: f64) -> Rede {
        let metros = if metros > 0.0 { metros } else { 1.0 };
        // (início, fim, no piso, curva: (entidade, pedaço, pedaços))
        let mut segs: Vec<(P, P, bool, Option<(Handle, usize, usize)>)> = Vec::new();
        for e in doc.model.iter() {
            if !setup.e_camada_eletroduto(&e.common.layer) {
                continue;
            }
            let piso = setup.e_camada_piso(&e.common.layer);
            match &e.kind {
                EntityKind::Line(l) => segs.push(([l.a.x, l.a.y], [l.b.x, l.b.y], piso, None)),
                EntityKind::LwPolyline(pl) => {
                    let v: Vec<P> = pl.vertices.iter().map(|v| [v.p.x, v.p.y]).collect();
                    segs.extend(v.windows(2).map(|w| (w[0], w[1], piso, None)));
                    if pl.closed && v.len() > 2 {
                        segs.push((v[v.len() - 1], v[0], piso, None));
                    }
                }
                // Curvas (o eletroduto em arco do ELEELETRODUTO): pedaços retos com até 5 mm
                // de desvio da curva.
                EntityKind::Spline(sp) => {
                    let v: Vec<P> = sp.tessellate(0.005 / metros).iter().map(|p| [p.x, p.y]).collect();
                    let n = v.len().saturating_sub(1);
                    segs.extend(v.windows(2).enumerate().map(|(i, w)| (w[0], w[1], piso, Some((e.handle, i, n)))));
                }
                _ => {}
            }
        }
        segs.retain(|(a, b, _, _)| dist(*a, *b) > 0.0);
        let segs_curva: Vec<Option<(Handle, usize, usize)>> = segs.iter().map(|s| s.3).collect();
        let segs: Vec<(P, P, bool)> = segs.into_iter().map(|(a, b, piso, _)| (a, b, piso)).collect();
        let pe = setup.pe_direito;
        // Tolerância de encontro de pontas: 1 cm. Raio de captura dos pontos: das configurações.
        let eps = 0.01 / metros;
        let raio = setup.raio_captura / metros;

        // Onde cada trecho é cortado: nas pontas, onde outra ponta encosta e onde um ponto liga.
        let mut cortes: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; segs.len()];
        for (i, (a, b, _)) in segs.iter().enumerate() {
            for (j, (c, d, _)) in segs.iter().enumerate() {
                if i == j {
                    continue;
                }
                for p in [*c, *d] {
                    let (t, dd) = projetar(p, *a, *b);
                    if dd <= eps && t > 0.0 && t < 1.0 {
                        cortes[i].push(t);
                    }
                }
            }
        }
        let mut ligar: Vec<(Handle, usize, f64, f64)> = Vec::new();
        for p in pontos {
            let melhor = segs
                .iter()
                .enumerate()
                .map(|(j, (a, b, _))| {
                    let (t, dd) = projetar(p.posicao, *a, *b);
                    (j, t, dd)
                })
                .filter(|(_, _, dd)| *dd <= raio)
                .min_by(|x, y| x.2.total_cmp(&y.2));
            if let Some((j, t, dd)) = melhor {
                cortes[j].push(t);
                ligar.push((p.handle, j, t, dd));
            }
        }

        let mut rede = Rede { trechos: segs.len(), metros, ..Rede::default() };
        let mut grade: HashMap<(i64, i64, bool), Vec<usize>> = HashMap::new();
        let celula = |p: P| ((p[0] / eps).round() as i64, (p[1] / eps).round() as i64);
        // O nó em `p` no nível dado (teto ou piso), criado se ainda não existe.
        let mut no = |p: P, piso: bool, rede: &mut Rede| -> usize {
            let c = celula(p);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(lista) = grade.get(&(c.0 + dx, c.1 + dy, piso)) {
                        for &n in lista {
                            if dist(rede.nos[n], p) <= eps {
                                return n;
                            }
                        }
                    }
                }
            }
            rede.nos.push(p);
            rede.elevacao.push(if piso { 0.0 } else { pe });
            rede.adj.push(Vec::new());
            let n = rede.nos.len() - 1;
            grade.entry((c.0, c.1, piso)).or_default().push(n);
            n
        };
        for (i, (a, b, piso)) in segs.iter().enumerate() {
            let mut ts = std::mem::take(&mut cortes[i]);
            ts.sort_by(f64::total_cmp);
            let nos: Vec<usize> = ts.iter().map(|t| no(em(*a, *b, *t), *piso, &mut rede)).collect();
            for w in nos.windows(2) {
                if w[0] != w[1] {
                    let comprimento = dist(rede.nos[w[0]], rede.nos[w[1]]) * metros;
                    rede.ligar(w[0], w[1], comprimento);
                    if let Some(c) = segs_curva.get(i).copied().flatten() {
                        rede.curvas.insert(rede.arestas.len().saturating_sub(1), c);
                    }
                }
            }
        }
        for (h, j, t, dd) in ligar {
            let (a, b, piso) = segs[j];
            let n = no(em(a, b, t), piso, &mut rede);
            rede.ligacoes.insert(h, Ligacao { no: n, distancia: dd * metros, elevacao: rede.elevacao[n] });
        }
        // Onde um eletroduto de piso encontra um de teto, o cabo sobe um pé-direito.
        let mut subidas = Vec::new();
        for (n, p) in rede.nos.iter().enumerate().filter(|(n, _)| rede.elevacao[*n] < pe) {
            let c = celula(*p);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &m in grade.get(&(c.0 + dx, c.1 + dy, false)).into_iter().flatten() {
                        if dist(rede.nos[m], *p) <= eps {
                            subidas.push((n, m));
                        }
                    }
                }
            }
        }
        for (n, m) in subidas {
            rede.ligar(n, m, pe);
        }
        rede
    }

    fn ligar(&mut self, a: usize, b: usize, comprimento: f64) {
        let e = self.arestas.len();
        self.arestas.push((a, b, comprimento));
        self.adj[a].push((b, e));
        self.adj[b].push((a, e));
    }

    /// Caminhos mínimos a partir de um nó (Dijkstra): distância (m) e a aresta de chegada.
    pub fn arvore(&self, origem: usize) -> Arvore {
        #[derive(PartialEq)]
        struct Item(f64, usize);
        impl Eq for Item {}
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> Ordering {
                o.0.total_cmp(&self.0)
            }
        }
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        let mut distancia = vec![f64::INFINITY; self.nos.len()];
        let mut chegada: Vec<Option<(usize, usize)>> = vec![None; self.nos.len()];
        let mut fila = BinaryHeap::new();
        if origem < self.nos.len() {
            distancia[origem] = 0.0;
            fila.push(Item(0.0, origem));
        }
        while let Some(Item(d, n)) = fila.pop() {
            if d > distancia[n] {
                continue;
            }
            for &(viz, e) in &self.adj[n] {
                let nd = d + self.arestas[e].2;
                if nd < distancia[viz] {
                    distancia[viz] = nd;
                    chegada[viz] = Some((n, e));
                    fila.push(Item(nd, viz));
                }
            }
        }
        Arvore { distancia, chegada }
    }
}

pub struct Arvore {
    pub distancia: Vec<f64>,
    chegada: Vec<Option<(usize, usize)>>,
}

impl Arvore {
    /// As arestas do caminho da origem até `no`. `None`: não há caminho.
    pub fn caminho(&self, mut no: usize) -> Option<Vec<usize>> {
        if !self.distancia.get(no)?.is_finite() {
            return None;
        }
        let mut arestas = Vec::new();
        while let Some((anterior, e)) = self.chegada[no] {
            arestas.push(e);
            no = anterior;
        }
        Some(arestas)
    }
}

/// O que o roteamento diz de um circuito.
#[derive(Clone, Debug, Default)]
pub struct RotaCircuito {
    /// Maior comprimento do quadro até um ponto, com subidas e descidas (m).
    pub comprimento: Option<f64>,
    /// Maior momento elétrico até um ponto de carga (A·m).
    pub momento: Option<f64>,
    /// Comprimento do eletroduto ocupado pelo circuito, com subidas e descidas (m), para
    /// o levantamento de cabo.
    pub percurso: f64,
    /// Arestas da rede que o circuito ocupa.
    pub arestas: HashSet<usize>,
    /// Pontos fora da rede: o comprimento deles é estimado (distância em L do quadro).
    pub fora_da_rede: Vec<Handle>,
}

/// Altura de instalação do ponto (m).
fn altura(p: &PontoEletrico, setup: &Setup) -> f64 {
    p.altura.unwrap_or_else(|| altura_padrao(p.tipo, Some(setup.pe_direito)))
}

/// Roteia os pontos de um circuito a partir do quadro. `corrente` dá a corrente de cada ponto.
pub fn rotear_circuito(
    rede: &Rede,
    arvore: Option<&Arvore>,
    quadro: &PontoEletrico,
    membros: &[&PontoEletrico],
    corrente: impl Fn(&PontoEletrico) -> f64,
    setup: &Setup,
) -> RotaCircuito {
    let pe = setup.pe_direito;
    // Fora da rede: desce do teto até a altura do ponto.
    let perna = |p: &PontoEletrico| (pe - altura(p, setup)).max(0.0);
    // Na rede: do eletroduto (teto ou piso) até a altura do ponto, mais a distância até ele.
    let perna_na_rede =
        |p: &PontoEletrico| rede.ligacoes.get(&p.handle).map_or_else(|| perna(p), |l| (l.elevacao - altura(p, setup)).abs() + l.distancia);
    let lig_q = rede.ligacoes.get(&quadro.handle);
    let perna_q = perna_na_rede(quadro);
    let total: f64 = membros.iter().map(|p| corrente(p)).sum();

    let mut rota = RotaCircuito { percurso: perna_q, ..RotaCircuito::default() };
    let mut descida: HashMap<usize, f64> = HashMap::new();
    let mut roteados: Vec<(&PontoEletrico, Vec<usize>, f64, f64)> = Vec::new(); // ponto, caminho, distância na rede, perna
    let mut estimados: Vec<(&PontoEletrico, f64)> = Vec::new(); // ponto, comprimento
    for p in membros {
        let perna_p = perna_na_rede(p);
        let caminho = match (arvore, lig_q, rede.ligacoes.get(&p.handle)) {
            (Some(a), Some(_), Some(l)) => a.caminho(l.no).map(|c| (c, a.distancia[l.no])),
            _ => None,
        };
        match caminho {
            Some((c, d)) => {
                for e in &c {
                    *descida.entry(*e).or_default() += corrente(p);
                }
                rota.comprimento = Some(rota.comprimento.unwrap_or(0.0).max(d + perna_p + perna_q));
                rota.percurso += perna_p;
                rota.arestas.extend(c.iter().copied());
                roteados.push((p, c, d, perna_p));
            }
            None => {
                let l =
                    ((p.posicao[0] - quadro.posicao[0]).abs() + (p.posicao[1] - quadro.posicao[1]).abs()) * rede.metros + perna(p) + perna(quadro);
                rota.comprimento = Some(rota.comprimento.unwrap_or(0.0).max(l));
                rota.percurso += l - perna(quadro);
                rota.fora_da_rede.push(p.handle);
                estimados.push((p, l));
            }
        }
    }
    rota.percurso += rota.arestas.iter().map(|e| rede.arestas[*e].2).sum::<f64>();
    // Momento até cada carga: cada trecho leva a corrente de tudo o que está depois dele.
    for (p, c, _, perna_p) in &roteados {
        if corrente(p) <= 0.0 {
            continue;
        }
        let m = c.iter().map(|e| descida[e] * rede.arestas[*e].2).sum::<f64>() + corrente(p) * perna_p + total * perna_q;
        rota.momento = Some(rota.momento.unwrap_or(0.0).max(m));
    }
    // Fora da rede: toda a corrente do circuito no comprimento estimado (lado seguro).
    for (p, l) in &estimados {
        if corrente(p) > 0.0 {
            rota.momento = Some(rota.momento.unwrap_or(0.0).max(total * l));
        }
    }
    rota
}
