//! Graph algorithms of structural analysis, sized for 10⁵-equation models:
//! bipartite graphs in compressed form, Hopcroft–Karp maximum matching
//! (O(E√V)), single augmenting paths for incremental matching, Tarjan's
//! strongly connected components, and the alternating-path closures of the
//! Dulmage–Mendelsohn decomposition. Every search is iterative, so a long
//! chain of equations cannot overflow the stack.

#![allow(clippy::needless_range_loop)] // parallel arrays indexed in step

/// "No partner" in a matching.
pub const NONE: usize = usize::MAX;

/// A bipartite graph: rows (equations) to columns (unknowns), stored
/// row-wise.
#[derive(Clone, Debug, Default)]
pub struct Bipartite {
    /// columns
    pub n_cols: usize,
    /// row starts, rows + 1 values
    pub row_ptr: Vec<usize>,
    /// column indices
    pub cols: Vec<usize>,
}

impl Bipartite {
    /// From each row's columns.
    pub fn from_rows(n_cols: usize, rows: &[Vec<usize>]) -> Self {
        let mut row_ptr = Vec::with_capacity(rows.len() + 1);
        let mut cols = Vec::with_capacity(rows.iter().map(Vec::len).sum());
        row_ptr.push(0);
        for r in rows {
            cols.extend_from_slice(r);
            row_ptr.push(cols.len());
        }
        Bipartite { n_cols, row_ptr, cols }
    }

    /// The number of rows.
    pub fn n_rows(&self) -> usize {
        self.row_ptr.len() - 1
    }

    /// A row's columns.
    pub fn row(&self, r: usize) -> &[usize] {
        &self.cols[self.row_ptr[r]..self.row_ptr[r + 1]]
    }

    /// Appends a row.
    pub fn push_row(&mut self, cols: &[usize]) {
        self.cols.extend_from_slice(cols);
        self.row_ptr.push(self.cols.len());
    }

    /// Removes the last row.
    pub fn pop_row(&mut self) {
        self.row_ptr.pop();
        let end = *self.row_ptr.last().expect("row_ptr keeps its first entry");
        self.cols.truncate(end);
    }

    /// The transpose: each column's rows.
    pub fn transpose(&self) -> Bipartite {
        let mut count = vec![0usize; self.n_cols + 1];
        for &c in &self.cols {
            count[c + 1] += 1;
        }
        for i in 0..self.n_cols {
            count[i + 1] += count[i];
        }
        let mut next = count.clone();
        let mut rows = vec![0; self.cols.len()];
        for r in 0..self.n_rows() {
            for &c in self.row(r) {
                rows[next[c]] = r;
                next[c] += 1;
            }
        }
        Bipartite { n_cols: self.n_rows(), row_ptr: count, cols: rows }
    }
}

/// A matching: each row's column and each column's row ([`NONE`] when
/// unmatched).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matching {
    /// row → column
    pub row: Vec<usize>,
    /// column → row
    pub col: Vec<usize>,
}

impl Matching {
    /// The empty matching of a graph.
    pub fn empty(g: &Bipartite) -> Self {
        Matching { row: vec![NONE; g.n_rows()], col: vec![NONE; g.n_cols] }
    }

    /// The number of matched pairs.
    pub fn size(&self) -> usize {
        self.row.iter().filter(|&&c| c != NONE).count()
    }

    /// Whether every row and every column is matched.
    pub fn is_perfect(&self) -> bool {
        self.row.iter().all(|&c| c != NONE) && self.col.iter().all(|&r| r != NONE)
    }
}

/// Hopcroft–Karp maximum matching, after a greedy start.
pub fn hopcroft_karp(g: &Bipartite) -> Matching {
    let mut m = Matching::empty(g);
    // greedy start: most rows of a model's graph match at once
    for r in 0..g.n_rows() {
        if let Some(&c) = g.row(r).iter().find(|&&c| m.col[c] == NONE) {
            m.row[r] = c;
            m.col[c] = r;
        }
    }
    complete(g, &mut m);
    m
}

