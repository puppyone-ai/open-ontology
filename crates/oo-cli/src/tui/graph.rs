//! The ontology graph as a deterministic pure function: adjudication + view →
//! layout → character grid. No terminal, no clock, no randomness, so every
//! layout is snapshot-testable and the TUI, `--format ascii` and the samples
//! render the same bytes.
//!
//! Layout: the hub (highest degree) sits in the centre row; BFS layers fan out
//! above and below it, so every edge joins adjacent rows, the same row, or
//! (when a row wraps or two sides meet) travels down a right-hand gutter lane.
//! Edges are orthogonal, routed in channels between rows with one track per
//! overlapping segment; cardinality markers (`N`/`1`) sit next to each box.
//! Rejected relations are dashed and carry their missing evidence.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::adjudication::{Adjudication, Item, magnitude, missing_evidence, role, scope_columns};
use super::theme::{Tone, char_width, fit, width};

/// Above this many entities the graph opens in focus mode.
pub const FOCUS_THRESHOLD: usize = 25;
const GAP: usize = 4;
const BOX_H: usize = 4;
const LABEL_MAX: usize = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    One,
    Many,
}

impl Side {
    fn mark(self) -> char {
        match self {
            Side::One => '1',
            Side::Many => 'N',
        }
    }
}

/// `(from side, to side)` for a cardinality string.
pub fn sides(cardinality: &str) -> (Side, Side) {
    match cardinality {
        "one_to_one" => (Side::One, Side::One),
        "one_to_many" => (Side::One, Side::Many),
        "many_to_many" => (Side::Many, Side::Many),
        _ => (Side::Many, Side::One),
    }
}

/// Text glyph for the relations list, read from the `from` entity.
pub fn arrow(cardinality: &str, adopted: bool) -> String {
    let line = if adopted { '─' } else { '┄' };
    let (a, b) = sides(cardinality);
    let left = if a == Side::Many { '>' } else { '1' };
    let right = if b == Side::Many { '<' } else { '1' };
    format!("{left}{line}{right}")
}

#[derive(Clone, Debug, Default)]
pub struct View<'a> {
    /// Highlighted entity id (heavy frame).
    pub selected: Option<&'a str>,
    /// Focus entity: render it with its one-hop neighbours only.
    pub focus: Option<&'a str>,
    /// Wrap rows to fit this many columns.
    pub max_width: Option<usize>,
    /// Append the rejected-relation legend.
    pub legend: bool,
}

/// A rendered graph. `cells` holds `'\0'` in the second column of wide glyphs.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Vec<(char, Tone)>>,
    /// `(entity id, x, y, w, h)` for every drawn box.
    pub boxes: Vec<(String, usize, usize, usize, usize)>,
    /// Entities in the drawn subgraph, focus first.
    pub shown: Vec<String>,
    pub focus: Option<String>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Canvas {
            width,
            height,
            cells: vec![vec![(' ', Tone::Faint); width]; height],
            boxes: Vec::new(),
            shown: Vec::new(),
            focus: None,
        }
    }

    fn put(&mut self, x: usize, y: usize, c: char, tone: Tone) {
        if y < self.height && x < self.width {
            self.cells[y][x] = (c, tone);
        }
    }

    /// Write text; returns columns used. Never splits a wide glyph.
    fn text(&mut self, x: usize, y: usize, s: &str, tone: Tone) -> usize {
        let mut cx = x;
        for c in s.chars() {
            let w = char_width(c);
            if w == 0 {
                continue;
            }
            if cx + w > self.width {
                break;
            }
            self.put(cx, y, c, tone);
            for k in 1..w {
                self.put(cx + k, y, '\0', tone);
            }
            cx += w;
        }
        cx - x
    }

    pub fn lines(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                let s: String = row.iter().filter(|c| c.0 != '\0').map(|c| c.0).collect();
                s.trim_end().to_owned()
            })
            .collect()
    }

    /// Colour layer: one tone letter per column, `.` for blank cells.
    #[cfg(test)]
    pub fn tones(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                let s: String = row
                    .iter()
                    .map(|(c, t)| if *c == ' ' { '.' } else { t.code() })
                    .collect();
                s.trim_end_matches('.').to_owned()
            })
            .collect()
    }
}

struct Node {
    id: String,
    label: String,
    sub: String,
    self_loop: bool,
}

struct Link {
    a: usize,
    b: usize,
    sides: (Side, Side),
    adopted: bool,
    /// Short tag on a rejected edge: the coverage figure or the structural flaw.
    short: String,
    /// Role when one child has several edges to the same parent.
    role: Option<String>,
}

/// Short reason drawn on a rejected edge: `scope valid_from`, `fan-out ×3`,
/// `collision`, else the first figure of the missing evidence.
fn short_missing(item: &Item, note: &str) -> String {
    let scope = scope_columns(item);
    let fanout = item
        .evidence
        .get("max_fanout")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if !scope.is_empty() {
        format!("scope {}", scope.join(","))
    } else if fanout > 1 {
        format!("fan-out ×{fanout}")
    } else if item.evidence.get("parent_unique") == Some(&serde_json::Value::Bool(false)) {
        "not unique".into()
    } else if item
        .evidence
        .get("normalized_collisions")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|n| n > 0)
    {
        "collision".into()
    } else {
        note.split_whitespace()
            .find(|w| w.parse::<f64>().is_ok())
            .unwrap_or("?")
            .to_owned()
    }
}

