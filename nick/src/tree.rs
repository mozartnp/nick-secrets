//! The tree of the panel (NS-38): Module > Project > Ticket > Agent, as a pure model, with no
//! database, no screen and no disk. The App fills a `TreeData` from the database and its running
//! agents; here are the nodes by identity (`NodeId`, the selection survives a refresh), the
//! open/closed state of the branches, the flattening into the visible rows with their depth, the
//! two filters (the hidden tickets, `show_hidden`; only the live agents, `only_active`), the "saída
//! nova" mark that goes up to the closed ancestors, and where the selection goes when its node is
//! gone (`reselect`). ui.rs draws the rows; app.rs moves on them.

use std::collections::{HashMap, HashSet};

use crate::agents::{Agent, AgentState};
use crate::app::Row;
use crate::modules::ModuleKind;
use crate::projects::Availability;
use crate::tickets::Ticket;

/// A node of the tree, by identity: the selection survives a refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeId {
    Module(ModuleKind),
    Project(i64),
    Ticket(i64),
    Agent(i64),
}

/// A live agent of this nick: where it is in the tree and whether it is marked "● saída nova".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveAgent {
    pub id: i64,
    pub ticket_id: i64,
    pub project_id: i64,
    pub unseen: bool,
}

/// What the tree is built from (the App fills it from the database and its running agents).
pub struct TreeData<'a> {
    /// The module whose projects are in `projects` (the only one today).
    pub module: ModuleKind,
    /// The projects of the module, with their availability.
    pub projects: &'a [Row],
    /// The tickets of each project (by project id), the hidden ones included.
    pub tickets: &'a HashMap<i64, Vec<Ticket>>,
    /// The agents of each ticket (by ticket id), of every stage and state, in the order they were
    /// opened.
    pub agents: &'a HashMap<i64, Vec<Agent>>,
    /// The agents this nick runs.
    pub live: &'a [LiveAgent],
}

/// The open branches and the filters. Every branch starts closed.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    open: HashSet<NodeId>,
    /// The hidden tickets are listed too, and the archived agents (`f`; NS-39).
    pub show_hidden: bool,
    /// Only the branches with a live agent of this nick, all open (`v`).
    pub only_active: bool,
}

/// A visible row: the node, its depth (0..=3), whether it can open, whether it is open, and
/// whether a live agent below it has new output (only when it is closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visible {
    pub id: NodeId,
    pub depth: u16,
    pub expandable: bool,
    pub open: bool,
    pub unseen_below: bool,
}