/// Grows `m` to a maximum matching by Hopcroft–Karp phases.
pub fn complete(g: &Bipartite, m: &mut Matching) {
    let n = g.n_rows();
    const INF: u32 = u32::MAX;
    let mut dist = vec![INF; n];
    let mut queue = Vec::with_capacity(n);
    let mut it = vec![0usize; n];
    let mut stack: Vec<usize> = vec![];
    let mut via: Vec<usize> = vec![];
    loop {
        // breadth-first layers from the free rows
        queue.clear();
        for r in 0..n {
            if m.row[r] == NONE {
                dist[r] = 0;
                queue.push(r);
            } else {
                dist[r] = INF;
            }
        }
        let mut free_at = INF;
        let mut head = 0;
        while head < queue.len() {
            let r = queue[head];
            head += 1;
            if dist[r] >= free_at {
                continue;
            }
            for &c in g.row(r) {
                let r2 = m.col[c];
                if r2 == NONE {
                    free_at = free_at.min(dist[r] + 1);
                } else if dist[r2] == INF {
                    dist[r2] = dist[r] + 1;
                    queue.push(r2);
                }
            }
        }
        if free_at == INF {
            return;
        }
        // depth-first augmentation along the layers
        it.iter_mut().for_each(|x| *x = 0);
        let mut augmented = false;
        for r0 in 0..n {
            if m.row[r0] != NONE || dist[r0] != 0 {
                continue;
            }
            stack.clear();
            via.clear();
            stack.push(r0);
            while let Some(&r) = stack.last() {
                let row = g.row(r);
                if it[r] < row.len() {
                    let c = row[it[r]];
                    it[r] += 1;
                    let r2 = m.col[c];
                    if r2 == NONE {
                        if dist[r] + 1 != free_at {
                            continue;
                        }
                        // augment: stack[i] takes via[i], the top takes c
                        via.push(c);
                        for (k, &rr) in stack.iter().enumerate() {
                            let cc = via[k];
                            m.row[rr] = cc;
                            m.col[cc] = rr;
                        }
                        augmented = true;
                        break;
                    } else if dist[r2] == dist[r] + 1 {
                        via.push(c);
                        stack.push(r2);
                    }
                } else {
                    dist[r] = INF;
                    stack.pop();
                    via.pop();
                }
            }
        }
        if !augmented {
            return;
        }
    }
}

/// One augmenting path from the free row `r0` (Kuhn's search), with
/// `stamp`/`seen` marking the columns visited by this search. Returns
/// whether `m` grew. Used to add rows one at a time in a chosen priority.
pub fn augment(g: &Bipartite, m: &mut Matching, r0: usize, seen: &mut [u32], stamp: u32) -> bool {
    let mut stack: Vec<(usize, usize)> = vec![(r0, 0)];
    let mut via: Vec<usize> = vec![];
    while let Some(top) = stack.last_mut() {
        let (r, pos) = *top;
        let row = g.row(r);
        if pos < row.len() {
            top.1 += 1;
            let c = row[pos];
            if seen[c] == stamp {
                continue;
            }
            seen[c] = stamp;
            via.push(c);
            match m.col[c] {
                NONE => {
                    for (k, &(rr, _)) in stack.iter().enumerate() {
                        let cc = via[k];
                        m.row[rr] = cc;
                        m.col[cc] = rr;
                    }
                    return true;
                }
                r2 => stack.push((r2, 0)),
            }
        } else {
            stack.pop();
            via.pop();
        }
    }
    false
}

/// Strongly connected components of a directed graph (`adj[v]`: the
/// vertices `v` has an edge to), each listed after every component it has
/// an edge to: dependencies first.
pub fn scc(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut s: Vec<usize> = vec![];
    let mut out = vec![];
    let mut next = 0;
    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        s.push(root);
        on_stack[root] = true;
        while let Some(top) = call.last_mut() {
            let (v, pos) = (top.0, top.1);
            if pos < adj[v].len() {
                top.1 += 1;
                let w = adj[v][pos];
                if index[w] == usize::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    s.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                call.pop();
                if low[v] == index[v] {
                    let mut comp = vec![];
                    loop {
                        let w = s.pop().expect("v is on the stack");
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    out.push(comp);
                }
                if let Some(parent) = call.last() {
                    low[parent.0] = low[parent.0].min(low[v]);
                }
            }
        }
    }
    out
}

/// The rows and columns reachable by alternating paths from the unmatched
/// rows: the over-determined part of the Dulmage–Mendelsohn decomposition.
pub fn over_part(g: &Bipartite, m: &Matching) -> (Vec<usize>, Vec<usize>) {
    let mut row_seen = vec![false; g.n_rows()];
    let mut col_seen = vec![false; g.n_cols];
    let mut todo: Vec<usize> = (0..g.n_rows()).filter(|&r| m.row[r] == NONE).collect();
    for &r in &todo {
        row_seen[r] = true;
    }
    while let Some(r) = todo.pop() {
        for &c in g.row(r) {
            if !col_seen[c] {
                col_seen[c] = true;
                let r2 = m.col[c];
                if r2 != NONE && !row_seen[r2] {
                    row_seen[r2] = true;
                    todo.push(r2);
                }
            }
        }
    }
    let rows = (0..g.n_rows()).filter(|&r| row_seen[r]).collect();
    let cols = (0..g.n_cols).filter(|&c| col_seen[c]).collect();
    (rows, cols)
}

