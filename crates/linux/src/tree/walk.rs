use agent_desktop_core::{Deadline, LocatorStats, ObservationBudget, ObservedSubtree};
use futures_util::future::join_all;
use zbus::Connection;

use super::deadline;
use super::element::ElementRef;
use super::node_read::{self, DetailPlan, NodeBasics, NodeDetails};

/// Elements read concurrently in one round. AT-SPI2 has no batched
/// multi-attribute read, so the walk pipelines one level at a time instead;
/// the chunk keeps the in-flight calls well under the bus daemon's pending
/// reply limit.
const CONCURRENT_ELEMENTS: usize = 24;

#[derive(Clone, Copy)]
pub(crate) struct WalkRequest {
    pub(crate) max_logical_depth: u8,
    pub(crate) max_raw_depth: u8,
    pub(crate) budget: ObservationBudget,
    pub(crate) deadline: Deadline,
    pub(crate) bounds_trusted: bool,
}

pub(crate) struct WalkOutcome {
    pub(crate) root: Option<ObservedSubtree>,
    pub(crate) stats: LocatorStats,
    pub(crate) root_defunct: bool,
}

struct Pending {
    element: ElementRef,
    parent: Option<usize>,
    native_index: usize,
    logical_depth: u8,
    raw_depth: u8,
}

struct Slot {
    element: ElementRef,
    position: Position,
    basics: NodeBasics,
    details: NodeDetails,
    children: Vec<usize>,
    children_count: Option<u32>,
    complete: bool,
}

#[derive(Clone, Copy)]
struct Position {
    parent: Option<usize>,
    native_index: usize,
}

struct Walker<'a> {
    connection: &'a Connection,
    request: WalkRequest,
    slots: Vec<Slot>,
    stats: LocatorStats,
    root_defunct: bool,
}

/// Walks the subtree under `root` breadth-first, one pipelined level at a
/// time, and reports what it observed even when the deadline or a budget cut
/// it short: every node whose children were not all observed is marked
/// incomplete, and a node at the requested depth boundary carries its real
/// `children_count` instead.
pub(crate) async fn walk(
    connection: &Connection,
    root: ElementRef,
    request: WalkRequest,
) -> WalkOutcome {
    let mut walker = Walker {
        connection,
        request,
        slots: Vec::new(),
        stats: LocatorStats::default(),
        root_defunct: false,
    };
    let mut level = vec![Pending {
        element: root,
        parent: None,
        native_index: 0,
        logical_depth: 0,
        raw_depth: 0,
    }];
    while !level.is_empty() {
        level = walker.visit_level(level).await;
    }
    let root = walker.assemble();
    WalkOutcome {
        root,
        stats: walker.stats,
        root_defunct: walker.root_defunct,
    }
}

