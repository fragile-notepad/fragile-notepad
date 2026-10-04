use super::fsm::ByteRange;
use super::fsm::{DeclarationEvent, StructuralEvent, StructuralEventKind};
use super::{FunctionKind, OutlineNode, OutlineNodeKind, OutlineScanMode, OutlineTree};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CascadeOutput {
    pub tree: OutlineTree,
    pub functions: Vec<CascadedFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CascadedFunction {
    pub name: String,
    pub kind: FunctionKind,
    pub range: super::EditorRange,
    pub body_range: Option<super::EditorRange>,
    pub depth: usize,
    pub start_offset: usize,
    pub end_offset: usize,
}

pub(super) fn cascade(
    text: &str,
    containers: &[StructuralEvent],
    declarations: Vec<DeclarationEvent>,
) -> CascadeOutput {
    let positions = TextPositions::new(text);
    let declaration_intervals =
        IntervalCounts::new(declarations.iter().filter_map(|event| event.body_range));
    let container_intervals =
        IntervalCounts::new(containers.iter().filter_map(|event| event.body_range));
    let mut method_intervals = Vec::new();
    for declaration in &declarations {
        for kind in &declaration.rule.method_containers {
            if !method_intervals
                .iter()
                .any(|(existing, _)| existing == kind)
            {
                let intervals = IntervalCounts::new(containers.iter().filter_map(|event| {
                    let StructuralEventKind::Body { owner_kind, .. } = event.kind;
                    (owner_kind == *kind).then_some(event.body_range).flatten()
                }));
                method_intervals.push((*kind, intervals));
            }
        }
    }
    let mut nodes = container_nodes(
        text,
        containers,
        &positions,
        &container_intervals,
        &declaration_intervals,
    );
    let mut functions = Vec::new();
    let mut function_nodes = Vec::new();

    for declaration in &declarations {
        let offset = declaration.signature_range.start;
        let own_depth = declarations.partition_point(|event| event.signature_range.start < offset);
        let own_end = declarations.partition_point(|event| event.signature_range.start <= offset);
        let excluded = declarations[own_depth..own_end]
            .iter()
            .filter(|event| {
                event
                    .body_range
                    .is_some_and(|range| range.start <= offset && offset < range.end)
            })
            .count();
        let declaration_depth = declaration_intervals.count(offset).saturating_sub(excluded);
        if declaration.rule.scan == OutlineScanMode::Callable
            && declaration.terminated
            && declaration_depth > 0
        {
            continue;
        }

        let depth = container_intervals.count(offset) + declaration_depth;
        let mut kind = if declaration.rule.node_kind == OutlineNodeKind::Method
            || method_intervals.iter().any(|(kind, intervals)| {
                declaration.rule.method_containers.contains(kind) && intervals.count(offset) > 0
            }) {
            FunctionKind::Method
        } else {
            FunctionKind::Function
        };
        if declaration.terminated {
            kind = FunctionKind::Declaration;
        }

        let Some(range) = indexed_range(&positions, declaration.signature_range) else {
            continue;
        };
        let body_range = declaration
            .body_range
            .and_then(|range| indexed_range(&positions, range));

        let node_kind = if declaration.rule.node_kind == OutlineNodeKind::Constructor {
            OutlineNodeKind::Constructor
        } else {
            match kind {
                FunctionKind::Function => OutlineNodeKind::Function,
                FunctionKind::Method => OutlineNodeKind::Method,
                FunctionKind::Declaration => OutlineNodeKind::Declaration,
            }
        };
        let function = CascadedFunction {
            name: declaration.name.clone(),
            kind,
            range,
            body_range,
            depth,
            start_offset: declaration.signature_range.start,
            end_offset: declaration.signature_range.end,
        };
        let node = OutlineNode::new(
            function.name.clone(),
            node_kind,
            function.range,
            function.body_range,
            depth,
        );

        function_nodes.push((function.clone(), node));
        functions.push(function);
    }
    deduplicate_functions(&mut functions);
    deduplicate_function_nodes(&mut function_nodes);

    nodes.extend(function_nodes.into_iter().map(|(_, node)| node));
    // Containers may be declared inside functions. Attach every kind in source
    // order so each enclosing declaration exists before its descendants.
    nodes.sort_by_key(|node| (node.range.start, Reverse(node.range.end), node.depth));
    let mut roots = Vec::new();
    let mut attachments = AttachmentIndex::default();
    for node in nodes {
        attachments.attach(&mut roots, node);
    }

    CascadeOutput {
        tree: OutlineTree::new(roots),
        functions,
    }
}

fn deduplicate_functions(functions: &mut Vec<CascadedFunction>) {
    functions.sort_by_key(|function| {
        (
            function.end_offset,
            Reverse(function.end_offset.saturating_sub(function.start_offset)),
            function.start_offset,
        )
    });
    functions.dedup_by(|left, right| {
        left.end_offset == right.end_offset && left.name == right.name && left.depth == right.depth
    });
    functions.sort_by_key(|function| function.range.start);
}

fn deduplicate_function_nodes(function_nodes: &mut Vec<(CascadedFunction, OutlineNode)>) {
    function_nodes.sort_by_key(|(function, _)| {
        (
            function.end_offset,
            Reverse(function.end_offset.saturating_sub(function.start_offset)),
            function.start_offset,
        )
    });
    function_nodes.dedup_by(|(left, _), (right, _)| {
        left.end_offset == right.end_offset && left.name == right.name && left.depth == right.depth
    });
    function_nodes.sort_by_key(|(function, _)| function.range.start);
}

fn container_nodes(
    text: &str,
    containers: &[StructuralEvent],
    positions: &TextPositions<'_>,
    intervals: &IntervalCounts,
    declaration_intervals: &IntervalCounts,
) -> Vec<OutlineNode> {
    let mut nodes = Vec::new();
    let mut sorted = containers.to_vec();
    sorted.sort_by_key(|event| (event.signature_range.start, event.signature_range.end));

    for event in &sorted {
        let Some(range) = indexed_range(positions, event.signature_range) else {
            continue;
        };
        let body_range = event
            .body_range
            .and_then(|range| indexed_range(positions, range));
        let StructuralEventKind::Body { owner_kind, .. } = event.kind;
        let first = sorted.partition_point(|candidate| {
            candidate.signature_range.start < event.signature_range.start
        });
        let last = sorted.partition_point(|candidate| {
            candidate.signature_range.start <= event.signature_range.start
        });
        let excluded = sorted[first..last]
            .iter()
            .filter(|container| {
                container.body_range.is_some_and(|range| {
                    range.start <= event.signature_range.start
                        && event.signature_range.start < range.end
                }) && container.signature_range.start == event.signature_range.start
            })
            .count();
        let depth = intervals
            .count(event.signature_range.start)
            .saturating_sub(excluded)
            + declaration_intervals.count(event.signature_range.start);
        let name = text
            .get(event.name_range.start..event.name_range.end)
            .unwrap_or("")
            .to_owned();
        let node = OutlineNode::new(name, owner_kind, range, body_range, depth);

        nodes.push(node);
    }

    nodes
}

fn indexed_range(buffer: &TextPositions<'_>, range: ByteRange) -> Option<super::EditorRange> {
    Some(super::EditorRange::new(
        buffer.position_for_byte_offset(range.start)?,
        buffer.position_for_byte_offset(range.end)?,
    ))
}

struct TextPositions<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> TextPositions<'a> {
    fn new(text: &'a str) -> Self {
        let bytes = text.as_bytes();
        let mut starts = vec![0];
        let mut offset = 0;
        while offset < bytes.len() {
            let first = bytes[offset];
            offset += 1;
            if matches!(first, b'\r' | b'\n') {
                if matches!(
                    (first, bytes.get(offset)),
                    (b'\r', Some(b'\n')) | (b'\n', Some(b'\r'))
                ) {
                    offset += 1;
                }
                starts.push(offset);
            }
        }
        Self { text, starts }
    }

    fn position_for_byte_offset(&self, offset: usize) -> Option<super::EditorPosition> {
        if offset > self.text.len() || !self.text.is_char_boundary(offset) {
            return None;
        }
        let line = self
            .starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        if offset > self.starts[line]
            && offset < self.text.len()
            && matches!(
                (
                    self.text.as_bytes()[offset - 1],
                    self.text.as_bytes()[offset]
                ),
                (b'\r', b'\n') | (b'\n', b'\r')
            )
            && self.starts.get(line + 1) == Some(&(offset + 1))
        {
            return Some(super::EditorPosition::new(line + 1, 0));
        }
        Some(super::EditorPosition::new(line, offset - self.starts[line]))
    }
}

