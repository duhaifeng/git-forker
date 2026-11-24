use std::collections::BTreeSet;
use std::fmt::Write;

use super::model::{Hunk, LineKind};

/// 把补丁应用到目标时的方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchDirection {
    /// 按差异原方向应用，例如把工作区更改暂存进索引。
    Forward,
    /// 反向应用，例如从索引取消暂存，或从工作区放弃更改。
    Reverse,
}

/// 用一个区块中的部分行构造补丁。`selected` 为 `None` 表示整个区块。
///
/// 未选中的行按目标中的现状处理：正向时未选中的新增行丢弃、删除行保留为上下文；
/// 反向时未选中的新增行保留为上下文、删除行丢弃。
pub fn hunk_patch(
    path: &str,
    hunk: &Hunk,
    selected: Option<&BTreeSet<usize>>,
    direction: PatchDirection,
) -> Option<String> {
    let mut body = String::new();
    let mut old_count = 0u32;
    let mut new_count = 0u32;
    let mut changes = 0usize;

    for (index, line) in hunk.lines.iter().enumerate() {
        let chosen = selected.is_none_or(|set| set.contains(&index));
        let kind = match (line.kind, direction) {
            (LineKind::Context, _) => LineKind::Context,
            (LineKind::Added, PatchDirection::Forward) => LineKind::Added,
            (LineKind::Removed, PatchDirection::Forward) => LineKind::Removed,
            (LineKind::Added, PatchDirection::Reverse) => LineKind::Removed,
            (LineKind::Removed, PatchDirection::Reverse) => LineKind::Added,
        };
        let kind = if chosen || kind == LineKind::Context {
            Some(kind)
        } else {
            match kind {
                LineKind::Added => None,
                LineKind::Removed | LineKind::Context => Some(LineKind::Context),
            }
        };
        let Some(kind) = kind else {
            continue;
        };
        let prefix = match kind {
            LineKind::Context => {
                old_count += 1;
                new_count += 1;
                ' '
            }
            LineKind::Added => {
                new_count += 1;
                changes += 1;
                '+'
            }
            LineKind::Removed => {
                old_count += 1;
                changes += 1;
                '-'
            }
        };
        body.push(prefix);
        body.push_str(&line.raw);
        if !line.raw.ends_with('\n') {
            body.push_str("\n\\ No newline at end of file\n");
        }
    }

    if changes == 0 {
        return None;
    }
    let (old_start, new_start) = match direction {
        PatchDirection::Forward => (hunk.old_start, hunk.old_start),
        PatchDirection::Reverse => (hunk.new_start, hunk.new_start),
    };
    let mut patch = String::new();
    let _ = writeln!(patch, "diff --git a/{path} b/{path}");
    let _ = writeln!(patch, "--- a/{path}");
    let _ = writeln!(patch, "+++ b/{path}");
    let _ = writeln!(
        patch,
        "@@ -{} +{} @@",
        range(old_start, old_count),
        range(new_start, new_count)
    );
    patch.push_str(&body);
    Some(patch)
}

fn range(start: u32, count: u32) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::model::DiffLine;

    fn line(kind: LineKind, raw: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no: None,
            new_no: None,
            raw: raw.to_string(),
        }
    }

    fn sample() -> Hunk {
        Hunk {
            header: "@@ -1,2 +1,2 @@".into(),
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 2,
            lines: vec![
                line(LineKind::Context, "a\n"),
                line(LineKind::Removed, "b\n"),
                line(LineKind::Added, "c\n"),
            ],
        }
    }

    #[test]
    fn whole_hunk_keeps_counts() {
        let patch = hunk_patch("f", &sample(), None, PatchDirection::Forward).unwrap();
        assert!(patch.contains("@@ -1,2 +1,2 @@\n a\n-b\n+c\n"));
    }

    #[test]
    fn selecting_only_addition_keeps_removed_line_as_context() {
        let selected = BTreeSet::from([2]);
        let patch = hunk_patch("f", &sample(), Some(&selected), PatchDirection::Forward).unwrap();
        assert!(patch.contains("@@ -1,2 +1,3 @@\n a\n b\n+c\n"));
    }

    #[test]
    fn reverse_swaps_signs() {
        let patch = hunk_patch("f", &sample(), None, PatchDirection::Reverse).unwrap();
        assert!(patch.contains(" a\n+b\n-c\n"));
    }
}