impl Tree {
    /// The rows on the screen, in order: each module (the root, even alone); its projects (an
    /// unavailable one does not open, but for one with a live agent); the tickets of an open
    /// project, without the hidden ones unless `show_hidden` or one of its agents is live (decision
    /// 7); the agents of an open ticket, in the order they were opened, without the archived ones
    /// unless `show_hidden` (NS-39; a ticket with only archived agents is then a leaf). With
    /// `only_active`, only the branches with a live agent and only the live agents, every branch
    /// open, whatever the state kept.
    pub fn visible(&self, data: &TreeData) -> Vec<Visible> {
        let live_in = |test: &dyn Fn(&LiveAgent) -> bool| data.live.iter().any(test);
        let unseen_in = |test: &dyn Fn(&LiveAgent) -> bool| {
            data.live.iter().any(|agent| agent.unseen && test(agent))
        };
        let mut rows = Vec::new();
        for &module in ModuleKind::ALL {
            let projects: Vec<&Row> = if module == data.module {
                data.projects
                    .iter()
                    .filter(|row| !self.only_active || live_in(&|a| a.project_id == row.project.id))
                    .collect()
            } else {
                Vec::new()
            };
            let ids: Vec<i64> = projects.iter().map(|row| row.project.id).collect();
            let expandable = !projects.is_empty();
            let open = expandable && (self.only_active || self.is_open(NodeId::Module(module)));
            rows.push(Visible {
                id: NodeId::Module(module),
                depth: 0,
                expandable,
                open,
                unseen_below: !open && unseen_in(&|a| ids.contains(&a.project_id)),
            });
            if !open {
                continue;
            }
            for row in projects {
                let project = row.project.id;
                // An unavailable project does not open (decision 11 of NS-38), unless a live
                // agent of this nick is below it: every live agent stays reachable (decision 7).
                let expandable = self.only_active
                    || row.availability == Availability::Available
                    || live_in(&|a| a.project_id == project);
                let open =
                    expandable && (self.only_active || self.is_open(NodeId::Project(project)));
                rows.push(Visible {
                    id: NodeId::Project(project),
                    depth: 1,
                    expandable,
                    open,
                    unseen_below: !open && unseen_in(&|a| a.project_id == project),
                });
                if !open {
                    continue;
                }
                for ticket in data.tickets.get(&project).map_or(&[][..], Vec::as_slice) {
                    let has_live = live_in(&|a| a.ticket_id == ticket.id);
                    let shown = if self.only_active {
                        has_live
                    } else {
                        ticket.hidden_at.is_none() || self.show_hidden || has_live
                    };
                    if !shown {
                        continue;
                    }
                    let agents: Vec<&Agent> = data
                        .agents
                        .get(&ticket.id)
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .filter(|agent| !self.only_active || live_in(&|a| a.id == agent.id))
                        // An archived agent only with the hidden ones (`f`, NS-39).
                        .filter(|agent| {
                            self.show_hidden || agent.state != AgentState::Archived.key()
                        })
                        .collect();
                    let expandable = !agents.is_empty();
                    let open =
                        expandable && (self.only_active || self.is_open(NodeId::Ticket(ticket.id)));
                    rows.push(Visible {
                        id: NodeId::Ticket(ticket.id),
                        depth: 2,
                        expandable,
                        open,
                        unseen_below: !open && unseen_in(&|a| a.ticket_id == ticket.id),
                    });
                    if !open {
                        continue;
                    }
                    for agent in agents {
                        rows.push(Visible {
                            id: NodeId::Agent(agent.id),
                            depth: 3,
                            expandable: false,
                            open: false,
                            unseen_below: false,
                        });
                    }
                }
            }
        }
        rows
    }

    /// Whether `id` is open in the state kept (the filter `v` shows every branch open without
    /// changing it).
    pub fn is_open(&self, id: NodeId) -> bool {
        self.open.contains(&id)
    }

    pub fn open(&mut self, id: NodeId) {
        self.open.insert(id);
    }

    pub fn close(&mut self, id: NodeId) {
        self.open.remove(&id);
    }

    /// The parent of `id`, as far as `data` knows it.
    /// A live agent's ticket comes from the running ones (it may not be read from the database
    /// yet).
    pub fn parent(&self, id: NodeId, data: &TreeData) -> Option<NodeId> {
        match id {
            NodeId::Module(_) => None,
            NodeId::Project(project) => data
                .projects
                .iter()
                .any(|row| row.project.id == project)
                .then_some(NodeId::Module(data.module)),
            NodeId::Ticket(ticket) => data
                .tickets
                .values()
                .flatten()
                .find(|t| t.id == ticket)
                .map(|t| NodeId::Project(t.project_id)),
            NodeId::Agent(agent) => data
                .live
                .iter()
                .find(|a| a.id == agent)
                .map(|a| a.ticket_id)
                .or_else(|| {
                    data.agents
                        .values()
                        .flatten()
                        .find(|a| a.id == agent)
                        .map(|a| a.ticket_id)
                })
                .map(NodeId::Ticket),
        }
    }

    /// Opens every ancestor of `id` (to select an agent from the shortcut, or a new node).
    pub fn reveal(&mut self, id: NodeId, data: &TreeData) {
        let mut next = self.parent(id, data);
        while let Some(parent) = next {
            self.open(parent);
            next = self.parent(parent, data);
        }
    }