struct IntervalCounts {
    starts: Vec<usize>,
    ends: Vec<usize>,
}

impl IntervalCounts {
    fn new(ranges: impl Iterator<Item = ByteRange>) -> Self {
        let (mut starts, mut ends): (Vec<_>, Vec<_>) = ranges
            .filter(|range| range.start < range.end)
            .map(|range| (range.start, range.end))
            .unzip();
        starts.sort_unstable();
        ends.sort_unstable();
        Self { starts, ends }
    }

    fn count(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset)
            - self.ends.partition_point(|end| *end <= offset)
    }
}

type PendingAttachment = (
    super::EditorPosition,
    super::EditorPosition,
    Vec<usize>,
    usize,
);

#[derive(Default)]
struct AttachmentIndex {
    pending: BinaryHeap<Reverse<PendingAttachment>>,
    ends: BinaryHeap<Reverse<(super::EditorPosition, Vec<usize>)>>,
    active: BTreeMap<Vec<usize>, usize>,
}

impl AttachmentIndex {
    fn attach(&mut self, nodes: &mut Vec<OutlineNode>, node: OutlineNode) {
        let position = node.range.start;
        while self
            .pending
            .peek()
            .is_some_and(|Reverse((start, _, _, _))| *start <= position)
        {
            let Reverse((_, end, path, depth)) = self.pending.pop().unwrap();
            self.ends.push(Reverse((end, path.clone())));
            self.active.insert(path, depth);
        }
        while self
            .ends
            .peek()
            .is_some_and(|Reverse((end, _))| *end <= position)
        {
            let Reverse((_, path)) = self.ends.pop().unwrap();
            self.active.remove(&path);
        }
        let parent = self
            .active
            .iter()
            .rev()
            .find(|(_, depth)| **depth < node.depth)
            .map(|(path, _)| path.clone());
        let mut range = node.body_range.unwrap_or(node.range);
        let depth = node.depth;
        let path = if let Some(mut path) = parent {
            // Match the original recursive walk: a descendant is reachable only
            // while all its ancestors contain the queried position.
            for length in 1..=path.len() {
                let ancestor = node_at_path_mut(nodes, &path[..length]);
                let parent_range = ancestor.body_range.unwrap_or(ancestor.range);
                range.start = range.start.max(parent_range.start);
                range.end = range.end.min(parent_range.end);
            }
            let parent = node_at_path_mut(nodes, &path);
            path.push(parent.children.len());
            parent.children.push(node);
            path
        } else {
            let path = vec![nodes.len()];
            nodes.push(node);
            path
        };
        self.pending
            .push(Reverse((range.start, range.end, path, depth)));
    }
}