/// Pick the focus entity: explicit, else the hub.
fn hub(adj: &Adjudication) -> Option<String> {
    let mut degree: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &adj.graph.edges {
        if e.from != e.to {
            *degree.entry(&e.from).or_default() += 1;
            *degree.entry(&e.to).or_default() += 1;
        }
    }
    adj.graph
        .entities
        .iter()
        .max_by(|x, y| {
            let dx = degree.get(x.id.as_str()).copied().unwrap_or(0);
            let dy = degree.get(y.id.as_str()).copied().unwrap_or(0);
            dx.cmp(&dy)
                .then(x.rows.unwrap_or(0).cmp(&y.rows.unwrap_or(0)))
                .then(y.id.cmp(&x.id))
        })
        .map(|e| e.id.clone())
}

/// Entities in focus-navigation order: focus mode cycles through this list.
pub fn focus_order(adj: &Adjudication) -> Vec<String> {
    let mut degree: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &adj.graph.edges {
        *degree.entry(&e.from).or_default() += 1;
        *degree.entry(&e.to).or_default() += 1;
    }
    let mut ids: Vec<&str> = adj.graph.entities.iter().map(|e| e.id.as_str()).collect();
    ids.sort_by(|a, b| {
        degree
            .get(b)
            .unwrap_or(&0)
            .cmp(degree.get(a).unwrap_or(&0))
            .then(a.cmp(b))
    });
    ids.into_iter().map(str::to_owned).collect()
}

/// One-hop neighbours of `id`, in stable order.
pub fn neighbours(adj: &Adjudication, id: &str) -> Vec<String> {
    let set: BTreeSet<&str> = adj
        .graph
        .edges
        .iter()
        .filter_map(|e| {
            if e.from == id && e.to != id {
                Some(e.to.as_str())
            } else if e.to == id && e.from != id {
                Some(e.from.as_str())
            } else {
                None
            }
        })
        .collect();
    set.into_iter().map(str::to_owned).collect()
}

pub fn render(adj: &Adjudication, view: &View) -> Canvas {
    let focus = view.focus.map(str::to_owned).or_else(|| {
        (adj.graph.entities.len() > FOCUS_THRESHOLD)
            .then(|| hub(adj))
            .flatten()
    });
    let shown: Vec<String> = match &focus {
        Some(f) => {
            let mut v = vec![f.clone()];
            v.extend(neighbours(adj, f));
            v
        }
        None => adj.graph.entities.iter().map(|e| e.id.clone()).collect(),
    };
    let index: BTreeMap<&str, usize> = shown
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let mut nodes: Vec<Node> = shown
        .iter()
        .map(|id| {
            let e = adj.graph.entities.iter().find(|e| &e.id == id);
            let label = e.map_or(id.clone(), |e| e.label.clone());
            let key = e.map_or(String::new(), |e| e.key.join(","));
            let sub = match e.and_then(|e| e.rows) {
                Some(rows) if !key.is_empty() => format!("{key} · {}", magnitude(rows)),
                Some(rows) => magnitude(rows),
                None => key,
            };
            Node {
                id: id.clone(),
                label: fit(&label, LABEL_MAX),
                sub: fit(&sub, LABEL_MAX),
                self_loop: false,
            }
        })
        .collect();
    let mut links = Vec::new();
    for e in &adj.graph.edges {
        let (Some(&a), Some(&b)) = (index.get(e.from.as_str()), index.get(e.to.as_str())) else {
            continue;
        };
        // Focus mode keeps only edges touching the focus.
        if let Some(f) = &focus
            && e.from != *f
            && e.to != *f
        {
            continue;
        }
        let item = adj.items.iter().find(|i| i.id == e.relation);
        let short = item
            .map(|i| short_missing(i, &missing_evidence(i, &adj.thresholds)))
            .unwrap_or_else(|| "?".into());
        if a == b {
            nodes[a].self_loop = true;
        }
        links.push(Link {
            a,
            b,
            sides: sides(&e.cardinality),
            adopted: e.adopted,
            short,
            role: item.and_then(role).map(str::to_owned),
        });
    }
    let mut canvas = draw(&nodes, &links, view, focus.as_deref());
    if view.legend {
        legend(&mut canvas, adj, &index, focus.as_deref());
    }
    drop(index);
    canvas.shown = shown;
    canvas.focus = focus;
    canvas
}

fn footprint(n: &Node) -> usize {
    width(&n.label).max(width(&n.sub)) + 4 + if n.self_loop { 3 } else { 0 }
}

fn box_w(n: &Node) -> usize {
    width(&n.label).max(width(&n.sub)).max(4) + 4
}

