// goals.rs — Goal and task management for unit mesh
//
// Goals are human-provided high-level objectives. Tasks are unit-level
// work items derived from goals. The GoalRegistry is shared across the
// mesh via gossip so all units maintain a consistent view of current work.
//
// Lifecycle:
//   human submits goal → goal broadcast → task created → unit claims task
//   → unit works → unit reports result → goal completed
//
// Humans set direction, the mesh navigates.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::mesh::{id_to_hex, NodeId};
use super::Cell;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub type GoalId = u64;
pub type TaskId = u64;

#[derive(Clone, Debug, PartialEq)]
pub enum GoalStatus {
    Pending,   // submitted, no tasks claimed yet
    Active,    // at least one task is being worked
    Completed, // all tasks done
    Failed,    // cancelled or failed
}

impl GoalStatus {
    pub fn as_u8(&self) -> u8 {
        match self {
            GoalStatus::Pending => 0,
            GoalStatus::Active => 1,
            GoalStatus::Completed => 2,
            GoalStatus::Failed => 3,
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => GoalStatus::Active,
            2 => GoalStatus::Completed,
            3 => GoalStatus::Failed,
            _ => GoalStatus::Pending,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            GoalStatus::Pending => "pending",
            GoalStatus::Active => "active",
            GoalStatus::Completed => "completed",
            GoalStatus::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TaskStatus {
    Waiting, // unclaimed, in queue
    Running, // claimed by a unit
    Done,    // completed successfully
    Failed,  // failed or cancelled
}

impl TaskStatus {
    pub fn as_u8(&self) -> u8 {
        match self {
            TaskStatus::Waiting => 0,
            TaskStatus::Running => 1,
            TaskStatus::Done => 2,
            TaskStatus::Failed => 3,
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => TaskStatus::Running,
            2 => TaskStatus::Done,
            3 => TaskStatus::Failed,
            _ => TaskStatus::Waiting,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            TaskStatus::Waiting => "waiting",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
        }
    }
}

// ---------------------------------------------------------------------------
// Goal
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Goal {
    pub id: GoalId,
    pub description: String,
    pub priority: Cell,
    pub status: GoalStatus,
    pub created_at: u64,
    pub creator: NodeId,
    pub task_ids: Vec<TaskId>,
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Task {
    pub id: TaskId,
    pub goal_id: GoalId,
    pub description: String,
    pub assigned_to: Option<NodeId>,
    pub status: TaskStatus,
    pub result: Option<String>,
    pub created_at: u64,
}

// ---------------------------------------------------------------------------
// GoalRegistry — manages all goals and tasks for a unit
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct GoalRegistry {
    pub goals: HashMap<GoalId, Goal>,
    pub tasks: HashMap<TaskId, Task>,
    id_counter: u64,
}

impl GoalRegistry {
    /// Create a new registry. The counter is seeded from the node ID to
    /// avoid collisions between nodes generating IDs concurrently.
    pub fn new(node_id: &NodeId) -> Self {
        // Seed from node ID to avoid collisions. Keep IDs small and typeable.
        let base = ((node_id[6] as u64) << 4 | (node_id[7] as u64 >> 4)) * 10 + 1;
        GoalRegistry {
            goals: HashMap::new(),
            tasks: HashMap::new(),
            id_counter: base,
        }
    }

    /// Create an empty registry (for deserialization).
    pub fn empty() -> Self {
        GoalRegistry {
            goals: HashMap::new(),
            tasks: HashMap::new(),
            id_counter: 1,
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.id_counter;
        self.id_counter += 1;
        id
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // -------------------------------------------------------------------
    // Goal lifecycle
    // -------------------------------------------------------------------

    /// Create a new goal with one initial task. Returns the goal ID.
    pub fn create_goal(
        &mut self,
        description: String,
        priority: Cell,
        creator: NodeId,
    ) -> GoalId {
        let goal_id = self.next_id();
        let task_id = self.next_id();
        let now = Self::now_millis();

        let task = Task {
            id: task_id,
            goal_id,
            description: description.clone(),
            assigned_to: None,
            status: TaskStatus::Waiting,
            result: None,
            created_at: now,
        };

        let goal = Goal {
            id: goal_id,
            description,
            priority,
            status: GoalStatus::Pending,
            created_at: now,
            creator,
            task_ids: vec![task_id],
        };

        self.tasks.insert(task_id, task);
        self.goals.insert(goal_id, goal);
        goal_id
    }

    /// Claim the highest-priority unclaimed task.
    /// Returns (task_id, goal_id, description) or None if nothing available.
    pub fn claim_task(&mut self, node_id: NodeId) -> Option<(TaskId, GoalId, String)> {
        // Gather unclaimed tasks with their parent goal priority.
        let mut candidates: Vec<(TaskId, Cell)> = self
            .tasks
            .iter()
            .filter(|(_, t)| t.status == TaskStatus::Waiting)
            .filter_map(|(tid, t)| {
                self.goals.get(&t.goal_id).map(|g| (*tid, g.priority))
            })
            .collect();

        // Highest priority first.
        candidates.sort_by(|a, b| b.1.cmp(&a.1));

        if let Some(&(task_id, _)) = candidates.first() {
            if let Some(task) = self.tasks.get_mut(&task_id) {
                task.assigned_to = Some(node_id);
                task.status = TaskStatus::Running;
                let goal_id = task.goal_id;
                let desc = task.description.clone();

                // Transition goal to Active.
                if let Some(goal) = self.goals.get_mut(&goal_id) {
                    if goal.status == GoalStatus::Pending {
                        goal.status = GoalStatus::Active;
                    }
                }
                return Some((task_id, goal_id, desc));
            }
        }
        None
    }

    /// Mark a task as done. Completes the parent goal if all tasks are done.
    pub fn complete_task(&mut self, task_id: TaskId, result: Option<String>) -> bool {
        let goal_id = if let Some(task) = self.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Done;
            task.result = result;
            task.goal_id
        } else {
            return false;
        };

        // Check if all tasks for this goal are now done.
        if let Some(goal) = self.goals.get(&goal_id) {
            let all_done = goal
                .task_ids
                .iter()
                .all(|tid| {
                    self.tasks
                        .get(tid)
                        .map(|t| t.status == TaskStatus::Done)
                        .unwrap_or(true)
                });
            if all_done {
                if let Some(g) = self.goals.get_mut(&goal_id) {
                    g.status = GoalStatus::Completed;
                }
            }
        }
        true
    }

    /// Cancel a goal and all its non-completed tasks.
    pub fn cancel_goal(&mut self, goal_id: GoalId) -> bool {
        if let Some(goal) = self.goals.get_mut(&goal_id) {
            goal.status = GoalStatus::Failed;
            let task_ids = goal.task_ids.clone();
            for tid in &task_ids {
                if let Some(task) = self.tasks.get_mut(tid) {
                    if task.status != TaskStatus::Done {
                        task.status = TaskStatus::Failed;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Change a goal's priority.
    pub fn steer_goal(&mut self, goal_id: GoalId, new_priority: Cell) -> bool {
        if let Some(goal) = self.goals.get_mut(&goal_id) {
            goal.priority = new_priority;
            true
        } else {
            false
        }
    }

    // -------------------------------------------------------------------
    // Gossip convergence — merge remote state into local registry
    // -------------------------------------------------------------------

    /// Merge a received goal. Only updates if the incoming state is newer
    /// (status further along the lifecycle, or priority changed).
    pub fn merge_goal(&mut self, goal: Goal) {
        if let Some(existing) = self.goals.get(&goal.id) {
            if goal.status.as_u8() > existing.status.as_u8()
                || goal.priority != existing.priority
            {
                // Preserve local task IDs, merge any new ones.
                let mut merged_tasks = existing.task_ids.clone();
                for tid in &goal.task_ids {
                    if !merged_tasks.contains(tid) {
                        merged_tasks.push(*tid);
                    }
                }
                let mut merged = goal;
                merged.task_ids = merged_tasks;
                self.goals.insert(merged.id, merged);
            }
        } else {
            // Ensure counter stays ahead of received IDs.
            if goal.id >= self.id_counter {
                self.id_counter = goal.id + 1;
            }
            self.goals.insert(goal.id, goal);
        }
    }

    /// Merge a received task.
    pub fn merge_task(&mut self, task: Task) {
        if let Some(existing) = self.tasks.get(&task.id) {
            if task.status.as_u8() > existing.status.as_u8() {
                self.tasks.insert(task.id, task);
            }
        } else {
            if task.id >= self.id_counter {
                self.id_counter = task.id + 1;
            }
            self.tasks.insert(task.id, task);
        }
    }

    // -------------------------------------------------------------------
    // Queries
    // -------------------------------------------------------------------

    /// Count of tasks in Waiting status.
    pub fn pending_task_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|t| t.status == TaskStatus::Waiting)
            .count()
    }

    /// Count of active (non-completed, non-failed) goals.
    pub fn active_goal_count(&self) -> usize {
        self.goals
            .values()
            .filter(|g| g.status == GoalStatus::Pending || g.status == GoalStatus::Active)
            .count()
    }

    // -------------------------------------------------------------------
    // Formatting for Forth display
    // -------------------------------------------------------------------

    /// Format all goals for the GOALS word.
    pub fn format_goals(&self) -> String {
        if self.goals.is_empty() {
            return "  (no goals)\n".to_string();
        }
        let mut goals: Vec<&Goal> = self.goals.values().collect();
        goals.sort_by(|a, b| b.priority.cmp(&a.priority));

        let mut out = String::new();
        for g in &goals {
            let total = g.task_ids.len();
            let done = g
                .task_ids
                .iter()
                .filter(|tid| {
                    self.tasks
                        .get(tid)
                        .map(|t| t.status == TaskStatus::Done)
                        .unwrap_or(false)
                })
                .count();
            out.push_str(&format!(
                "  #{} [{}] p={} ({}/{} tasks): {}\n",
                g.id,
                g.status.label(),
                g.priority,
                done,
                total,
                g.description
            ));
        }
        out
    }

    /// Format this unit's task queue for the TASKS word.
    pub fn format_my_tasks(&self, node_id: &NodeId) -> String {
        let mut my_tasks: Vec<&Task> = self
            .tasks
            .values()
            .filter(|t| t.assigned_to.as_ref() == Some(node_id))
            .collect();

        if my_tasks.is_empty() {
            return "  (no tasks claimed)\n".to_string();
        }

        my_tasks.sort_by_key(|t| std::cmp::Reverse(t.created_at));

        let mut out = String::new();
        for t in &my_tasks {
            out.push_str(&format!(
                "  task #{} [{}] goal #{}: {}\n",
                t.id,
                t.status.label(),
                t.goal_id,
                t.description
            ));
        }
        out
    }

    /// Format task breakdown for a specific goal (TASK-STATUS word).
    pub fn format_goal_tasks(&self, goal_id: GoalId) -> String {
        if let Some(goal) = self.goals.get(&goal_id) {
            let mut out = format!(
                "goal #{} [{}] p={}: {}\n",
                goal.id,
                goal.status.label(),
                goal.priority,
                goal.description
            );
            out.push_str(&format!(
                "  creator: {}\n",
                id_to_hex(&goal.creator)
            ));
            for tid in &goal.task_ids {
                if let Some(task) = self.tasks.get(tid) {
                    let assignee = task
                        .assigned_to
                        .as_ref()
                        .map(id_to_hex)
                        .unwrap_or_else(|| "unassigned".to_string());
                    out.push_str(&format!(
                        "  task #{} [{}] -> {}: {}\n",
                        task.id,
                        task.status.label(),
                        assignee,
                        task.description,
                    ));
                    if let Some(ref result) = task.result {
                        out.push_str(&format!("    result: {}\n", result));
                    }
                }
            }
            out
        } else {
            format!("goal #{} not found\n", goal_id)
        }
    }

    /// Format mesh-wide progress report (REPORT word).
    pub fn format_report(&self) -> String {
        let total_goals = self.goals.len();
        let g_pending = self
            .goals
            .values()
            .filter(|g| g.status == GoalStatus::Pending)
            .count();
        let g_active = self
            .goals
            .values()
            .filter(|g| g.status == GoalStatus::Active)
            .count();
        let g_done = self
            .goals
            .values()
            .filter(|g| g.status == GoalStatus::Completed)
            .count();
        let g_failed = self
            .goals
            .values()
            .filter(|g| g.status == GoalStatus::Failed)
            .count();

        let total_tasks = self.tasks.len();
        let t_waiting = self
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Waiting)
            .count();
        let t_running = self
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Running)
            .count();
        let t_done = self
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Done)
            .count();
        let t_failed = self
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Failed)
            .count();

        // Collect unique assignees.
        let mut workers: Vec<NodeId> = self
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Running)
            .filter_map(|t| t.assigned_to)
            .collect();
        workers.sort();
        workers.dedup();

        let mut out = String::from("--- mesh progress report ---\n");
        out.push_str(&format!(
            "goals: {} total ({} pending, {} active, {} completed, {} failed)\n",
            total_goals, g_pending, g_active, g_done, g_failed
        ));
        out.push_str(&format!(
            "tasks: {} total ({} waiting, {} running, {} done, {} failed)\n",
            total_tasks, t_waiting, t_running, t_done, t_failed
        ));
        out.push_str(&format!("workers: {} active units\n", workers.len()));
        out.push_str("---\n");
        out
    }
}
