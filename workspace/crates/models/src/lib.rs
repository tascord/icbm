//! Domain models shared across the workspace.
//!
//! Intentionally exercises:
//! * Generic structs and trait bounds
//! * Serde derive
//! * Recursive enums
//! * Builder pattern
//! * Complex lifetimes and `impl Trait`

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utils::{Describable, Id};

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Status::Pending => "pending",
            Status::Running => "running",
            Status::Completed => "completed",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
        };
        write!(f, "{}", s)
    }
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task<P: Serialize + Clone> {
    pub id: Id,
    pub name: String,
    pub status: Status,
    pub payload: P,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

impl<P: Serialize + Clone + std::fmt::Debug> Task<P> {
    pub fn new(name: impl Into<String>, payload: P) -> Self {
        Task {
            id: Id::new(),
            name: name.into(),
            status: Status::Pending,
            payload,
            tags: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn with_meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn transition(&mut self, next: Status) -> Result<(), StatusError> {
        use Status::*;
        let allowed = matches!(
            (self.status, next),
            (Pending, Running)
                | (Running, Completed)
                | (Running, Failed)
                | (Pending, Cancelled)
                | (Running, Cancelled)
        );
        if allowed {
            self.status = next;
            Ok(())
        } else {
            Err(StatusError::InvalidTransition {
                from: self.status,
                to: next,
            })
        }
    }
}

impl<P: Serialize + Clone + std::fmt::Debug> Describable for Task<P> {
    fn describe(&self) -> String {
        format!("Task[{}] '{}' ({})", self.id, self.name, self.status)
    }
}

// ---------------------------------------------------------------------------
// StatusError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum StatusError {
    #[error("Invalid status transition: {from} → {to}")]
    InvalidTransition { from: Status, to: Status },
}

// ---------------------------------------------------------------------------
// Metric
// ---------------------------------------------------------------------------

/// A named, timestamped floating-point measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub tags: Vec<String>,
}

impl Metric {
    pub fn new(name: impl Into<String>, value: f64, unit: impl Into<String>) -> Self {
        Metric {
            name: name.into(),
            value,
            unit: unit.into(),
            tags: vec![],
        }
    }
}

impl Describable for Metric {
    fn describe(&self) -> String {
        format!("{}: {} {}", self.name, self.value, self.unit)
    }
}

// ---------------------------------------------------------------------------
// Tree<T> – a recursive generic tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree<T: Clone> {
    pub value: T,
    pub children: Vec<Tree<T>>,
}

impl<T: Clone> Tree<T> {
    pub fn leaf(value: T) -> Self {
        Tree {
            value,
            children: vec![],
        }
    }

    pub fn with_child(mut self, child: Tree<T>) -> Self {
        self.children.push(child);
        self
    }

    /// Depth-first traversal, calling `f` for every node.
    pub fn walk<F: FnMut(&T)>(&self, f: &mut F) {
        f(&self.value);
        for child in &self.children {
            child.walk(f);
        }
    }

    /// Returns the depth of the deepest leaf.
    pub fn depth(&self) -> usize {
        if self.children.is_empty() {
            0
        } else {
            1 + self.children.iter().map(Tree::depth).max().unwrap_or(0)
        }
    }

    /// Returns the total number of nodes.
    pub fn size(&self) -> usize {
        1 + self.children.iter().map(Tree::size).sum::<usize>()
    }
}

// ---------------------------------------------------------------------------
// Event<D> – a typed event envelope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event<D: Clone + Serialize> {
    pub id: Id,
    pub kind: String,
    pub data: D,
    pub version: u32,
}

impl<D: Clone + Serialize> Event<D> {
    pub fn new(kind: impl Into<String>, data: D) -> Self {
        Event {
            id: Id::new(),
            kind: kind.into(),
            data,
            version: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_transitions() {
        let mut t = Task::new("demo", 42u32);
        assert_eq!(t.status, Status::Pending);
        t.transition(Status::Running).unwrap();
        t.transition(Status::Completed).unwrap();
        assert_eq!(t.status, Status::Completed);
    }

    #[test]
    fn task_invalid_transition() {
        let mut t = Task::new("demo", 42u32);
        let err = t.transition(Status::Completed);
        assert!(err.is_err());
    }

    #[test]
    fn tree_depth_and_size() {
        let tree = Tree::leaf(1)
            .with_child(Tree::leaf(2).with_child(Tree::leaf(3)))
            .with_child(Tree::leaf(4));
        assert_eq!(tree.depth(), 2);
        assert_eq!(tree.size(), 4);
    }

    #[test]
    fn tree_walk() {
        let tree = Tree::leaf("a").with_child(Tree::leaf("b")).with_child(Tree::leaf("c"));
        let mut visited = vec![];
        tree.walk(&mut |v| visited.push(*v));
        assert_eq!(visited, vec!["a", "b", "c"]);
    }
}
