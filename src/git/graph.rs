use super::model::{CommitRow, EdgeKind, GraphEdge, GraphRow, Oid};

/// 为按拓扑顺序排列的提交分配泳道。每条泳道保存它等待的下一个提交。
pub fn layout(commits: &[CommitRow]) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<(Oid, usize)>> = Vec::new();
    let mut next_color = 0usize;
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let before = lanes.clone();
        let hits: Vec<usize> = before
            .iter()
            .enumerate()
            .filter_map(|(index, lane)| match lane {
                Some((id, _)) if *id == commit.id => Some(index),
                _ => None,
            })
            .collect();

        let (node, color) = match hits.first() {
            Some(&index) => (index, before[index].map(|(_, color)| color).unwrap_or(0)),
            None => {
                let slot = free_slot(&mut lanes);
                let color = next_color;
                next_color += 1;
                (slot, color)
            }
        };

        let mut edges = Vec::new();
        for &index in &hits {
            let lane_color = before[index].map(|(_, color)| color).unwrap_or(color);
            edges.push(GraphEdge {
                from: index,
                to: node,
                kind: EdgeKind::TopToNode,
                color: lane_color,
            });
            lanes[index] = None;
        }
        for (index, lane) in before.iter().enumerate() {
            if let Some((_, lane_color)) = lane
                && !hits.contains(&index)
            {
                edges.push(GraphEdge {
                    from: index,
                    to: index,
                    kind: EdgeKind::Through,
                    color: *lane_color,
                });
            }
        }

        for (position, parent) in commit.parents.iter().enumerate() {
            let existing = lanes
                .iter()
                .position(|lane| matches!(lane, Some((id, _)) if id == parent));
            let (target, edge_color) = match existing {
                Some(index) => (index, lanes[index].map(|(_, color)| color).unwrap_or(color)),
                None if position == 0 => {
                    if node >= lanes.len() {
                        lanes.resize(node + 1, None);
                    }
                    lanes[node] = Some((*parent, color));
                    (node, color)
                }
                None => {
                    let slot = free_slot(&mut lanes);
                    let lane_color = next_color;
                    next_color += 1;
                    lanes[slot] = Some((*parent, lane_color));
                    (slot, lane_color)
                }
            };
            edges.push(GraphEdge {
                from: node,
                to: target,
                kind: EdgeKind::NodeToBottom,
                color: edge_color,
            });
        }

        let width = before.len().max(lanes.len()).max(node + 1);
        while matches!(lanes.last(), Some(None)) {
            lanes.pop();
        }
        rows.push(GraphRow {
            node,
            color,
            width,
            edges,
        });
    }
    rows
}

fn free_slot(lanes: &mut Vec<Option<(Oid, usize)>>) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(index) => index,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> Oid {
        Oid::from_bytes(&[byte; 20]).unwrap()
    }

    fn row(id: u8, parents: &[u8]) -> CommitRow {
        CommitRow {
            id: oid(id),
            short: String::new(),
            summary: String::new(),
            author: String::new(),
            email: String::new(),
            time: 0,
            parents: parents.iter().map(|byte| oid(*byte)).collect(),
        }
    }

    #[test]
    fn linear_history_stays_in_one_lane() {
        let rows = layout(&[row(3, &[2]), row(2, &[1]), row(1, &[])]);
        assert!(rows.iter().all(|row| row.node == 0));
        assert_eq!(rows[2].edges.len(), 1);
    }

    #[test]
    fn merge_opens_and_closes_second_lane() {
        // 4 合并 3 和 2；3 与 2 都来自 1。
        let rows = layout(&[row(4, &[3, 2]), row(3, &[1]), row(2, &[1]), row(1, &[])]);
        assert_eq!(rows[0].node, 0);
        assert_eq!(rows[2].node, 1);
        assert!(
            rows[2]
                .edges
                .iter()
                .any(|edge| edge.kind == EdgeKind::NodeToBottom && edge.from == 1 && edge.to == 0)
        );
        assert_eq!(rows[3].node, 0);
        assert_eq!(rows[3].width, 1);
    }
}