impl Walker<'_> {
    async fn visit_level(&mut self, level: Vec<Pending>) -> Vec<Pending> {
        let mut next = Vec::new();
        let mut chunks = level.into_iter().peekable();
        while chunks.peek().is_some() {
            let chunk: Vec<Pending> = chunks.by_ref().take(CONCURRENT_ELEMENTS).collect();
            match self.visit_chunk(chunk).await {
                Ok(children) => next.extend(children),
                Err(abandoned) => {
                    self.abandon(abandoned.into_iter().chain(chunks));
                    return Vec::new();
                }
            }
        }
        next
    }

    async fn visit_chunk(&mut self, chunk: Vec<Pending>) -> Result<Vec<Pending>, Vec<Pending>> {
        let connection = self.connection;
        let reads = chunk
            .iter()
            .map(|pending| node_read::read_basics(connection, &pending.element));
        let Some(basics) = deadline::until(self.request.deadline, join_all(reads)).await else {
            return Err(chunk);
        };
        let mut accepted = Vec::new();
        for (pending, basics) in chunk.into_iter().zip(basics) {
            if let Some(plan) = self.admit(&pending, &basics) {
                accepted.push((pending, basics, plan));
            }
        }
        let reads = accepted.iter().map(|(pending, basics, plan)| {
            node_read::read_details(connection, &pending.element, basics, *plan)
        });
        let Some(details) = deadline::until(self.request.deadline, join_all(reads)).await else {
            return Err(accepted
                .into_iter()
                .map(|(pending, _, _)| pending)
                .collect());
        };
        let mut next = Vec::new();
        for ((pending, basics, plan), details) in accepted.into_iter().zip(details) {
            next.extend(self.record(pending, basics, plan, details));
        }
        Ok(next)
    }

    /// Decides whether a read element joins the observation and what its
    /// second round should fetch. A descendant without `VISIBLE` is left out
    /// with its subtree: toolkits keep hidden widgets (Chromium's unused
    /// toolbar buttons, a closed select's options) in the tree, and a user
    /// or screen reader never meets them. Leaving them out is not a gap in
    /// the observation, so it does not mark the parent incomplete.
    fn admit(&mut self, pending: &Pending, basics: &NodeBasics) -> Option<DetailPlan> {
        if basics.is_defunct() {
            self.stats.reads.health.native_read_failures += 1;
            if pending.parent.is_none() {
                self.root_defunct = true;
            }
            self.mark_parent_incomplete(pending.parent);
            return None;
        }
        if pending.parent.is_some() && basics.states.is_some_and(|states| !states.visible()) {
            return None;
        }
        if self.slots.len() >= self.request.budget.max_nodes {
            self.stats.traversal.limits.node_hits += 1;
            self.mark_parent_incomplete(pending.parent);
            return None;
        }
        if self.is_ancestor(pending.parent, &pending.element) {
            self.stats.traversal.cycles_skipped += 1;
            self.mark_parent_incomplete(pending.parent);
            return None;
        }
        let child_logical_depth = pending.logical_depth + u8::from(!basics.is_web_wrapper());
        let has_children = basics.child_count().is_none_or(|count| count > 0);
        let within_logical = child_logical_depth <= self.request.max_logical_depth;
        let within_raw = pending.raw_depth < self.request.max_raw_depth;
        Some(DetailPlan {
            bounds_trusted: self.request.bounds_trusted,
            children: has_children && within_logical && within_raw,
            max_field_bytes: self.request.budget.max_field_bytes,
        })
    }

    fn record(
        &mut self,
        pending: Pending,
        basics: NodeBasics,
        plan: DetailPlan,
        details: NodeDetails,
    ) -> Vec<Pending> {
        let index = self.slots.len();
        self.note_visit(&pending, &basics);
        let child_logical_depth = pending.logical_depth + u8::from(!basics.is_web_wrapper());
        let mut complete = basics.role_code.is_some() && basics.states.is_some();
        let mut children_count = None;
        let mut next = Vec::new();
        let boundary_count = basics.child_count().filter(|count| *count > 0);
        if !plan.children && boundary_count.is_some() {
            if child_logical_depth <= self.request.max_logical_depth {
                self.stats.traversal.limits.depth_hits += 1;
                complete = false;
            } else {
                children_count = boundary_count;
            }
        }
        if plan.children {
            self.stats.reads.counts.child_reads += 1;
            match &details.children {
                Some(children) => {
                    let limit = self.request.budget.max_children_per_node;
                    if children.len() > limit {
                        self.stats.traversal.limits.child_hits += 1;
                        complete = false;
                    }
                    next = children
                        .iter()
                        .take(limit)
                        .enumerate()
                        .map(|(native_index, element)| Pending {
                            element: element.clone(),
                            parent: Some(index),
                            native_index,
                            logical_depth: child_logical_depth,
                            raw_depth: pending.raw_depth.saturating_add(1),
                        })
                        .collect();
                }
                None => {
                    self.stats.reads.health.native_read_failures += 1;
                    complete = false;
                }
            }
        }
        if let Some(parent) = pending.parent {
            self.slots[parent].children.push(index);
        }
        self.slots.push(Slot {
            element: pending.element,
            position: Position {
                parent: pending.parent,
                native_index: pending.native_index,
            },
            basics,
            details,
            children: Vec::new(),
            children_count,
            complete,
        });
        next
    }

    fn note_visit(&mut self, pending: &Pending, basics: &NodeBasics) {
        let traversal = &mut self.stats.traversal;
        traversal.nodes_visited += 1;
        traversal.max_raw_depth = traversal.max_raw_depth.max(pending.raw_depth);
        traversal.max_logical_depth = traversal.max_logical_depth.max(pending.logical_depth);
        if basics.is_web_wrapper() {
            traversal.web_wrapper_nodes += 1;
        }
    }

    fn abandon(&mut self, pending: impl Iterator<Item = Pending>) {
        self.stats.reads.health.deadline_exhausted += 1;
        for pending in pending {
            self.mark_parent_incomplete(pending.parent);
        }
    }

    fn mark_parent_incomplete(&mut self, parent: Option<usize>) {
        if let Some(parent) = parent.and_then(|index| self.slots.get_mut(index)) {
            parent.complete = false;
        }
    }

    fn is_ancestor(&self, mut parent: Option<usize>, element: &ElementRef) -> bool {
        while let Some(index) = parent {
            let Some(slot) = self.slots.get(index) else {
                return false;
            };
            if &slot.element == element {
                return true;
            }
            parent = slot.position.parent;
        }
        false
    }

    fn assemble(&mut self) -> Option<ObservedSubtree> {
        if self.slots.is_empty() {
            return None;
        }
        Some(self.subtree(0))
    }

    fn subtree(&self, index: usize) -> ObservedSubtree {
        let slot = &self.slots[index];
        let children = slot
            .children
            .iter()
            .map(|child| {
                self.subtree(*child)
                    .with_source_child_index(self.slots[*child].position.native_index)
            })
            .collect();
        ObservedSubtree::new(
            slot.basics.evidence(&slot.details),
            children,
            slot.complete,
            slot.children_count,
        )
    }
}
