// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Julio Ferreira. Free Elétrica: ver LICENSE e AVISO_LEGAL.md na raiz do repositorio.
//! Inserir eletroduto: uma spline em arco entre dois objetos (pontos elétricos, quadros, caixas),
//! na camada de eletroduto do teto ou do piso. O roteamento lê a curva em pedaços retos.

use cadcraft_doc::color::Color;
use cadcraft_doc::geom::{Spline, Vec2};
use cadcraft_doc::{Common, Drawing, EntityKind, Handle, Lineweight, Space};

/// Camadas do eletroduto (as do template).
pub const CAMADA_TETO: &str = "ELE-ELETRODUTO-PVC-MEDIA-TETO";
pub const CAMADA_PISO: &str = "ELE-ELETRODUTO-PVC-MEDIA-PISO";
/// Flecha padrão do arco: a distância do meio da corda ao arco, como fração da corda.
pub const FLECHA_PADRAO: f64 = 0.15;
/// A flecha vai até meia corda (meia circunferência).
pub const FLECHA_MAXIMA: f64 = 0.5;

type P = [f64; 2];

/// Onde um objeto se liga ao eletroduto: o ponto de inserção de um bloco ou o centro de um
/// círculo. `None` para outros objetos.
pub fn posicao(doc: &Drawing, h: Handle) -> Option<P> {
    match &doc.model.get(h)?.kind {
        EntityKind::Insert(i) => Some([i.insert.x, i.insert.y]),
        EntityKind::Circle(c) => Some([c.center.x, c.center.y]),
        _ => None,
    }
}

/// Pontos de um arco de circunferência de `a` a `b` (ambos incluídos). `flecha` é fração da
/// corda: positiva curva para a esquerda de quem vai de `a` para `b`, negativa para a direita,
/// zero é reta. Limitada a meia corda.
pub fn pontos_do_arco(a: P, b: P, flecha: f64, n: usize) -> Vec<P> {
    let n = n.max(2);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let corda = dx.hypot(dy);
    let reta = |t: f64| [a[0] + dx * t, a[1] + dy * t];
    let flecha = if flecha.is_finite() { flecha.clamp(-FLECHA_MAXIMA, FLECHA_MAXIMA) } else { 0.0 };
    let h = flecha * corda;
    if corda <= 0.0 || h.abs() < 1e-9 * corda.max(1.0) {
        return (0..n).map(|i| reta(i as f64 / (n - 1) as f64)).collect();
    }
    let u = [dx / corda, dy / corda];
    let lado = h.signum();
    // Do meio da corda para o lado do arco.
    let topo = [-u[1] * lado, u[0] * lado];
    let r = (corda * corda / 4.0 + h * h) / (2.0 * h.abs());
    let meio = reta(0.5);
    let centro = [meio[0] - topo[0] * (r - h.abs()), meio[1] - topo[1] * (r - h.abs())];
    let phi = (corda / (2.0 * r)).clamp(-1.0, 1.0).asin();
    (0..n)
        .map(|i| {
            let alfa = -phi + 2.0 * phi * i as f64 / (n - 1) as f64;
            let (s, c) = alfa.sin_cos();
            [centro[0] + r * (topo[0] * c + u[0] * s), centro[1] + r * (topo[1] * c + u[1] * s)]
        })
        .collect()
}

/// Cria o eletroduto em arco de `a` a `b`, no teto ou no piso. A camada é criada com a cor do
/// template quando ainda não existe.
pub fn inserir(doc: &mut Drawing, a: P, b: P, piso: bool, flecha: f64) -> Result<Handle, String> {
    if !(a.iter().chain(b.iter()).all(|v| v.is_finite())) {
        return Err("coordenadas inválidas".into());
    }
    if (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-9 {
        return Err("os dois objetos estão no mesmo lugar".into());
    }
    let camada = if piso { CAMADA_PISO } else { CAMADA_TETO };
    if doc.ensure_layer(camada)
        && let Some(l) = doc.layer_mut(camada)
    {
        l.color = Color::Index(if piso { 6 } else { 4 });
        l.lineweight = Lineweight::Mm100(30);
        l.description = if piso { "Eletroduto PVC linha média embutido no piso" } else { "Eletroduto PVC linha média embutido no teto/laje" }.into();
    }
    let fit: Vec<Vec2> = pontos_do_arco(a, b, flecha, 5).into_iter().map(|p| Vec2::new(p[0], p[1])).collect();
    let spline = Spline::from_fit_points(&fit);
    doc.add(&Space::Model, Common { layer: camada.into(), ..Common::default() }, EntityKind::Spline(spline)).map_err(|e| e.to_string())
}
