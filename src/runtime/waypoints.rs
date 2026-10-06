//! A small waypoint graph for monsters and NPCs that walk between hand-placed
//! points: nodes, two-way edges, the nearest node to a position, and the
//! shortest path between two nodes.
//!
//! Everything is plain data with a fixed visiting order, so the same graph
//! and query give the same path on every run and replays stay in sync.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WaypointGraph {
    /// Node positions in world space; a node's ID is its index.
    pub nodes: Vec<[f32; 3]>,
    /// Neighbors of each node, parallel to `nodes`.
    pub edges: Vec<Vec<usize>>,
}

impl WaypointGraph {
    /// Adds a node and returns its ID.
    pub fn add_node(&mut self, position: [f32; 3]) -> usize {
        self.nodes.push(position);
        self.edges.push(Vec::new());
        self.nodes.len() - 1
    }

    /// Joins two nodes both ways. Joining twice or a node to itself does
    /// nothing.
    ///
    /// # Panics
    /// When either ID is not a node.
    pub fn connect(&mut self, a: usize, b: usize) {
        assert!(a < self.nodes.len() && b < self.nodes.len(), "no node");
        if a != b && !self.edges[a].contains(&b) {
            self.edges[a].push(b);
            self.edges[b].push(a);
        }
    }

    /// Removes the edge between two nodes, for a door that closes.
    pub fn disconnect(&mut self, a: usize, b: usize) {
        if let Some(edges) = self.edges.get_mut(a) {
            edges.retain(|&n| n != b);
        }
        if let Some(edges) = self.edges.get_mut(b) {
            edges.retain(|&n| n != a);
        }
    }

    /// The node closest to `position`, the lowest ID on a tie, or `None`
    /// for an empty graph.
    #[must_use]
    pub fn nearest(&self, position: [f32; 3]) -> Option<usize> {
        (0..self.nodes.len()).min_by(|&a, &b| {
            distance(self.nodes[a], position)
                .total_cmp(&distance(self.nodes[b], position))
        })
    }

    /// The shortest path by straight-line edge length from `from` to `to`,
    /// both included, or `None` when no path joins them.
    #[must_use]
    pub fn path(&self, from: usize, to: usize) -> Option<Vec<usize>> {
        if from >= self.nodes.len() || to >= self.nodes.len() {
            return None;
        }
        let mut cost = vec![f32::INFINITY; self.nodes.len()];
        let mut previous = vec![usize::MAX; self.nodes.len()];
        let mut open = BinaryHeap::new();
        cost[from] = 0.0;
        open.push(Open {
            cost: 0.0,
            node: from,
        });
        while let Some(Open {
            cost: reached,
            node,
        }) = open.pop()
        {
            if node == to {
                break;
            }
            if reached > cost[node] {
                continue;
            }
            for &next in &self.edges[node] {
                let through =
                    reached + distance(self.nodes[node], self.nodes[next]);
                if through < cost[next] {
                    cost[next] = through;
                    previous[next] = node;
                    open.push(Open {
                        cost: through,
                        node: next,
                    });
                }
            }
        }
        if !cost[to].is_finite() {
            return None;
        }
        let mut path = vec![to];
        while *path.last().unwrap() != from {
            path.push(previous[*path.last().unwrap()]);
        }
        path.reverse();
        Some(path)
    }

    /// The total edge length along `path`.
    #[must_use]
    pub fn length(&self, path: &[usize]) -> f32 {
        path.windows(2)
            .map(|pair| distance(self.nodes[pair[0]], self.nodes[pair[1]]))
            .sum()
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// A heap entry ordered cheapest first, then lowest node ID, so equal-cost
/// paths always resolve the same way.
#[derive(PartialEq)]
struct Open {
    cost: f32,
    node: usize,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.node.cmp(&self.node))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square 0-1-2-3 with a long detour 0-4-2.
    fn square() -> WaypointGraph {
        let mut graph = WaypointGraph::default();
        for position in [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [-5.0, 0.0, 5.0],
        ] {
            graph.add_node(position);
        }
        for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0), (0, 4), (4, 2)] {
            graph.connect(a, b);
        }
        graph
    }

    #[test]
    fn shortest_path_avoids_the_detour_and_ties_break_the_same_way() {
        let mut graph = square();
        // 0-1-2 and 0-3-2 cost the same; the lower ID wins every time.
        assert_eq!(graph.path(0, 2), Some(vec![0, 1, 2]));
        assert_eq!(graph.path(0, 2), graph.clone().path(0, 2));
        assert_eq!(graph.length(&[0, 1, 2]), 2.0);
        assert_eq!(graph.path(3, 3), Some(vec![3]));
        graph.disconnect(1, 2);
        assert_eq!(graph.path(0, 2), Some(vec![0, 3, 2]));
        graph.disconnect(3, 2);
        assert_eq!(graph.path(0, 2), Some(vec![0, 4, 2]));
        graph.disconnect(4, 2);
        assert_eq!(graph.path(0, 2), None);
        assert_eq!(graph.path(0, 99), None);
    }

    #[test]
    fn nearest_picks_the_closest_node() {
        let graph = square();
        assert_eq!(graph.nearest([0.9, 3.0, 1.2]), Some(2));
        assert_eq!(graph.nearest([-4.0, 0.0, 4.0]), Some(4));
        assert_eq!(graph.nearest([0.5, 0.0, 0.0]), Some(0), "tie: lowest ID");
        assert_eq!(WaypointGraph::default().nearest([0.0; 3]), None);
    }
}