/// The rows and columns reachable by alternating paths from the unmatched
/// columns: the under-determined part. `gt` is `g`'s transpose.
pub fn under_part(g: &Bipartite, gt: &Bipartite, m: &Matching) -> (Vec<usize>, Vec<usize>) {
    let mut row_seen = vec![false; g.n_rows()];
    let mut col_seen = vec![false; g.n_cols];
    let mut todo: Vec<usize> = (0..g.n_cols).filter(|&c| m.col[c] == NONE).collect();
    for &c in &todo {
        col_seen[c] = true;
    }
    while let Some(c) = todo.pop() {
        for &r in gt.row(c) {
            if !row_seen[r] {
                row_seen[r] = true;
                let c2 = m.row[r];
                if c2 != NONE && !col_seen[c2] {
                    col_seen[c2] = true;
                    todo.push(c2);
                }
            }
        }
    }
    let rows = (0..g.n_rows()).filter(|&r| row_seen[r]).collect();
    let cols = (0..g.n_cols).filter(|&c| col_seen[c]).collect();
    (rows, cols)
}

/// Splits items into groups that share a key (union-find): `keys[i]` are
/// item i's keys; returns each group's items, in order of first item.
pub fn groups(n_keys: usize, keys: &[Vec<usize>]) -> Vec<Vec<usize>> {
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let n = keys.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut owner = vec![NONE; n_keys];
    for (i, ks) in keys.iter().enumerate() {
        for &k in ks {
            if owner[k] == NONE {
                owner[k] = i;
            } else {
                let (a, b) = (find(&mut parent, owner[k]), find(&mut parent, i));
                if a != b {
                    parent[b.max(a)] = a.min(b);
                }
            }
        }
    }
    let mut slot = vec![NONE; n];
    let mut out: Vec<Vec<usize>> = vec![];
    for i in 0..n {
        let r = find(&mut parent, i);
        if slot[r] == NONE {
            slot[r] = out.len();
            out.push(vec![]);
        }
        out[slot[r]].push(i);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hopcroft_karp_finds_maximum_matchings() {
        // the greedy start matches r0-c0, leaving r1 (only c0) to augment
        let g = Bipartite::from_rows(3, &[vec![0, 1], vec![0], vec![1, 2]]);
        let m = hopcroft_karp(&g);
        assert!(m.is_perfect());
        assert_eq!(m.row, vec![1, 0, 2]);
        // a structurally singular graph: two rows on one column
        let g = Bipartite::from_rows(2, &[vec![0], vec![0], vec![0, 1]]);
        let m = hopcroft_karp(&g);
        assert_eq!(m.size(), 2);
        let (rows, cols) = over_part(&g, &m);
        assert_eq!((rows, cols), (vec![0, 1], vec![0]));
    }

    #[test]
    fn long_chains_need_no_recursion() {
        // a chain whose greedy start is wrong everywhere: row i prefers
        // column i+1, the last row has only its own column, so the one
        // augmenting path runs the whole length of the chain
        let n = 200_000;
        let mut rows: Vec<Vec<usize>> = (0..n).map(|i| vec![i + 1, i]).collect();
        rows[n - 1] = vec![n - 1];
        let g = Bipartite::from_rows(n, &rows);
        let m = hopcroft_karp(&g);
        assert!(m.is_perfect());
        let mut m2 = Matching::empty(&g);
        for r in 0..n - 1 {
            m2.row[r] = r + 1;
            m2.col[r + 1] = r;
        }
        let mut seen = vec![0u32; n];
        assert!(augment(&g, &mut m2, n - 1, &mut seen, 1));
        assert!(m2.is_perfect());
    }

    #[test]
    fn under_part_follows_alternating_paths() {
        // r0: c0 c1 ; r1: c1 — c2 is free, c0/c1 fine
        let g = Bipartite::from_rows(3, &[vec![0, 1], vec![1, 2]]);
        let m = hopcroft_karp(&g);
        assert_eq!(m.size(), 2);
        let (rows, cols) = under_part(&g, &g.transpose(), &m);
        assert!(cols.len() >= 2 && !rows.is_empty(), "{rows:?} {cols:?}");
    }

    #[test]
    fn components_and_groups() {
        let adj = vec![vec![1], vec![2], vec![0], vec![0]];
        let comps = scc(&adj);
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].len(), 3);
        assert_eq!(comps[1], vec![3]);
        let gr = groups(4, &[vec![0], vec![1], vec![0, 2], vec![3]]);
        assert_eq!(gr, vec![vec![0, 2], vec![1], vec![3]]);
    }
}