fn draw(nodes: &[Node], links: &[Link], view: &View, focus: Option<&str>) -> Canvas {
    let n = nodes.len();
    if n == 0 {
        return Canvas::new(0, 0);
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for l in links.iter().filter(|l| l.a != l.b) {
        adj[l.a].push(l.b);
        adj[l.b].push(l.a);
    }
    for list in &mut adj {
        list.sort_unstable();
        list.dedup();
    }
    let degree: Vec<usize> = adj.iter().map(Vec::len).collect();
    let by_rank = |list: &mut Vec<usize>| {
        list.sort_by(|&x, &y| {
            degree[y]
                .cmp(&degree[x])
                .then(nodes[x].id.cmp(&nodes[y].id))
        });
    };
    // Roots: the focus (index 0 in focus mode) or the highest-degree node of
    // each component.
    let mut order: Vec<usize> = (0..n).collect();
    by_rank(&mut order);
    if focus.is_some() {
        order.retain(|&i| i != 0);
        order.insert(0, 0);
    }
    // (layer, side, parent) per node. side: -1 above, 0 centre, 1 below.
    let mut layer = vec![usize::MAX; n];
    let mut side = vec![0i8; n];
    let mut base = 0;
    for (component, &root) in order.iter().enumerate() {
        if layer[root] != usize::MAX {
            continue;
        }
        let first = component == 0;
        layer[root] = base;
        side[root] = if first { 0 } else { 1 };
        let mut queue = VecDeque::from([root]);
        let mut seen_first_layer = 0usize;
        let mut deepest = base;
        while let Some(u) = queue.pop_front() {
            let mut next = adj[u].clone();
            by_rank(&mut next);
            for v in next {
                if layer[v] != usize::MAX {
                    continue;
                }
                layer[v] = layer[u] + 1;
                deepest = deepest.max(layer[v]);
                side[v] = if !first {
                    1
                } else if u == root {
                    seen_first_layer += 1;
                    if seen_first_layer % 2 == 1 { -1 } else { 1 }
                } else {
                    side[u]
                };
                queue.push_back(v);
            }
        }
        base = deepest + 1;
    }
    // Groups in top-to-bottom order.
    let mut groups: BTreeMap<(i64, usize), Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let key = match side[i] {
            -1 => (-(layer[i] as i64), 0),
            0 => (0, 0),
            _ => (layer[i] as i64, 0),
        };
        groups.entry(key).or_default().push(i);
    }
    // Barycentric ordering, processed outward from the centre row.
    let mut keys: Vec<(i64, usize)> = groups.keys().copied().collect();
    keys.sort_by_key(|k| (k.0.unsigned_abs(), k.0));
    let mut position: Vec<Option<f64>> = vec![None; n];
    for key in &keys {
        let group = groups.get_mut(key).unwrap_or_else(|| unreachable!());
        let bary = |i: usize, position: &[Option<f64>]| {
            let placed: Vec<f64> = adj[i].iter().filter_map(|&j| position[j]).collect();
            if placed.is_empty() {
                0.5
            } else {
                placed.iter().sum::<f64>() / placed.len() as f64
            }
        };
        let mut scored: Vec<(f64, usize)> =
            group.iter().map(|&i| (bary(i, &position), i)).collect();
        scored.sort_by(|x, y| {
            x.0.partial_cmp(&y.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(degree[y.1].cmp(&degree[x.1]))
                .then(nodes[x.1].id.cmp(&nodes[y.1].id))
        });
        *group = scored.iter().map(|s| s.1).collect();
        let len = group.len() as f64;
        for (k, &i) in group.iter().enumerate() {
            position[i] = Some((k as f64 + 0.5) / len);
        }
    }
    // Wrap rows to the width budget (gutter lanes need a few columns).
    let budget = view
        .max_width
        .unwrap_or(usize::MAX)
        .saturating_sub(8)
        .max(1);
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut ordered_keys: Vec<(i64, usize)> = groups.keys().copied().collect();
    ordered_keys.sort();
    for key in ordered_keys {
        let mut row = Vec::new();
        let mut used = 0;
        for &i in &groups[&key] {
            let need = footprint(&nodes[i]) + if row.is_empty() { 0 } else { GAP };
            if !row.is_empty() && used + need > budget {
                rows.push(std::mem::take(&mut row));
                used = 0;
            }
            used += footprint(&nodes[i]) + if row.is_empty() { 0 } else { GAP };
            row.push(i);
        }
        rows.push(row);
    }
    let row_of: Vec<usize> = {
        let mut r = vec![0; n];
        for (k, row) in rows.iter().enumerate() {
            for &i in row {
                r[i] = k;
            }
        }
        r
    };
    let row_w = |row: &Vec<usize>| -> usize {
        row.iter().map(|&i| footprint(&nodes[i])).sum::<usize>() + GAP * row.len().saturating_sub(1)
    };
    let inner = rows.iter().map(row_w).max().unwrap_or(0) + 2;
    let mut x = vec![0usize; n];
    for row in &rows {
        let mut cx = 1 + (inner - 2 - row_w(row)) / 2;
        for &i in row {
            x[i] = cx;
            cx += footprint(&nodes[i]) + GAP;
        }
    }
    let center = |i: usize| x[i] + box_w(&nodes[i]) / 2;

    // Routes: (upper, lower, kind). kind 0 adjacent, 1 same row, 2 long.
    struct Route {
        link: usize,
        u: usize,
        l: usize,
        kind: u8,
        lane: usize,
        pu: usize,
        pl: usize,
        t1: usize,
        t2: usize,
    }
    let mut routes: Vec<Route> = Vec::new();
    let mut lanes = 0;
    for (k, link) in links.iter().enumerate() {
        if link.a == link.b {
            continue;
        }
        let (u, l) = if (row_of[link.a], x[link.a]) <= (row_of[link.b], x[link.b]) {
            (link.a, link.b)
        } else {
            (link.b, link.a)
        };
        let kind = match row_of[l] - row_of[u] {
            0 => 1,
            1 => 0,
            _ => 2,
        };
        let lane = if kind == 2 {
            lanes += 1;
            lanes - 1
        } else {
            0
        };
        routes.push(Route {
            link: k,
            u,
            l,
            kind,
            lane,
            pu: 0,
            pl: 0,
            t1: 0,
            t2: 0,
        });
    }
    let gutter = |lane: usize| inner + 1 + 2 * lane;
    // Ports: bottom of upper; top of lower (bottom for same-row edges).
    // (node, bottom side) -> [(other end x, route, is upper end)]
    type Ports = BTreeMap<(usize, bool), Vec<(usize, usize, bool)>>;
    let mut ports: Ports = BTreeMap::new();
    for (r, route) in routes.iter().enumerate() {
        let target_u = if route.kind == 2 {
            gutter(route.lane)
        } else {
            center(route.l)
        };
        let target_l = if route.kind == 2 {
            gutter(route.lane)
        } else {
            center(route.u)
        };
        ports
            .entry((route.u, true))
            .or_default()
            .push((target_u, r, true));
        ports
            .entry((route.l, route.kind == 1))
            .or_default()
            .push((target_l, r, false));
    }
    // Bottom sides first; top sides then avoid columns already used in the
    // same channel so two edges never share a vertical run.
    let mut used: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut sides_order: Vec<(usize, bool)> = ports.keys().copied().collect();
    sides_order.sort_by_key(|&(node, bottom)| (!bottom, node));
    for key in sides_order {
        let (node, bottom) = key;
        let list = ports.get_mut(&key).unwrap_or_else(|| unreachable!());
        list.sort();
        let w = box_w(&nodes[node]) - 2;
        let k = list.len();
        let channel = if bottom {
            row_of[node]
        } else {
            row_of[node] - 1
        };
        let (lo, hi) = (x[node] + 1, x[node] + w);
        for (i, &(_, r, upper)) in list.iter().enumerate() {
            let ideal = x[node] + 1 + ((i + 1) * w) / (k + 1);
            let px = (0..w)
                .flat_map(|d| [ideal + d, ideal.wrapping_sub(d)])
                .find(|&c| (lo..=hi).contains(&c) && !used.contains(&(channel, c)))
                .unwrap_or(ideal);
            used.insert((channel, px));
            if upper {
                routes[r].pu = px;
            } else {
                routes[r].pl = px;
            }
        }
    }
    // Tracks per channel (channel r sits below row r).
    let mut segments: BTreeMap<usize, Vec<(usize, usize, usize, bool)>> = BTreeMap::new();
    for (r, route) in routes.iter().enumerate() {
        let ru = row_of[route.u];
        let rl = row_of[route.l];
        match route.kind {
            2 => {
                let g = gutter(route.lane);
                segments
                    .entry(ru)
                    .or_default()
                    .push((route.pu.min(g), route.pu.max(g), r, true));
                segments.entry(rl - 1).or_default().push((
                    route.pl.min(g),
                    route.pl.max(g),
                    r,
                    false,
                ));
            }
            _ => {
                segments.entry(ru).or_default().push((
                    route.pu.min(route.pl),
                    route.pu.max(route.pl),
                    r,
                    true,
                ));
            }
        }
    }
    let mut tracks = vec![0usize; rows.len()];
    for (channel, list) in &mut segments {
        list.sort();
        let mut ends: Vec<usize> = Vec::new();
        for &(a, b, r, first) in list.iter() {
            let t = match ends.iter().position(|&e| e + 1 < a) {
                Some(t) => t,
                None => {
                    ends.push(0);
                    ends.len() - 1
                }
            };
            ends[t] = b;
            if first {
                routes[r].t1 = t;
            } else {
                routes[r].t2 = t;
            }
        }
        tracks[*channel] = ends.len();
    }
    // Vertical positions.
    let mut row_y = Vec::with_capacity(rows.len());
    let mut channel_y = Vec::with_capacity(rows.len());
    let mut y = 0;
    for (r, _) in rows.iter().enumerate() {
        row_y.push(y);
        y += BOX_H;
        channel_y.push(y);
        let last = r + 1 == rows.len();
        if !last || tracks[r] > 0 {
            y += 2 + tracks[r].max(1);
        }
    }
    let cols = inner + 1 + 2 * lanes;
    let mut canvas = Canvas::new(cols, y);
    let track_y = |channel: usize, t: usize| channel_y[channel] + 1 + t;
    // Line layer: bits U=1 D=2 L=4 R=8, solid and dashed kept apart.
    let mut solid = vec![vec![0u8; cols]; y];
    let mut dashed = vec![vec![0u8; cols]; y];
    let mut markers: Vec<(usize, usize, char, bool, bool)> = Vec::new();
    // Routes touching the selected entity are drawn hot.
    let sel = view
        .selected
        .and_then(|id| nodes.iter().position(|n| n.id == id));
    let mut hot = vec![vec![false; cols]; y];
    let mut near = vec![false; n];
    let mut joints: Vec<(usize, usize, bool)> = Vec::new();
    // (cells of the run, text, drawn on an adopted edge)
    type Label = (Vec<(usize, usize)>, String, bool);
    let mut labels: Vec<Label> = Vec::new();
    for route in &routes {
        let link = &links[route.link];
        let layer = if link.adopted {
            &mut solid
        } else {
            &mut dashed
        };
        let (ru, rl) = (row_of[route.u], row_of[route.l]);
        let start = (route.pu, row_y[ru] + BOX_H);
        let mut path = vec![start, (route.pu, track_y(ru, route.t1))];
        let end = match route.kind {
            1 => (route.pl, row_y[rl] + BOX_H),
            _ => (route.pl, row_y[rl] - 1),
        };
        match route.kind {
            2 => {
                let g = gutter(route.lane);
                path.push((g, track_y(ru, route.t1)));
                path.push((g, track_y(rl - 1, route.t2)));
                path.push((route.pl, track_y(rl - 1, route.t2)));
            }
            _ => path.push((route.pl, track_y(ru, route.t1))),
        }
        path.push(end);
        let is_hot = sel.is_some_and(|s| link.a == s || link.b == s);
        if is_hot {
            near[link.a] = true;
            near[link.b] = true;
        }
        for pair in path.windows(2) {
            line(layer, pair[0], pair[1]);
            if is_hot {
                let (a, b) = (pair[0], pair[1]);
                for row in &mut hot[a.1.min(b.1)..=a.1.max(b.1)] {
                    row[a.0.min(b.0)..=a.0.max(b.0)].fill(true);
                }
            }
        }
        let (side_u, side_l) = if route.u == link.a {
            (link.sides.0, link.sides.1)
        } else {
            (link.sides.1, link.sides.0)
        };
        markers.push((start.0, start.1, side_u.mark(), link.adopted, is_hot));
        markers.push((end.0, end.1, side_l.mark(), link.adopted, is_hot));
        joints.push((route.pu, row_y[ru] + BOX_H - 1, true));
        if route.kind == 1 {
            joints.push((route.pl, row_y[rl] + BOX_H - 1, true));
        } else {
            joints.push((route.pl, row_y[rl], false));
        }
        // Longest horizontal run carries the evidence label (rejected) or
        // the role (one of several edges between the same two entities).
        let text = match (&link.role, link.adopted) {
            (_, false) => Some(format!(" ✗ {} ", link.short)),
            (Some(role), true) => Some(format!(" {role} ")),
            (None, true) => None,
        };
        let best = path
            .windows(2)
            .filter(|p| p[0].1 == p[1].1)
            .max_by_key(|p| p[0].0.abs_diff(p[1].0));
        if let (Some(text), Some(p)) = (text, best) {
            let (a, b) = (p[0].0.min(p[1].0), p[0].0.max(p[1].0));
            let cells: Vec<(usize, usize)> = (a + 2..b.saturating_sub(1))
                .map(|cx| (cx, p[0].1))
                .collect();
            labels.push((cells, text, link.adopted));
        }
    }
    for cy in 0..y {
        for cx in 0..cols {
            let (s, d) = (solid[cy][cx], dashed[cy][cx]);
            if s | d == 0 {
                continue;
            }
            let c = glyph(s | d, s == 0);
            let tone = match (s == 0, hot[cy][cx]) {
                (true, _) => Tone::Bad,
                (false, true) => Tone::Accent,
                (false, false) => Tone::Info,
            };
            canvas.put(cx, cy, c, tone);
        }
    }
    // Labels on clear runs only: nothing crosses or joins them, and the run
    // is not shared with an edge of the other kind.
    for (cells, text, adopted) in labels {
        let text = fit(&text, cells.len());
        let tw = width(&text);
        if tw < 6 {
            continue;
        }
        let (own, other) = if adopted {
            (&solid, &dashed)
        } else {
            (&dashed, &solid)
        };
        let clear = cells
            .iter()
            .all(|&(cx, cy)| other[cy][cx] == 0 && own[cy][cx] & 3 == 0);
        if !clear {
            continue;
        }
        let start = cells[0].0 + (cells.len() - tw) / 2;
        canvas.text(
            start,
            cells[0].1,
            &text,
            if adopted { Tone::Info } else { Tone::Warn },
        );
    }
    // Boxes over lines.
    for (i, node) in nodes.iter().enumerate() {
        let bw = box_w(node);
        let (bx, by) = (x[i], row_y[row_of[i]]);
        let heavy = view.selected == Some(node.id.as_str());
        let focused = focus == Some(node.id.as_str());
        let tone = if heavy {
            Tone::Accent
        } else if focused || near[i] {
            Tone::Ink
        } else {
            Tone::Faint
        };
        let [tl, tr, bl, br, h, v] = if heavy || focused {
            ['┏', '┓', '┗', '┛', '━', '┃']
        } else {
            ['╭', '╮', '╰', '╯', '─', '│']
        };
        for cx in bx..bx + bw {
            canvas.put(cx, by, h, tone);
            canvas.put(cx, by + BOX_H - 1, h, tone);
            for cy in by + 1..by + BOX_H - 1 {
                canvas.put(cx, cy, ' ', tone);
            }
        }
        for cy in by..by + BOX_H {
            canvas.put(bx, cy, v, tone);
            canvas.put(bx + bw - 1, cy, v, tone);
        }
        canvas.put(bx, by, tl, tone);
        canvas.put(bx + bw - 1, by, tr, tone);
        canvas.put(bx, by + BOX_H - 1, bl, tone);
        canvas.put(bx + bw - 1, by + BOX_H - 1, br, tone);
        canvas.text(
            bx + 2,
            by + 1,
            &node.label,
            if heavy { Tone::Accent } else { Tone::Ink },
        );
        canvas.text(bx + 2, by + 2, &node.sub, Tone::Dim);
        canvas.boxes.push((node.id.clone(), bx, by, bw, BOX_H));
        if node.self_loop {
            let rx = bx + bw - 1;
            let (one, many) = links
                .iter()
                .find(|l| l.a == i && l.b == i)
                .map_or(('N', '1'), |l| (l.sides.0.mark(), l.sides.1.mark()));
            let joint = if heavy || focused { '┠' } else { '├' };
            canvas.put(rx, by + 1, joint, tone);
            canvas.put(rx, by + 2, joint, tone);
            canvas.put(rx + 1, by + 1, one, Tone::Accent);
            canvas.put(rx + 2, by + 1, '╮', Tone::Info);
            canvas.put(rx + 1, by + 2, many, Tone::Accent);
            canvas.put(rx + 2, by + 2, '╯', Tone::Info);
        }
    }
    for (jx, jy, bottom) in joints {
        let heavy = canvas.cells[jy][jx].0 == '━';
        let c = match (bottom, heavy) {
            (true, false) => '┬',
            (false, false) => '┴',
            (true, true) => '┯',
            (false, true) => '┷',
        };
        let tone = canvas.cells[jy][jx].1;
        canvas.put(jx, jy, c, tone);
    }
    for (mx, my, c, adopted, is_hot) in markers {
        let tone = match (adopted, sel.is_none() || is_hot) {
            (false, _) => Tone::Warn,
            (true, true) => Tone::Accent,
            (true, false) => Tone::Dim,
        };
        canvas.put(mx, my, c, tone);
    }
    canvas
}

/// Frames the confirm-screen reveal takes.
pub const REVEAL_FRAMES: usize = 10;

/// Radar-style reveal (after ascii.rest `radar` and kit `scan`): a front
/// spreads from `origin` (a cell; columns count half, so it reads round);
/// cells it has just reached flash as `▓`/`▒`, cells behind it settle to
/// their own glyph, cells ahead stay blank. Pure in its inputs; the still
/// frame (`frame >= REVEAL_FRAMES - 1`) is the canvas unchanged.
pub fn reveal(canvas: &Canvas, origin: (usize, usize), frame: usize) -> Vec<Vec<(char, Tone)>> {
    if frame >= REVEAL_FRAMES - 1 {
        return canvas.cells.clone();
    }
    let p = (frame + 1) as f32 / (REVEAL_FRAMES - 1) as f32;
    let dist = |x: usize, y: usize| {
        let dx = (x as f32 - origin.0 as f32) / 2.0;
        let dy = y as f32 - origin.1 as f32;
        (dx * dx + dy * dy).sqrt()
    };
    let far = [
        (0, 0),
        (canvas.width, 0),
        (0, canvas.height),
        (canvas.width, canvas.height),
    ]
    .iter()
    .map(|&(x, y)| dist(x, y))
    .fold(1.0f32, f32::max);
    let band = 0.14;
    let front = p * (1.0 + band);
    canvas
        .cells
        .iter()
        .enumerate()
        .map(|(y, row)| {
            row.iter()
                .enumerate()
                .map(|(x, &(c, tone))| {
                    if c == ' ' {
                        return (c, tone);
                    }
                    let d = dist(x, y) / far;
                    if d > front {
                        (' ', Tone::Faint)
                    } else if front - d < band * 0.4 {
                        if c == '\0' {
                            (' ', Tone::Faint)
                        } else {
                            ('▓', Tone::Hot)
                        }
                    } else if front - d < band {
                        if c == '\0' {
                            (' ', Tone::Faint)
                        } else {
                            ('▒', Tone::Ember)
                        }
                    } else {
                        (c, tone)
                    }
                })
                .collect()
        })
        .collect()
}

fn line(layer: &mut [Vec<u8>], a: (usize, usize), b: (usize, usize)) {
    const U: u8 = 1;
    const D: u8 = 2;
    const L: u8 = 4;
    const R: u8 = 8;
    if a.1 == b.1 {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        for cx in x0..x1 {
            layer[a.1][cx] |= R;
            layer[a.1][cx + 1] |= L;
        }
    } else {
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        for cy in y0..y1 {
            layer[cy][a.0] |= D;
            layer[cy + 1][a.0] |= U;
        }
    }
}

fn glyph(bits: u8, dashed: bool) -> char {
    match bits {
        0b0001..=0b0011 => {
            if dashed {
                '┆'
            } else {
                '│'
            }
        }
        0b1100 | 0b0100 | 0b1000 => {
            if dashed {
                '┄'
            } else {
                '─'
            }
        }
        0b1010 => '╭',
        0b0110 => '╮',
        0b1001 => '╰',
        0b0101 => '╯',
        0b1011 => '├',
        0b0111 => '┤',
        0b1110 => '┬',
        0b1101 => '┴',
        _ => '┼',
    }
}

fn legend(
    canvas: &mut Canvas,
    adj: &Adjudication,
    index: &BTreeMap<&str, usize>,
    focus: Option<&str>,
) {
    let label = |id: &str| {
        adj.graph
            .entities
            .iter()
            .find(|e| e.id == id)
            .map_or(id.to_owned(), |e| e.label.clone())
    };
    let mut lines: Vec<(String, Tone)> = vec![(
        "N─1 many-to-one   1─1 one-to-one   ┄ rejected (missing evidence)".into(),
        Tone::Dim,
    )];
    for e in adj.graph.edges.iter().filter(|e| !e.adopted) {
        if !index.contains_key(e.from.as_str()) || !index.contains_key(e.to.as_str()) {
            continue;
        }
        if focus.is_some_and(|f| e.from != f && e.to != f) {
            continue;
        }
        let note = adj
            .items
            .iter()
            .find(|i| i.id == e.relation)
            .map(|i| missing_evidence(i, &adj.thresholds))
            .unwrap_or_default();
        lines.push((
            format!("┄┄ {} → {}  ✗ {note}", label(&e.from), label(&e.to)),
            Tone::Bad,
        ));
    }
    // Several edges between the same two entities: name each by its role.
    for e in adj.graph.edges.iter().filter(|e| e.adopted) {
        if !index.contains_key(e.from.as_str()) || !index.contains_key(e.to.as_str()) {
            continue;
        }
        if focus.is_some_and(|f| e.from != f && e.to != f) {
            continue;
        }
        if let Some(r) = adj.items.iter().find(|i| i.id == e.relation).and_then(role) {
            lines.push((
                format!("── {} → {}  as {r}", label(&e.from), label(&e.to)),
                Tone::Info,
            ));
        }
    }
    let extra = lines.len() + 1;
    let wide = lines.iter().map(|l| width(&l.0)).max().unwrap_or(0);
    let new_w = canvas.width.max(wide);
    for row in &mut canvas.cells {
        row.resize(new_w, (' ', Tone::Faint));
    }
    canvas.width = new_w;
    canvas
        .cells
        .extend((0..extra).map(|_| vec![(' ', Tone::Faint); new_w]));
    let base = canvas.height + 1;
    canvas.height += extra;
    for (k, (text, tone)) in lines.iter().enumerate() {
        canvas.text(0, base + k, text, *tone);
    }
}

/// One relation for the Relations tab and `--format text`.
#[derive(Clone, Debug, PartialEq)]
pub struct RelationLine {
    pub relation: String,
    pub from: String,
    pub to: String,
    pub arrow: String,
    pub via: String,
    pub adopted: bool,
    pub confidence: String,
    pub note: String,
}

fn short_column(path: &str) -> String {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() >= 2 {
        parts[parts.len() - 2..].join(".")
    } else {
        path.to_owned()
    }
}

pub fn relations(adj: &Adjudication) -> Vec<RelationLine> {
    let label = |id: &str| {
        adj.graph
            .entities
            .iter()
            .find(|e| e.id == id)
            .map_or(id.to_owned(), |e| e.label.clone())
    };
    adj.graph
        .edges
        .iter()
        .map(|e| {
            let item = adj.items.iter().find(|i| i.id == e.relation);
            RelationLine {
                relation: e.relation.clone(),
                from: label(&e.from),
                to: label(&e.to),
                arrow: arrow(&e.cardinality, e.adopted),
                via: item
                    .and_then(|i| i.subject.get("from"))
                    .and_then(|v| v.as_str())
                    .map(short_column)
                    .unwrap_or_default(),
                adopted: e.adopted,
                confidence: item
                    .and_then(|i| i.scores.get("confidence"))
                    .map(super::adjudication::number)
                    .unwrap_or_default(),
                note: item
                    .map(|i| missing_evidence(i, &adj.thresholds))
                    .unwrap_or_default(),
            }
        })
        .collect()
}

pub fn text(adj: &Adjudication) -> String {
    let rels = relations(adj);
    let left = rels.iter().map(|r| width(&r.from)).max().unwrap_or(0);
    let right = rels.iter().map(|r| width(&r.to)).max().unwrap_or(0);
    let via = rels.iter().map(|r| width(&r.via) + 4).max().unwrap_or(0);
    let mut out = format!(
        "# oo.adjudication/v1 summary sha256:{}\n",
        adj.summary_sha256
    );
    for r in &rels {
        let status = if r.adopted {
            format!("✓ {}", r.confidence)
        } else {
            format!("✗ {}", r.note)
        };
        let line = format!(
            "{} {} {}  {}  {}",
            super::theme::pad(&r.from, left),
            r.arrow,
            super::theme::pad(&r.to, right),
            super::theme::pad(&format!("via {}", r.via), via),
            status
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

pub fn mermaid(adj: &Adjudication) -> String {
    let mut ids: BTreeMap<&str, String> = BTreeMap::new();
    let mut used = BTreeSet::new();
    for e in &adj.graph.entities {
        let base: String = e
            .table
            .rsplit('.')
            .next()
            .unwrap_or("entity")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let base = if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
            format!("e_{base}")
        } else {
            base
        };
        let mut id = base.clone();
        let mut k = 2;
        while !used.insert(id.clone()) {
            id = format!("{base}_{k}");
            k += 1;
        }
        ids.insert(&e.id, id);
    }
    let esc = |s: &str| s.replace('"', "#quot;").replace(['\n', '\r'], " ");
    let mut out = format!(
        "flowchart LR\n  %% oo.adjudication/v1 summary sha256:{}\n",
        adj.summary_sha256
    );
    for e in &adj.graph.entities {
        let rows = e
            .rows
            .map(|r| format!(" · {}", magnitude(r)))
            .unwrap_or_default();
        out.push_str(&format!(
            "  {}[\"{}<br/>{}{}\"]\n",
            ids[e.id.as_str()],
            esc(&e.label),
            esc(&e.key.join(",")),
            rows
        ));
    }
    for r in relations(adj).iter().zip(&adj.graph.edges) {
        let (line, edge) = r;
        let (a, b) = sides(&edge.cardinality);
        let card = format!(
            "{}:{}",
            if a == Side::Many { "N" } else { "1" },
            if b == Side::Many { "N" } else { "1" }
        );
        let column = line.via.rsplit('.').next().unwrap_or_default();
        if edge.adopted {
            out.push_str(&format!(
                "  {} -->|\"{card} {}\"| {}\n",
                ids[edge.from.as_str()],
                esc(column),
                ids[edge.to.as_str()]
            ));
        } else {
            out.push_str(&format!(
                "  {} -.->|\"rejected: {}\"| {}\n",
                ids[edge.from.as_str()],
                esc(&line.note),
                ids[edge.to.as_str()]
            ));
        }
    }
    out
}

/// `--format ascii`: header, canvas and legend.
pub fn ascii(adj: &Adjudication, max_width: usize, focus: Option<&str>) -> String {
    let canvas = render(
        adj,
        &View {
            selected: None,
            focus,
            max_width: Some(max_width),
            legend: true,
        },
    );
    let mut out = format!(
        "# oo.adjudication/v1 summary sha256:{}\n",
        adj.summary_sha256
    );
    if let Some(f) = &canvas.focus {
        let label = adj
            .graph
            .entities
            .iter()
            .find(|e| &e.id == f)
            .map_or(f.as_str(), |e| e.label.as_str());
        out.push_str(&format!(
            "# focus {label}: {} of {} entities (one hop)\n",
            canvas.shown.len(),
            adj.graph.entities.len()
        ));
    }
    for line in canvas.lines() {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tui::adjudication::{Edge, Entity, Graph, fixture, summary_sha256};

    pub(crate) fn synthetic(n: usize, edges: &[(usize, usize, bool)]) -> Adjudication {
        let entities = (0..n)
            .map(|i| Entity {
                id: format!("entity/t{i}"),
                label: format!("实体{i}"),
                table: format!("s.t{i}"),
                key: vec!["id".into()],
                display: None,
                rows: Some(10u64.pow((i % 7) as u32)),
            })
            .collect();
        let edges = edges
            .iter()
            .enumerate()
            .map(|(k, &(a, b, adopted))| Edge {
                relation: format!("relation/r{k}"),
                from: format!("entity/t{a}"),
                to: format!("entity/t{b}"),
                cardinality: "many_to_one".into(),
                adopted,
            })
            .collect();
        let mut adj = Adjudication {
            protocol: "oo.adjudication/v1".into(),
            input_sha256: String::new(),
            thresholds: Default::default(),
            items: Vec::new(),
            discovery: None,
            graph: Graph { entities, edges },
            summary_sha256: String::new(),
        };
        adj.summary_sha256 = summary_sha256(&adj);
        adj
    }

    fn boxes_disjoint(c: &Canvas) {
        for (i, a) in c.boxes.iter().enumerate() {
            for b in &c.boxes[i + 1..] {
                let apart =
                    a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1 || a.2 + a.4 <= b.2 || b.2 + b.4 <= a.2;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn small_graph_is_deterministic_and_hub_centred() {
        let adj = synthetic(3, &[(0, 1, true), (2, 1, true)]);
        let a = render(&adj, &View::default());
        assert_eq!(a, render(&adj, &View::default()));
        boxes_disjoint(&a);
        let hub = a.boxes.iter().find(|b| b.0 == "entity/t1").unwrap();
        let ys: Vec<usize> = a.boxes.iter().map(|b| b.2).collect();
        assert!(
            ys.iter().any(|&y| y < hub.2) && ys.iter().any(|&y| y > hub.2),
            "{:#?}",
            a.lines()
        );
        let text = a.lines().join("\n");
        assert!(text.contains('N') && text.contains('1'));
    }

    #[test]
    fn cycles_self_references_and_rejections_render() {
        let adj = synthetic(
            4,
            &[
                (0, 1, true),
                (1, 2, true),
                (2, 0, true),
                (3, 3, true),
                (3, 0, false),
            ],
        );
        let c = render(
            &adj,
            &View {
                legend: true,
                ..View::default()
            },
        );
        boxes_disjoint(&c);
        let text = c.lines().join("\n");
        assert!(
            text.contains('╮') && text.contains("├N╮") && text.contains("├1╯"),
            "{text}"
        );
        assert!(text.contains('┄') || text.contains('┆'), "{text}");
        assert!(text.contains("┄┄ 实体3 → 实体0"), "{text}");
        // Colour layer: rejected lines are red, markers accent, boxes faint.
        let tones = c.tones().join("\n");
        assert!(
            tones.contains('R') && tones.contains('A') && tones.contains('f'),
            "{tones}"
        );
    }

    #[test]
    fn large_graphs_open_in_focus_mode_with_one_hop() {
        let edges: Vec<(usize, usize, bool)> = (1..30).map(|i| (i, (i - 1) / 4, true)).collect();
        let adj = synthetic(30, &edges);
        let c = render(&adj, &View::default());
        let focus = c.focus.clone().unwrap();
        assert_eq!(c.shown[0], focus);
        assert_eq!(c.shown.len(), 1 + neighbours(&adj, &focus).len());
        assert!(c.shown.len() < 30);
        boxes_disjoint(&c);
        let moved = render(
            &adj,
            &View {
                focus: Some("entity/t7"),
                ..View::default()
            },
        );
        assert_eq!(moved.shown[0], "entity/t7");
        assert!(moved.shown.contains(&"entity/t1".to_owned()));
    }

    #[test]
    fn long_cjk_labels_keep_display_width() {
        let mut adj = synthetic(2, &[(0, 1, true)]);
        adj.graph.entities[0].label = "国际跨境物流运输订单明细记录表".into();
        let c = render(&adj, &View::default());
        for row in &c.cells {
            let cols: usize = row
                .iter()
                .filter(|c| c.0 != '\0')
                .map(|c| char_width(c.0).max(1))
                .sum();
            assert_eq!(cols, c.width);
        }
        assert!(c.lines().join("\n").contains("国际跨境物流运输…"));
        boxes_disjoint(&c);
    }

    #[test]
    fn fixture_text_and_mermaid() {
        let adj = fixture();
        let text = text(&adj);
        assert!(
            text.contains("订单     >─1 客户    via orders.customer_id"),
            "{text}"
        );
        assert!(text.contains("via orders.customer_id"));
        assert!(
            text.contains("运单     >┄1 仓库") && text.contains("✗ containment 0.71 < 0.98"),
            "{text}"
        );
        let m = mermaid(&adj);
        assert!(
            m.starts_with("flowchart LR")
                && m.contains("shipments -.->|\"rejected: containment 0.71 < 0.98\"| warehouses")
        );
        if std::env::var_os("OO_PRINT_GRAPH").is_some() {
            println!("{}", ascii(&adj, 120, None));
            println!("{}", ascii(&adj, 80, None));
        }
        let wrapped = render(
            &adj,
            &View {
                max_width: Some(76),
                ..View::default()
            },
        );
        assert!(wrapped.width <= 80, "{}", wrapped.width);
        boxes_disjoint(&wrapped);
    }

    #[test]
    fn role_edges_carry_their_role() {
        let adj = fixture();
        let out = ascii(&adj, 120, None);
        assert_eq!(out, ascii(&adj, 120, None), "deterministic");
        assert!(out.contains("── 线路 → 网点  as origin_station"), "{out}");
        assert!(out.contains("── 线路 → 网点  as dest_station"), "{out}");
        // Only role edges are named; ordinary edges stay unlabeled.
        assert_eq!(out.matches("  as ").count(), 2, "{out}");
        // On the canvas the role rides on the edge's own horizontal run.
        let canvas = render(&adj, &View::default());
        let lines = canvas.lines().join("\n");
        assert!(lines.contains("─ dest_sta"), "{lines}");
        // The rejected scope edge names its flaw, not a bare number.
        assert!(lines.contains("✗ scope"), "{lines}");
    }
}
