//! Window backdrop: a radial navy glow and a faint, static network mesh behind the glass cards.

use std::sync::OnceLock;

use eframe::egui::{epaint::Vertex, Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke};

use super::theme::{c, lerp_color};

const SEED: u64 = 0x5eed_a17a;
const NODES: usize = 18;

/// Nodes in unit space (biased toward the lower part) and edges to each node's nearest neighbours.
pub fn mesh_graph(seed: u64, n: usize) -> (Vec<Pos2>, Vec<(usize, usize)>) {
    let mut state = seed.max(1);
    let mut rand = move || {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32
    };
    let nodes: Vec<Pos2> = (0..n).map(|_| Pos2::new(rand(), 0.4 + 0.6 * rand())).collect();
    let mut edges = Vec::new();
    for (i, a) in nodes.iter().enumerate() {
        let mut near: Vec<usize> = (0..n).filter(|&j| j != i).collect();
        near.sort_by(|&x, &y| a.distance_sq(nodes[x]).total_cmp(&a.distance_sq(nodes[y])));
        let k = 2 + (i % 2);
        for &j in near.iter().take(k) {
            let e = (i.min(j), i.max(j));
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
    }
    (nodes, edges)
}

fn graph() -> &'static (Vec<Pos2>, Vec<(usize, usize)>) {
    static G: OnceLock<(Vec<Pos2>, Vec<(usize, usize)>)> = OnceLock::new();
    G.get_or_init(|| mesh_graph(SEED, NODES))
}

/// Paints the gradient and the mesh. Call on a background layer before any panel content.
pub fn paint_backdrop(p: &Painter, rect: Rect) {
    let p = p.with_clip_rect(rect);
    let (glow, mid, deep) = (c::bg_glow(), c::bg_mid(), c::bg_deep());
    let centre = Pos2::new(rect.left() + rect.width() * 0.65, rect.top() - rect.height() * 0.1);
    let radius = [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()]
        .iter()
        .map(|q| q.distance(centre))
        .fold(0.0, f32::max);
    let colour = |t: f32| if t < 0.45 { lerp_color(glow, mid, t / 0.45) } else { lerp_color(mid, deep, (t - 0.45) / 0.55) };

    // radial gradient as a polar grid of vertex-coloured quads
    const SEGS: u32 = 64;
    const RINGS: u32 = 8;
    let mut mesh = Mesh::default();
    for r in 0..=RINGS {
        let t = r as f32 / RINGS as f32;
        for s in 0..=SEGS {
            let a = s as f32 / SEGS as f32 * std::f32::consts::TAU;
            let pos = centre + eframe::egui::vec2(a.cos(), a.sin()) * radius * t;
            mesh.vertices.push(Vertex { pos, uv: eframe::egui::epaint::WHITE_UV, color: colour(t) });
        }
    }
    let row = SEGS + 1;
    for r in 0..RINGS {
        for s in 0..SEGS {
            let i = r * row + s;
            mesh.add_triangle(i, i + 1, i + row);
            mesh.add_triangle(i + 1, i + row + 1, i + row);
        }
    }
    p.add(Shape::mesh(mesh));

    let (nodes, edges) = graph();
    let at = |q: Pos2| Pos2::new(rect.left() + q.x * rect.width(), rect.top() + q.y * rect.height());
    let line = Stroke::new(0.8_f32, c::mesh_line());
    for &(a, b) in edges {
        p.line_segment([at(nodes[a]), at(nodes[b])], line);
    }
    let node = c::mesh_node();
    let halo = Color32::from_rgba_unmultiplied(node.r(), node.g(), node.b(), node.a() / 4);
    for (i, q) in nodes.iter().enumerate() {
        let r = 1.3 + (i % 3) as f32 * 0.5;
        p.circle_filled(at(*q), r * 2.2, halo);
        p.circle_filled(at(*q), r, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_is_deterministic() {
        assert_eq!(mesh_graph(7, 28), mesh_graph(7, 28));
    }

    #[test]
    fn mesh_nodes_in_unit_square() {
        let (nodes, _) = mesh_graph(7, 28);
        assert_eq!(nodes.len(), 28);
        assert!(nodes.iter().all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)));
    }

    #[test]
    fn mesh_edges_valid() {
        let n = 28;
        let (_, edges) = mesh_graph(7, n);
        let mut seen = std::collections::HashSet::new();
        let mut degree = vec![0; n];
        for &(a, b) in &edges {
            assert!(a != b && a < n && b < n);
            assert!(seen.insert((a.min(b), a.max(b))), "duplicate edge {a}-{b}");
            degree[a] += 1;
            degree[b] += 1;
        }
        assert!(degree.iter().all(|&d| d > 0));
    }
}
