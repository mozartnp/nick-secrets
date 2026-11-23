//! The tree of the panel (NS-38): Module > Project > [Epic >] Ticket > Agent (the optional epic
//! since the NS-40), as a pure model, with no database, no screen and no disk. The App fills a
//! `TreeData` from the database and its running agents; here are the nodes by identity (`NodeId`,
//! the selection survives a refresh), the open/closed state of the branches, the flattening into
//! the visible rows with their depth, the three filters (the hidden tickets and epics,
//! `show_hidden`; the done tickets and epics, `hide_done`, NS-54; only the live agents,
//! `only_active`), the "saída nova" mark that goes up to the closed ancestors, and where the
//! selection goes when its node is gone (`reselect`). ui.rs draws the rows; app.rs moves on them.

use std::collections::{HashMap, HashSet};

use crate::agents::{Agent, AgentState, Owner};
use crate::app::Row;
use crate::db::Epic;
use crate::modules::ModuleKind;
use crate::projects::Availability;
use crate::tickets::{Ticket, TicketStage};

/// A node of the tree, by identity: the selection survives a refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeId {
    Module(ModuleKind),
    Project(i64),
    Epic(i64),
    Ticket(i64),
    Agent(i64),
}

impl From<Owner> for NodeId {
    /// The node of the owner of an agent: its parent on the tree.
    fn from(owner: Owner) -> NodeId {
        match owner {
            Owner::Ticket(id) => NodeId::Ticket(id),
            Owner::Epic(id) => NodeId::Epic(id),
        }
    }
}

/// A live agent of this nick: where it is in the tree and whether it is marked "● saída nova".
/// `epic_id` is the epic of its branch (NS-40): its owner, when an epic, or the epic of its
/// ticket; the mark and the filter `v` go up through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveAgent {
    pub id: i64,
    pub owner: Owner,
    pub project_id: i64,
    pub epic_id: Option<i64>,
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
    /// The epics of each project (by project id), the hidden ones included (NS-40).
    pub epics: &'a HashMap<i64, Vec<Epic>>,
    /// The agents of each ticket and epic (by owner), of every stage and state, in the order they
    /// were opened.
    pub agents: &'a HashMap<Owner, Vec<Agent>>,
    /// The agents this nick runs.
    pub live: &'a [LiveAgent],
}

/// The ids of the done epics and tickets of a `TreeData`, read once per `Tree::visible` with the
/// filter of the done ones on (review of the NS-54, P3: not a scan of every epic and ticket for
/// each live agent).
struct DoneIds {
    epics: HashSet<i64>,
    tickets: HashSet<i64>,
}

impl DoneIds {
    fn of(data: &TreeData) -> DoneIds {
        DoneIds {
            epics: data
                .epics
                .values()
                .flatten()
                .filter(|epic| epic.done_at.is_some())
                .map(|epic| epic.id)
                .collect(),
            tickets: data
                .tickets
                .values()
                .flatten()
                .filter(|ticket| ticket.stage == TicketStage::Done.key())
                .map(|ticket| ticket.id)
                .collect(),
        }
    }

    /// Whether the filter takes the branch of the live `agent`: its ticket or the epic of its
    /// branch is done.
    fn takes(&self, agent: &LiveAgent) -> bool {
        let epic_done = agent.epic_id.is_some_and(|id| self.epics.contains(&id));
        let ticket_done = match agent.owner {
            Owner::Ticket(id) => self.tickets.contains(&id),
            Owner::Epic(_) => false,
        };
        epic_done || ticket_done
    }
}

/// The open branches and the filters. Every branch starts closed.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    open: HashSet<NodeId>,
    /// The hidden tickets and epics are listed too, and the archived agents (`f`; NS-39).
    pub show_hidden: bool,
    /// Only the branches with a live agent of this nick, all open (`v`).
    pub only_active: bool,
    /// The done tickets and the done epics, with their whole branch, are out (`d`; NS-54). Off
    /// by default: everything shows.
    pub hide_done: bool,
}