#[cfg(test)]
fn nearest_container_path(
    nodes: &[OutlineNode],
    position: super::EditorPosition,
    depth: usize,
) -> Option<Vec<usize>> {
    let mut best = None;

    for (index, node) in nodes.iter().enumerate() {
        let contains = node
            .body_range
            .or(Some(node.range))
            .is_some_and(|range| range.start <= position && position < range.end);
        if !contains || node.depth >= depth {
            continue;
        }

        let mut path = vec![index];
        if let Some(mut child_path) = nearest_container_path(&node.children, position, depth) {
            path.append(&mut child_path);
        }
        best = Some(path);
    }

    best
}

fn node_at_path_mut<'a>(nodes: &'a mut [OutlineNode], path: &[usize]) -> &'a mut OutlineNode {
    let (first, rest) = path.split_first().expect("non-empty outline path");
    let node = &mut nodes[*first];
    if rest.is_empty() {
        node
    } else {
        node_at_path_mut(&mut node.children, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::outline::{
        OutlineRegistry, source::OutlineSource, structure::discover_structure,
    };

    fn old_attach(nodes: &mut Vec<OutlineNode>, node: OutlineNode) {
        if let Some(path) = nearest_container_path(nodes, node.range.start, node.depth) {
            node_at_path_mut(nodes, &path).children.push(node);
        } else {
            nodes.push(node);
        }
    }

    #[test]
    fn indexed_positions_match_prefix_reference_at_every_offset() {
        for text in [
            "",
            "one\r\ntwo\n\rthree",
            "\u{4f60}\u{597d}\r\n\u{e9}",
            "\r\n\r\n\r",
            "a\n\rb\r",
        ] {
            let positions = TextPositions::new(text);
            for offset in 0..=text.len() + 1 {
                assert_eq!(
                    positions.position_for_byte_offset(offset),
                    crate::editor::position_for_byte_offset(text, offset),
                    "{text:?} at {offset}"
                );
            }
        }
    }

    #[test]
    fn indexed_cascade_matches_reference_depth_positions_and_tree() {
        let cases = [
            (
                "rs",
                "// \u{4f60}\u{597d}\nfn caf\u{e9}() {}\r\nfn second() {}\n\rfn last() {}",
            ),
            (
                "rs",
                "mod outer {\r\nimpl Thing {\r\nfn one() { fn nested() {} }\r\nfn two() {}\r\n}\r\n}\r\nfn last() {}",
            ),
            (
                "py",
                "class Outer:\n    def one(self):\n        def nested():\n            pass\n    def two(self):\n        pass\ndef last():\n    pass\n",
            ),
            (
                "js",
                "class Outer { one() { function inner() {} } two() {} }\nfunction last() {}",
            ),
            (
                "rb",
                "module Outer\n  class Inner\n    def one\n      1\n    end\n  end\nend\n",
            ),
            (
                "rs",
                "fn outer() { mod local { fn leaf() {} } fn sibling() {} }",
            ),
            (
                "py",
                "def outer():\n    class Local:\n        def leaf(self):\n            pass\n    def sibling():\n        pass\n",
            ),
            (
                "js",
                "function outer() { class Local { leaf() {} } function sibling() {} }",
            ),
        ];
        for (token, text) in cases {
            let registry = OutlineRegistry::shared();
            let plan = registry.plan_for_syntax(token).unwrap();
            let structure = discover_structure(text, &OutlineSource::new(text, plan), plan);
            let actual = cascade(text, &structure.containers, structure.declarations.clone());
            let mut reference_nodes = Vec::new();
            for event in &structure.containers {
                let range =
                    super::super::structure_support::editor_range(text, event.signature_range)
                        .unwrap();
                let body = event
                    .body_range
                    .and_then(|range| super::super::structure_support::editor_range(text, range));
                let depth = structure
                    .containers
                    .iter()
                    .filter(|other| {
                        other.signature_range.start != event.signature_range.start
                            && other.body_range.is_some_and(|range| {
                                range.start <= event.signature_range.start
                                    && event.signature_range.start < range.end
                            })
                    })
                    .count()
                    + super::super::structure_support::declaration_depth(
                        &structure.declarations,
                        event.signature_range.start,
                    );
                let StructuralEventKind::Body { owner_kind, .. } = event.kind;
                reference_nodes.push(OutlineNode::new(
                    &text[event.name_range.start..event.name_range.end],
                    owner_kind,
                    range,
                    body,
                    depth,
                ));
            }
            for function in &actual.functions {
                let declaration = structure
                    .declarations
                    .iter()
                    .find(|event| {
                        event.name == function.name
                            && event.signature_range.start == function.start_offset
                    })
                    .unwrap();
                let expected_depth = super::super::structure_support::container_depth(
                    &structure.containers,
                    function.start_offset,
                ) + super::super::structure_support::declaration_depth(
                    &structure.declarations,
                    function.start_offset,
                );
                assert_eq!(function.depth, expected_depth, "{token} {}", function.name);
                let expected_kind = if declaration.terminated {
                    FunctionKind::Declaration
                } else if declaration.rule.node_kind == OutlineNodeKind::Method
                    || super::super::structure_support::containing_container(
                        &structure.containers,
                        function.start_offset,
                        &declaration.rule.method_containers,
                    )
                    .is_some()
                {
                    FunctionKind::Method
                } else {
                    FunctionKind::Function
                };
                assert_eq!(function.kind, expected_kind, "{token} {}", function.name);
                assert_eq!(
                    Some(function.range),
                    super::super::structure_support::editor_range(
                        text,
                        declaration.signature_range
                    )
                );
                let kind = match function.kind {
                    FunctionKind::Function => OutlineNodeKind::Function,
                    FunctionKind::Method => OutlineNodeKind::Method,
                    FunctionKind::Declaration => OutlineNodeKind::Declaration,
                };
                reference_nodes.push(OutlineNode::new(
                    &function.name,
                    kind,
                    function.range,
                    function.body_range,
                    function.depth,
                ));
            }
            reference_nodes
                .sort_by_key(|node| (node.range.start, Reverse(node.range.end), node.depth));
            let mut reference = Vec::new();
            for node in reference_nodes {
                old_attach(&mut reference, node);
            }
            assert_eq!(actual.tree.roots, reference, "{token}");
        }
    }

    #[test]
    fn sweep_matches_recursive_attachment_for_overlapping_and_equal_ranges() {
        for seed in 0..32 {
            let mut random = seed + 1u64;
            let mut fast = Vec::new();
            let mut slow = Vec::new();
            let mut index = AttachmentIndex::default();
            for start in 0..200 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let position = |column| super::super::EditorPosition::new(0, column);
                let node = OutlineNode::new(
                    start.to_string(),
                    OutlineNodeKind::Class,
                    super::super::EditorRange::new(
                        position(start / 2),
                        position(start / 2 + (random as usize % 17)),
                    ),
                    None,
                    random as usize % 8,
                );
                index.attach(&mut fast, node.clone());
                old_attach(&mut slow, node);
            }
            assert_eq!(fast, slow, "seed {seed}");
        }
    }
}