    /// After a refresh: the selection when `selected` is gone from the rows (the next sibling,
    /// else the previous one, else the closest ancestor still there); `selected` itself when it
    /// is still there. What it was is read in `before`, the rows before the refresh.
    pub fn reselect(before: &[Visible], after: &[Visible], selected: NodeId) -> NodeId {
        let there = |id: NodeId| after.iter().any(|row| row.id == id);
        let first = || after.first().map_or(selected, |row| row.id);
        if there(selected) {
            return selected;
        }
        let Some(at) = before.iter().position(|row| row.id == selected) else {
            return first();
        };
        let depth = before[at].depth;
        let siblings_after = before[at + 1..].iter().take_while(|row| row.depth >= depth);
        let siblings_before = before[..at]
            .iter()
            .rev()
            .take_while(|row| row.depth >= depth);
        let sibling = siblings_after
            .chain(siblings_before)
            .find(|row| row.depth == depth && there(row.id));
        if let Some(row) = sibling {
            return row.id;
        }
        let mut level = depth;
        for row in before[..at].iter().rev() {
            if row.depth < level {
                if there(row.id) {
                    return row.id;
                }
                level = row.depth;
            }
        }
        first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::Project;
    use std::path::PathBuf;

    const M: NodeId = NodeId::Module(ModuleKind::AutoScrum);

    fn row(id: i64, available: bool) -> Row {
        Row {
            project: Project {
                id,
                name: format!("p{id}"),
                path: PathBuf::from(format!("/p{id}")),
                created_at: String::new(),
            },
            availability: if available {
                Availability::Available
            } else {
                Availability::DirMissing
            },
        }
    }

    fn ticket(id: i64, project_id: i64, hidden: bool) -> Ticket {
        Ticket {
            id,
            project_id,
            key: format!("T-{id}"),
            is_provisional: false,
            title: String::new(),
            created_at: String::new(),
            hidden_at: hidden.then(|| "2026-11-20T00:00:00Z".to_string()),
            stage: "po".into(),
        }
    }

    fn agent(id: i64, ticket_id: i64) -> Agent {
        Agent {
            id,
            ticket_id,
            stage: "po".into(),
            session_id: format!("s{id}"),
            name: format!("a{id}"),
            opened_at: String::new(),
            state: "stopped".into(),
            opened_key: None,
        }
    }

    /// Projects 1 (available) and 2 (unavailable); tickets 10, 11 (hidden) and 12 of project 1,
    /// 20 of project 2; agents 100 and 101 of ticket 10, 110 of ticket 11, 200 of ticket 20.
    struct Data {
        projects: Vec<Row>,
        tickets: HashMap<i64, Vec<Ticket>>,
        agents: HashMap<i64, Vec<Agent>>,
        live: Vec<LiveAgent>,
    }

    impl Data {
        fn new() -> Data {
            Data {
                projects: vec![row(1, true), row(2, false)],
                tickets: HashMap::from([
                    (
                        1,
                        vec![
                            ticket(10, 1, false),
                            ticket(11, 1, true),
                            ticket(12, 1, false),
                        ],
                    ),
                    (2, vec![ticket(20, 2, false)]),
                ]),
                agents: HashMap::from([
                    (10, vec![agent(100, 10), agent(101, 10)]),
                    (11, vec![agent(110, 11)]),
                    (20, vec![agent(200, 20)]),
                ]),
                live: Vec::new(),
            }
        }

        fn data(&self) -> TreeData<'_> {
            TreeData {
                module: ModuleKind::AutoScrum,
                projects: &self.projects,
                tickets: &self.tickets,
                agents: &self.agents,
                live: &self.live,
            }
        }

        fn live(mut self, id: i64, ticket_id: i64, project_id: i64, unseen: bool) -> Data {
            self.live.push(LiveAgent {
                id,
                ticket_id,
                project_id,
                unseen,
            });
            self
        }
    }