/// A visible row: the node, its depth (0..=4), whether it can open, whether it is open, and
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
    /// unavailable one does not open, but for one with a live agent); in an open project, first
    /// its epics (NS-40, decision 3), then its tickets without epic; in an open epic, first its
    /// agents, then its tickets. A hidden ticket or epic shows only with `show_hidden` or with a
    /// live agent of this nick in it (decision 7 of NS-38, decision 9 of NS-40). The agents of an
    /// open ticket or epic, in the order they were opened, without the archived ones unless
    /// `show_hidden` (NS-39; a node with only archived agents is then a leaf). With `only_active`,
    /// only the branches with a live agent and only the live agents, every branch open, whatever
    /// the state kept. With `hide_done` (NS-54, D5), a done ticket (at any depth) and a done epic
    /// with its whole branch are out, and, unlike `show_hidden`, a live agent does not hold them:
    /// it stays reachable by `only_active`, where `hide_done` does not apply. A live agent the
    /// filter took is not counted anywhere above it (its mark, a hidden epic it would hold, an
    /// unavailable project it would open). `show_hidden` and `hide_done` are independent: a
    /// hidden and done ticket shows only with the first on and the second off.
    pub fn visible(&self, data: &TreeData) -> Vec<Visible> {
        // Only with the filter on: the data as it is otherwise, with no copy (review of the
        // NS-54, P3).
        let live: Vec<LiveAgent>;
        let narrowed: TreeData;
        let data = if self.hides_done() {
            let done = DoneIds::of(data);
            live = data
                .live
                .iter()
                .filter(|agent| !done.takes(agent))
                .copied()
                .collect();
            narrowed = TreeData {
                live: &live,
                ..*data
            };
            &narrowed
        } else {
            data
        };
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
                let epics = data.epics.get(&project).map_or(&[][..], Vec::as_slice);
                let tickets = data.tickets.get(&project).map_or(&[][..], Vec::as_slice);
                for epic in epics {
                    self.push_epic(&mut rows, data, epic, tickets);
                }
                // The tickets without epic (or of an epic this data does not know).
                for ticket in tickets {
                    let in_epic = ticket
                        .epic_id
                        .is_some_and(|e| epics.iter().any(|epic| epic.id == e));
                    if !in_epic {
                        self.push_ticket(&mut rows, data, ticket, 2);
                    }
                }
            }
        }
        rows
    }

    /// Whether the filter of the done ones is on and applies (not with `only_active`, NS-54). The
    /// App asks it too, to say whether a completed epic leaves the tree (review of the NS-54, P1).
    pub(crate) fn hides_done(&self) -> bool {
        self.hide_done && !self.only_active
    }

    /// The agents of `owner` that show (the filters `v` and `f`).
    fn agents_of<'a>(&self, data: &'a TreeData, owner: Owner) -> Vec<&'a Agent> {
        data.agents
            .get(&owner)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|agent| !self.only_active || data.live.iter().any(|a| a.id == agent.id))
            // An archived agent only with the hidden ones (`f`, NS-39).
            .filter(|agent| self.show_hidden || agent.state != AgentState::Archived.key())
            .collect()
    }

    /// Whether a ticket or an epic with `hidden_at` shows, `live` telling whether a live agent of
    /// this nick is below it.
    fn shows(&self, hidden: bool, live: bool) -> bool {
        if self.only_active {
            live
        } else {
            !hidden || self.show_hidden || live
        }
    }

    fn push_agents(rows: &mut Vec<Visible>, agents: &[&Agent], depth: u16) {
        for agent in agents {
            rows.push(Visible {
                id: NodeId::Agent(agent.id),
                depth,
                expandable: false,
                open: false,
                unseen_below: false,
            });
        }
    }

    /// A ticket at `depth` (2 under the project, 3 under an epic) and, open, its agents.
    fn push_ticket(&self, rows: &mut Vec<Visible>, data: &TreeData, ticket: &Ticket, depth: u16) {
        if self.hides_done() && ticket.stage == TicketStage::Done.key() {
            return;
        }
        let owner = Owner::Ticket(ticket.id);
        let has_live = data.live.iter().any(|a| a.owner == owner);
        if !self.shows(ticket.hidden_at.is_some(), has_live) {
            return;
        }
        let agents = self.agents_of(data, owner);
        let expandable = !agents.is_empty();
        let open = expandable && (self.only_active || self.is_open(NodeId::Ticket(ticket.id)));
        rows.push(Visible {
            id: NodeId::Ticket(ticket.id),
            depth,
            expandable,
            open,
            unseen_below: !open && data.live.iter().any(|a| a.unseen && a.owner == owner),
        });
        if open {
            Tree::push_agents(rows, &agents, depth + 1);
        }
    }

    /// An epic (depth 2) and, open, its agents (depth 3) and then its tickets (depth 3).
    fn push_epic(&self, rows: &mut Vec<Visible>, data: &TreeData, epic: &Epic, tickets: &[Ticket]) {
        if self.hides_done() && epic.done_at.is_some() {
            return;
        }
        let in_branch = |a: &LiveAgent| a.epic_id == Some(epic.id);
        let has_live = data.live.iter().any(in_branch);
        if !self.shows(epic.hidden_at.is_some(), has_live) {
            return;
        }
        let agents = self.agents_of(data, Owner::Epic(epic.id));
        let mut children = Vec::new();
        Tree::push_agents(&mut children, &agents, 3);
        for ticket in tickets.iter().filter(|t| t.epic_id == Some(epic.id)) {
            self.push_ticket(&mut children, data, ticket, 3);
        }
        let expandable = !children.is_empty();
        let open = expandable && (self.only_active || self.is_open(NodeId::Epic(epic.id)));
        rows.push(Visible {
            id: NodeId::Epic(epic.id),
            depth: 2,
            expandable,
            open,
            unseen_below: !open && data.live.iter().any(|a| a.unseen && in_branch(a)),
        });
        if open {
            rows.extend(children);
        }
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

    /// The parent of `id`, as far as `data` knows it: a ticket in an epic is under the epic, an
    /// epic and a ticket without epic under the project, and an agent under its owner (NS-40).
    /// A live agent's owner comes from the running ones (it may not be read from the database
    /// yet).
    pub fn parent(&self, id: NodeId, data: &TreeData) -> Option<NodeId> {
        match id {
            NodeId::Module(_) => None,
            NodeId::Project(project) => data
                .projects
                .iter()
                .any(|row| row.project.id == project)
                .then_some(NodeId::Module(data.module)),
            NodeId::Epic(epic) => data
                .epics
                .values()
                .flatten()
                .find(|e| e.id == epic)
                .map(|e| NodeId::Project(e.project_id)),
            NodeId::Ticket(ticket) => {
                let ticket = data.tickets.values().flatten().find(|t| t.id == ticket)?;
                let known = |epic: i64| data.epics.values().flatten().any(|e| e.id == epic);
                Some(match ticket.epic_id.filter(|epic| known(*epic)) {
                    Some(epic) => NodeId::Epic(epic),
                    None => NodeId::Project(ticket.project_id),
                })
            }
            NodeId::Agent(agent) => data
                .live
                .iter()
                .find(|a| a.id == agent)
                .map(|a| a.owner)
                .or_else(|| {
                    data.agents
                        .values()
                        .flatten()
                        .find(|a| a.id == agent)
                        .map(|a| a.owner)
                })
                .map(NodeId::from),
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
            jira_url: None,
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
            epic_id: None,
        }
    }

    fn agent(id: i64, ticket_id: i64) -> Agent {
        Agent {
            id,
            owner: Owner::Ticket(ticket_id),
            stage: "po".into(),
            session_id: format!("s{id}"),
            name: format!("a{id}"),
            opened_at: String::new(),
            state: "stopped".into(),
            opened_key: None,
            resume_session_id: None,
        }
    }

    /// Projects 1 (available) and 2 (unavailable); tickets 10, 11 (hidden) and 12 of project 1,
    /// 20 of project 2; agents 100 and 101 of ticket 10, 110 of ticket 11, 200 of ticket 20.
    struct Data {
        projects: Vec<Row>,
        tickets: HashMap<i64, Vec<Ticket>>,
        epics: HashMap<i64, Vec<Epic>>,
        agents: HashMap<Owner, Vec<Agent>>,
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
                epics: HashMap::new(),
                agents: HashMap::from([
                    (Owner::Ticket(10), vec![agent(100, 10), agent(101, 10)]),
                    (Owner::Ticket(11), vec![agent(110, 11)]),
                    (Owner::Ticket(20), vec![agent(200, 20)]),
                ]),
                live: Vec::new(),
            }
        }

        fn data(&self) -> TreeData<'_> {
            TreeData {
                module: ModuleKind::AutoScrum,
                projects: &self.projects,
                tickets: &self.tickets,
                epics: &self.epics,
                agents: &self.agents,
                live: &self.live,
            }
        }

        fn live(mut self, id: i64, ticket_id: i64, project_id: i64, unseen: bool) -> Data {
            self.live.push(LiveAgent {
                id,
                owner: Owner::Ticket(ticket_id),
                project_id,
                epic_id: None,
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
        data.agents.get_mut(&Owner::Ticket(10)).unwrap()[0].state = "archived".into();
        data.agents.insert(Owner::Ticket(12), vec![agent(120, 12)]);
        data.agents.get_mut(&Owner::Ticket(12)).unwrap()[0].state = "archived".into();
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

    // ---- the epics (NS-40) ----

    fn epic(id: i64, project_id: i64, hidden: bool) -> Epic {
        Epic {
            id,
            project_id,
            key: format!("E-{id}"),
            is_provisional: false,
            title: String::new(),
            created_at: String::new(),
            hidden_at: hidden.then(|| "2026-11-20T00:00:00Z".to_string()),
            done_at: None,
        }
    }

    fn epic_agent(id: i64, epic_id: i64) -> Agent {
        Agent {
            owner: Owner::Epic(epic_id),
            ..agent(id, 0)
        }
    }

    const E5: NodeId = NodeId::Epic(5);
    const E6: NodeId = NodeId::Epic(6);
    const P1: NodeId = NodeId::Project(1);

    /// `Data::new` plus, in project 1, the epics 5 (with the agents 500 and 501, and the tickets
    /// 13 and 14, which has the agent 140) and 6 (hidden, with the ticket 15), created after the
    /// tickets 10, 11 and 12 (which have no epic).
    fn with_epics() -> Data {
        let mut data = Data::new();
        let tickets = data.tickets.get_mut(&1).unwrap();
        for (id, epic) in [(13, 5), (14, 5), (15, 6)] {
            tickets.push(Ticket {
                epic_id: Some(epic),
                ..ticket(id, 1, false)
            });
        }
        data.epics = HashMap::from([(1, vec![epic(5, 1, false), epic(6, 1, true)])]);
        data.agents
            .insert(Owner::Epic(5), vec![epic_agent(500, 5), epic_agent(501, 5)]);
        data.agents.insert(Owner::Ticket(14), vec![agent(140, 14)]);
        data
    }

    fn live_of_epic(mut data: Data, id: i64, owner: Owner, epic: i64, unseen: bool) -> Data {
        data.live.push(LiveAgent {
            id,
            owner,
            project_id: 1,
            epic_id: Some(epic),
            unseen,
        });
        data
    }

    #[test]
    fn the_epics_come_before_the_tickets_and_the_agents_of_an_epic_before_its_tickets() {
        let data = with_epics();
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(P1);
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (P1, 1),
                (E5, 2),
                (NodeId::Ticket(10), 2),
                (NodeId::Ticket(12), 2),
                (NodeId::Project(2), 1)
            ],
            "the epics first (by creation), then the tickets without epic; the hidden epic 6 out"
        );
        assert!(find(&tree, &data, E5).expandable);
        tree.open(E5);
        tree.open(NodeId::Ticket(14));
        assert_eq!(
            rows(&tree, &data)[2..8],
            [
                (E5, 2),
                (NodeId::Agent(500), 3),
                (NodeId::Agent(501), 3),
                (NodeId::Ticket(13), 3),
                (NodeId::Ticket(14), 3),
                (NodeId::Agent(140), 4),
            ],
            "the depths 2, 3 and 4"
        );
    }

    #[test]
    fn a_project_without_epics_gives_the_rows_of_before() {
        // The same rows as `opening_shows_the_children_with_their_depth`, with the map of the
        // epics present and empty for the project.
        let mut data = Data::new();
        data.epics.insert(1, Vec::new());
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(P1);
        tree.open(NodeId::Ticket(10));
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (P1, 1),
                (NodeId::Ticket(10), 2),
                (NodeId::Agent(100), 3),
                (NodeId::Agent(101), 3),
                (NodeId::Ticket(12), 2),
                (NodeId::Project(2), 1)
            ]
        );
    }

    #[test]
    fn a_hidden_epic_takes_its_branch_and_shows_with_f_or_with_a_live_agent() {
        let data = with_epics();
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(P1);
        tree.open(E6);
        let ids = |tree: &Tree, data: &Data| -> Vec<NodeId> {
            rows(tree, data).into_iter().map(|(id, _)| id).collect()
        };
        assert!(!ids(&tree, &data).contains(&E6));
        assert!(
            !ids(&tree, &data).contains(&NodeId::Ticket(15)),
            "its ticket goes with it"
        );
        tree.show_hidden = true;
        let shown = ids(&tree, &data);
        assert!(shown.contains(&E6) && shown.contains(&NodeId::Ticket(15)));
        tree.show_hidden = false;
        // A live agent of this nick in a ticket of the hidden epic: the epic stays (decision 9).
        let live = live_of_epic(with_epics(), 150, Owner::Ticket(15), 6, false);
        let shown = ids(&tree, &live);
        assert!(
            shown.contains(&E6) && shown.contains(&NodeId::Ticket(15)),
            "{shown:?}"
        );
    }

    #[test]
    fn only_active_shows_the_epics_with_a_live_agent_in_their_branch_open() {
        let tree = Tree {
            only_active: true,
            ..Tree::default()
        };
        // A live agent of a ticket of the epic.
        let mut data = live_of_epic(with_epics(), 140, Owner::Ticket(14), 5, false);
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (P1, 1),
                (E5, 2),
                (NodeId::Ticket(14), 3),
                (NodeId::Agent(140), 4)
            ]
        );
        // A live agent of the epic itself.
        data.live.clear();
        let data = live_of_epic(data, 501, Owner::Epic(5), 5, false);
        assert_eq!(
            rows(&tree, &data),
            [(M, 0), (P1, 1), (E5, 2), (NodeId::Agent(501), 3)]
        );
    }

    #[test]
    fn the_mark_goes_up_through_a_closed_epic() {
        let data = live_of_epic(with_epics(), 140, Owner::Ticket(14), 5, true);
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(P1);
        assert!(find(&tree, &data, E5).unseen_below, "the closed epic");
        assert!(
            !find(&tree, &data, P1).unseen_below,
            "the open project does not repeat it"
        );
        tree.open(E5);
        assert!(!find(&tree, &data, E5).unseen_below);
        assert!(
            find(&tree, &data, NodeId::Ticket(14)).unseen_below,
            "the closed ticket"
        );
        let data = live_of_epic(with_epics(), 500, Owner::Epic(5), 5, true);
        tree.close(E5);
        assert!(
            find(&tree, &data, E5).unseen_below,
            "an agent of the epic itself"
        );
    }

    #[test]
    fn parent_and_reveal_go_through_the_epic() {
        let data = with_epics();
        let tree = Tree::default();
        let d = data.data();
        assert_eq!(
            tree.parent(NodeId::Agent(140), &d),
            Some(NodeId::Ticket(14))
        );
        assert_eq!(tree.parent(NodeId::Ticket(14), &d), Some(E5));
        assert_eq!(tree.parent(NodeId::Agent(500), &d), Some(E5));
        assert_eq!(tree.parent(E5, &d), Some(P1));
        assert_eq!(tree.parent(NodeId::Ticket(10), &d), Some(P1), "no epic");
        let mut tree = Tree::default();
        tree.reveal(NodeId::Agent(140), &d);
        for id in [M, P1, E5, NodeId::Ticket(14)] {
            assert!(tree.is_open(id), "{id:?}");
        }
        assert!(rows(&tree, &data).contains(&(NodeId::Agent(140), 4)));
    }

    #[test]
    fn reselect_leaves_a_node_of_depth_4() {
        let data = with_epics();
        let mut tree = Tree::default();
        tree.reveal(NodeId::Agent(140), &data.data());
        let before = tree.visible(&data.data());
        // The agent 140 is archived meanwhile: its ticket is the closest node left.
        let mut after_data = with_epics();
        after_data.agents.get_mut(&Owner::Ticket(14)).unwrap()[0].state = "archived".into();
        let after = tree.visible(&after_data.data());
        assert_eq!(
            Tree::reselect(&before, &after, NodeId::Agent(140)),
            NodeId::Ticket(14)
        );
        // The ticket 14 moves out of the epic: its agent is still there, now at depth 3.
        let mut moved = with_epics();
        moved.tickets.get_mut(&1).unwrap()[4].epic_id = None;
        let after = tree.visible(&moved.data());
        assert!(after.contains(&Visible {
            id: NodeId::Agent(140),
            depth: 3,
            expandable: false,
            open: false,
            unseen_below: false
        }));
        assert_eq!(
            Tree::reselect(&before, &after, NodeId::Agent(140)),
            NodeId::Agent(140)
        );
        // The epic hidden meanwhile (by another nick): its branch goes, and the selection goes
        // to the closest ancestor, the project.
        let mut hidden = with_epics();
        hidden.epics.get_mut(&1).unwrap()[0].hidden_at = Some("x".into());
        let after = tree.visible(&hidden.data());
        assert_eq!(Tree::reselect(&before, &after, NodeId::Agent(140)), P1);
    }

    // ---- the filter of the done ones (NS-54, D5) ----

    const T: fn(i64) -> NodeId = NodeId::Ticket;

    /// `with_epics` with the ticket 12 (no epic) and 13 (of the epic 5) done.
    fn with_done_tickets() -> Data {
        let mut data = with_epics();
        for t in data.tickets.get_mut(&1).unwrap() {
            if [12, 13].contains(&t.id) {
                t.stage = "done".into();
            }
        }
        data
    }

    fn done_epic(mut data: Data, id: i64) -> Data {
        for e in data.epics.get_mut(&1).unwrap() {
            if e.id == id {
                e.done_at = Some("2026-11-22T00:00:00Z".into());
            }
        }
        data
    }

    fn ids(tree: &Tree, data: &Data) -> Vec<NodeId> {
        rows(tree, data).into_iter().map(|(id, _)| id).collect()
    }

    fn opened() -> Tree {
        let mut tree = Tree::default();
        for id in [M, P1, E5, T(14)] {
            tree.open(id);
        }
        tree
    }

    #[test]
    fn by_default_the_done_ones_show_with_the_rows_of_before() {
        let tree = opened();
        assert!(!tree.hide_done, "the default shows everything");
        let done = done_epic(with_done_tickets(), 5);
        assert_eq!(
            rows(&tree, &done),
            rows(&tree, &with_epics()),
            "a done stage or epic changes no row"
        );
    }

    #[test]
    fn hide_done_takes_the_done_tickets_at_depth_2_and_3() {
        let mut tree = opened();
        tree.hide_done = true;
        let data = with_done_tickets();
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (P1, 1),
                (E5, 2),
                (NodeId::Agent(500), 3),
                (NodeId::Agent(501), 3),
                (T(14), 3),
                (NodeId::Agent(140), 4),
                (T(10), 2),
                (NodeId::Project(2), 1),
            ],
            "13 (in the epic) and 12 (no epic) out; the open epic stays"
        );
    }

    #[test]
    fn hide_done_takes_a_done_epic_with_its_whole_branch() {
        let mut tree = opened();
        tree.hide_done = true;
        let data = done_epic(with_epics(), 5);
        let shown = ids(&tree, &data);
        for gone in [E5, NodeId::Agent(500), NodeId::Agent(501), T(13), T(14)] {
            assert!(!shown.contains(&gone), "{gone:?}: {shown:?}");
        }
        assert!(shown.contains(&T(10)) && shown.contains(&T(12)));
    }

    #[test]
    fn a_live_agent_does_not_hold_a_done_ticket_nor_a_done_epic() {
        let mut tree = opened();
        tree.hide_done = true;
        let data = live_of_epic(with_done_tickets(), 130, Owner::Ticket(13), 5, false);
        assert!(!ids(&tree, &data).contains(&T(13)), "unlike the f");
        let data = live_of_epic(done_epic(with_epics(), 5), 500, Owner::Epic(5), 5, false);
        assert!(!ids(&tree, &data).contains(&E5));
        // Not even a hidden and done epic with a live agent (the f would keep it).
        let mut data = live_of_epic(done_epic(with_epics(), 6), 150, Owner::Ticket(15), 6, false);
        assert!(!ids(&tree, &data).contains(&E6));
        tree.hide_done = false;
        assert!(
            ids(&tree, &data).contains(&E6),
            "the decision 9 of the NS-40"
        );
        data.live.clear();
        assert!(!ids(&tree, &data).contains(&E6));
    }

    #[test]
    fn with_only_active_the_filter_of_the_done_ones_does_not_apply() {
        let tree = Tree {
            only_active: true,
            hide_done: true,
            ..Tree::default()
        };
        // The live agent of a done ticket of a done epic stays reachable by the v.
        let mut data = done_epic(with_done_tickets(), 5);
        data.agents.insert(Owner::Ticket(13), vec![agent(130, 13)]);
        let data = live_of_epic(data, 130, Owner::Ticket(13), 5, false);
        assert_eq!(
            rows(&tree, &data),
            [
                (M, 0),
                (P1, 1),
                (E5, 2),
                (T(13), 3),
                (NodeId::Agent(130), 4)
            ]
        );
    }

    #[test]
    fn the_filters_of_the_hidden_and_of_the_done_ones_are_independent() {
        let mut data = Data::new();
        // The ticket 11 is hidden; done too now.
        data.tickets.get_mut(&1).unwrap()[1].stage = "done".into();
        let mut tree = Tree::default();
        tree.open(M);
        tree.open(P1);
        let shows_11 = |tree: &Tree| ids(tree, &data).contains(&T(11));
        for (show_hidden, hide_done, shown) in [
            (false, false, false),
            (true, false, true),
            (false, true, false),
            (true, true, false),
        ] {
            tree.show_hidden = show_hidden;
            tree.hide_done = hide_done;
            assert_eq!(shows_11(&tree), shown, "f {show_hidden}, d {hide_done}");
        }
    }

    #[test]
    fn the_mark_does_not_go_up_from_a_branch_the_filter_took() {
        let mut data = with_done_tickets();
        data.agents.insert(Owner::Ticket(13), vec![agent(130, 13)]);
        let data = live_of_epic(data, 130, Owner::Ticket(13), 5, true);
        let mut tree = Tree {
            hide_done: true,
            ..Tree::default()
        };
        assert!(!find(&tree, &data, M).unseen_below, "the closed module");
        tree.open(M);
        assert!(!find(&tree, &data, P1).unseen_below, "the closed project");
        tree.open(P1);
        assert!(!find(&tree, &data, E5).unseen_below, "the closed epic");
        tree.hide_done = false;
        assert!(find(&tree, &data, E5).unseen_below, "without the filter");
    }

    #[test]
    fn reselect_leaves_a_node_the_filter_of_the_done_ones_took() {
        let data = done_epic(with_done_tickets(), 5);
        let mut tree = opened();
        let before = tree.visible(&data.data());
        tree.hide_done = true;
        let after = tree.visible(&data.data());
        // The ticket 12 (done): the previous sibling, the ticket 10 (no next one).
        assert_eq!(Tree::reselect(&before, &after, T(12)), T(10));
        // The agent of the done epic: the closest ancestor left, the project.
        assert_eq!(Tree::reselect(&before, &after, NodeId::Agent(140)), P1);
        // The done epic: the next sibling, the ticket 10.
        assert_eq!(Tree::reselect(&before, &after, E5), T(10));
    }
}