    /// (node, depth) of every visible row.
    fn rows(tree: &Tree, data: &Data) -> Vec<(NodeId, u16)> {
        tree.visible(&data.data())
            .iter()
            .map(|v| (v.id, v.depth))
            .collect()
    }

    fn find(tree: &Tree, data: &Data, id: NodeId) -> Visible {
        *tree
            .visible(&data.data())
            .iter()
            .find(|v| v.id == id)
            .unwrap_or_else(|| panic!("{id:?} not visible"))
    }

    #[test]
    fn the_module_is_the_root_even_alone_and_starts_closed() {
        let data = Data::new();
        let tree = Tree::default();
        assert_eq!(rows(&tree, &data), [(M, 0)]);
        let module = find(&tree, &data, M);
        assert!(module.expandable && !module.open);
        // Without any project the module is still there, as a leaf.
        let empty = Data {
            projects: Vec::new(),
            ..Data::new()
        };
        assert_eq!(rows(&tree, &empty), [(M, 0)]);
        assert!(!find(&tree, &empty, M).expandable);
    }

    #[test]
    fn opening_shows_the_children_with_their_depth() {
        let data = Data::new();
        let mut tree = Tree::default();
        tree.open(M);
        assert_eq!(
            rows(&tree, &data),
            [(M, 0), (NodeId::Project(1), 1), (NodeId::Project(2), 1)]
        );
        tree.open(NodeId::Project(1));
        tree.open(NodeId::Ticket(10));
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (NodeId::Project(1), 1),
                (NodeId::Ticket(10), 2),
                (NodeId::Agent(100), 3),
                (NodeId::Agent(101), 3),
                (NodeId::Ticket(12), 2),
                (NodeId::Project(2), 1),
            ]
        );
        let t10 = find(&tree, &data, NodeId::Ticket(10));
        assert!(t10.expandable && t10.open);
        // A ticket without agents is a leaf; an agent always is.
        assert!(!find(&tree, &data, NodeId::Ticket(12)).expandable);
        assert!(!find(&tree, &data, NodeId::Agent(100)).expandable);
        // Closing hides the branch and keeps what is open below it.
        tree.close(NodeId::Project(1));
        assert_eq!(rows(&tree, &data).len(), 3);
        tree.open(NodeId::Project(1));
        assert_eq!(rows(&tree, &data).len(), 7);
    }

    #[test]
    fn an_unavailable_project_is_not_expandable_and_does_not_open() {
        let data = Data::new();
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(NodeId::Project(2));
        let p2 = find(&tree, &data, NodeId::Project(2));
        assert!(!p2.expandable && !p2.open);
        assert!(!rows(&tree, &data).contains(&(NodeId::Ticket(20), 2)));
        // With a live agent of this nick it opens, so the agent stays reachable (decision 7).
        let data = Data::new().live(200, 20, 2, false);
        let p2 = find(&tree, &data, NodeId::Project(2));
        assert!(p2.expandable && p2.open);
        assert!(rows(&tree, &data).contains(&(NodeId::Ticket(20), 2)));
    }

    #[test]
    fn the_hidden_tickets_only_show_with_the_filter_but_one_with_a_live_agent_always() {
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(NodeId::Project(1));
        let data = Data::new();
        let tickets = |tree: &Tree, data: &Data| -> Vec<NodeId> {
            rows(tree, data)
                .into_iter()
                .filter(|(id, _)| matches!(id, NodeId::Ticket(_)))
                .map(|(id, _)| id)
                .collect()
        };
        assert_eq!(
            tickets(&tree, &data),
            [NodeId::Ticket(10), NodeId::Ticket(12)]
        );
        tree.show_hidden = true;
        assert_eq!(
            tickets(&tree, &data),
            [NodeId::Ticket(10), NodeId::Ticket(11), NodeId::Ticket(12)]
        );
        // Decision 7: a hidden ticket with a live agent of this nick stays.
        tree.show_hidden = false;
        let data = Data::new().live(110, 11, 1, false);
        assert_eq!(
            tickets(&tree, &data),
            [NodeId::Ticket(10), NodeId::Ticket(11), NodeId::Ticket(12)]
        );
    }

    #[test]
    fn the_filter_of_the_active_agents_opens_only_their_branches_and_gives_back_the_state() {
        let data = Data::new().live(101, 10, 1, false).live(200, 20, 2, false);
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(NodeId::Project(1));
        tree.open(NodeId::Ticket(10));
        let before = rows(&tree, &data);
        tree.only_active = true;
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (NodeId::Project(1), 1),
                (NodeId::Ticket(10), 2),
                (NodeId::Agent(101), 3),
                // The unavailable project shows its live agent under the filter.
                (NodeId::Project(2), 1),
                (NodeId::Ticket(20), 2),
                (NodeId::Agent(200), 3),
            ]
        );
        assert!(
            tree.visible(&data.data())
                .iter()
                .filter(|v| v.expandable)
                .all(|v| v.open),
            "every branch open"
        );
        tree.only_active = false;
        assert_eq!(rows(&tree, &data), before, "the state kept comes back");
        // Without a live agent the filter shows only the module.
        let none = Data::new();
        tree.only_active = true;
        assert_eq!(rows(&tree, &none), [(M, 0)]);
        assert!(!find(&tree, &none, M).expandable);
    }

    #[test]
    fn the_mark_goes_up_to_every_closed_ancestor_and_not_to_the_open_ones() {
        let data = Data::new().live(101, 10, 1, true).live(100, 10, 1, false);
        let mut tree = Tree::default();
        let unseen = |tree: &Tree, id| find(tree, &data, id).unseen_below;
        assert!(unseen(&tree, M), "the closed module");
        tree.open(M);
        assert!(!unseen(&tree, M), "open: the mark is below");
        assert!(unseen(&tree, NodeId::Project(1)));
        assert!(!unseen(&tree, NodeId::Project(2)));
        tree.open(NodeId::Project(1));
        assert!(!unseen(&tree, NodeId::Project(1)));
        assert!(unseen(&tree, NodeId::Ticket(10)));
        assert!(!unseen(&tree, NodeId::Ticket(12)));
        tree.open(NodeId::Ticket(10));
        assert!(!unseen(&tree, NodeId::Ticket(10)));
        // The agent shows its own mark (in its label), not `unseen_below`.
        assert!(!unseen(&tree, NodeId::Agent(101)));
        // A closed ancestor above an open one is marked too.
        tree.close(M);
        assert!(unseen(&tree, M));
    }

    #[test]
    fn parent_goes_up_one_level() {
        let data = Data::new().live(999, 12, 1, false);
        let tree = Tree::default();
        let parent = |id| tree.parent(id, &data.data());
        assert_eq!(parent(M), None);
        assert_eq!(parent(NodeId::Project(2)), Some(M));
        assert_eq!(parent(NodeId::Ticket(11)), Some(NodeId::Project(1)));
        assert_eq!(parent(NodeId::Agent(200)), Some(NodeId::Ticket(20)));
        // A live agent not read yet from the database: its ticket comes from the running ones.
        assert_eq!(parent(NodeId::Agent(999)), Some(NodeId::Ticket(12)));
        assert_eq!(parent(NodeId::Ticket(77)), None, "unknown");
    }

    #[test]
    fn reveal_opens_every_ancestor() {
        let data = Data::new();
        let mut tree = Tree::default();
        tree.reveal(NodeId::Agent(101), &data.data());
        assert!(tree.is_open(M));
        assert!(tree.is_open(NodeId::Project(1)));
        assert!(tree.is_open(NodeId::Ticket(10)));
        assert!(!tree.is_open(NodeId::Agent(101)));
        assert!(rows(&tree, &data).contains(&(NodeId::Agent(101), 3)));
    }

    fn v(id: NodeId, depth: u16) -> Visible {
        Visible {
            id,
            depth,
            expandable: false,
            open: false,
            unseen_below: false,
        }
    }

    #[test]
    fn reselect_keeps_a_node_still_there_else_next_previous_parent() {
        let (p1, t10, t11, t12) = (
            NodeId::Project(1),
            NodeId::Ticket(10),
            NodeId::Ticket(11),
            NodeId::Ticket(12),
        );
        let before = [v(M, 0), v(p1, 1), v(t10, 2), v(t11, 2), v(t12, 2)];
        // Still there (even somewhere else).
        let moved = [v(M, 0), v(p1, 1), v(t12, 2), v(t11, 2)];
        assert_eq!(Tree::reselect(&before, &moved, t11), t11);
        // Gone: the next sibling.
        let no_t11 = [v(M, 0), v(p1, 1), v(t10, 2), v(t12, 2)];
        assert_eq!(Tree::reselect(&before, &no_t11, t11), t12);
        // The last gone: the previous one.
        let no_t12 = [v(M, 0), v(p1, 1), v(t10, 2), v(t11, 2)];
        assert_eq!(Tree::reselect(&before, &no_t12, t12), t11);
        // No sibling left: the parent.
        let closed = [v(M, 0), v(p1, 1)];
        assert_eq!(Tree::reselect(&before, &closed, t11), p1);
        // The parent gone too: the closest ancestor still there.
        assert_eq!(Tree::reselect(&before, &[v(M, 0)], t11), M);
        // The next sibling is the next one still there, not a nephew.
        let deep = [
            v(M, 0),
            v(p1, 1),
            v(t10, 2),
            v(NodeId::Agent(1), 3),
            v(t12, 2),
        ];
        let after = [v(M, 0), v(p1, 1), v(NodeId::Agent(1), 3), v(t12, 2)];
        assert_eq!(Tree::reselect(&deep, &after, t10), t12);
        // Unknown before: the first row.
        assert_eq!(Tree::reselect(&[], &closed, t11), M);
    }

    /// NS-39: an archived agent only shows with the filter `f` (as a hidden ticket), and a ticket
    /// whose agents are all archived is a leaf without it; the filter `v` does not change.
    #[test]
    fn an_archived_agent_shows_only_with_the_filter_of_the_hidden_ones() {
        let mut data = Data::new();
        data.agents.get_mut(&10).unwrap()[0].state = "archived".into();
        data.agents.insert(12, vec![agent(120, 12)]);
        data.agents.get_mut(&12).unwrap()[0].state = "archived".into();
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(NodeId::Project(1));
        tree.open(NodeId::Ticket(10));
        tree.open(NodeId::Ticket(12));
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (NodeId::Project(1), 1),
                (NodeId::Ticket(10), 2),
                (NodeId::Agent(101), 3),
                (NodeId::Ticket(12), 2),
                (NodeId::Project(2), 1),
            ]
        );
        let leaf = find(&tree, &data, NodeId::Ticket(12));
        assert!(
            !leaf.expandable && !leaf.open,
            "only archived agents: a leaf"
        );
        tree.show_hidden = true;
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (NodeId::Project(1), 1),
                (NodeId::Ticket(10), 2),
                (NodeId::Agent(100), 3),
                (NodeId::Agent(101), 3),
                (NodeId::Ticket(11), 2),
                (NodeId::Ticket(12), 2),
                (NodeId::Agent(120), 3),
                (NodeId::Project(2), 1),
            ]
        );
        assert!(find(&tree, &data, NodeId::Ticket(12)).expandable);
        // The filter `v` shows only the live agents, as before: an archived one never is.
        tree.only_active = true;
        let data = data.live(101, 10, 1, false);
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (NodeId::Project(1), 1),
                (NodeId::Ticket(10), 2),
                (NodeId::Agent(101), 3),
            ]
        );
    }
}
