use std::collections::HashMap;
use std::io;

use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use ratatui::Terminal;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::config::lib::{Config, HttpTest};

#[derive(Debug, Clone)]
struct TestTree {
    nodes: HashMap<String, TestNode>,
    root_nodes: Vec<String>,
    list_state: ListState,
    visible_items: Vec<String>,  // Track visible nodes in display order
}

#[derive(Debug, Clone)]
struct TestNode {
    id: String,
    children: Vec<String>,
    parent: Option<String>,
    expanded: bool,
}

pub struct Dashboard {
    config: Config,
    tree: TestTree,
}

impl Dashboard {
    pub fn new(config: Config) -> Self {
        let mut tree = TestTree::new(&config.tests);
        tree.select_first();
        Self { config, tree }
    }

    pub fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;

        loop {
            terminal.draw(|frame| self.render(frame))?;

            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Up => self.tree.select_previous(),
                    KeyCode::Down => self.tree.select_next(),
                    KeyCode::Right => self.tree.expand_selected(),
                    KeyCode::Left => self.tree.collapse_selected(),
                    _ => {}
                }
            }
        }

        disable_raw_mode()?;
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        let size = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(95), Constraint::Percentage(5)])
            .split(size);

        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(chunks[0]);

        self.render_tree(frame, main_chunks[0]);
        self.render_details(frame, main_chunks[1]);
        self.render_help(frame, chunks[1]);
    }

    fn render_tree(&mut self, frame: &mut Frame, area: Rect) {
        let mut list_state = self.tree.list_state.clone();
        let items = {
            let tree = &mut self.tree;
            tree.build_items()
        };
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Tests"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        
        frame.render_stateful_widget(list, area, &mut list_state);
        self.tree.list_state = list_state;
    }

    fn render_details(&self, frame: &mut Frame, area: Rect) {
        let selected_test = self
            .tree
            .get_selected_node_id()
            .and_then(|id| self.config.tests.iter().find(|t| t.id == id));

        let content = if let Some(test) = selected_test {
            vec![
                Line::from(vec![
                    Span::styled("ID: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&test.id),
                ]),
                Line::from(vec![
                    Span::styled("Name: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&test.name),
                ]),
                Line::from(vec![
                    Span::styled("URL: ", Style::default().fg(Color::Yellow)),
                    Span::raw(test.url.as_deref().unwrap_or("N/A")),
                ]),
                Line::from(vec![
                    Span::styled("Method: ", Style::default().fg(Color::Yellow)),
                    Span::raw(test.method.as_deref().unwrap_or("GET")),
                ]),
            ]
        } else {
            vec![Line::from("No test selected")]
        };

        let details = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Test Details"))
            .wrap(ratatui::widgets::Wrap { trim: true });

        frame.render_widget(details, area);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let help_text = vec![
            Span::raw("↑/↓: Navigate  "),
            Span::raw("←/→: Collapse/Expand  "),
            Span::raw("q: Quit  "),
            Span::raw("Enter: Run Test"),
        ];

        let help = Paragraph::new(Line::from(help_text))
            .block(Block::default().borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(help, area);
    }
}

impl TestTree {
    fn new(tests: &[HttpTest]) -> Self {
        let (nodes, root_nodes) = Self::build_nodes(tests);
        Self {
            nodes,
            root_nodes,
            list_state: ListState::default(),
            visible_items: Vec::new(),
        }
    }

    fn build_nodes(tests: &[HttpTest]) -> (HashMap<String, TestNode>, Vec<String>) {
        let mut nodes = HashMap::new();
        let mut root_nodes = Vec::new();

        // First pass: Create nodes
        for test in tests {
            let node = TestNode {
                id: test.id.clone(),
                children: Vec::new(),
                parent: test.depends_on.clone(),
                expanded: true,
            };

            if node.parent.is_none() {
                root_nodes.push(node.id.clone());
            }
            nodes.insert(node.id.clone(), node);
        }

        // Second pass: Build relationships
        for test in tests {
            if let Some(parent) = &test.depends_on {
                if let Some(parent_node) = nodes.get_mut(parent) {
                    parent_node.children.push(test.id.clone());
                }
            }
        }

        (nodes, root_nodes)
    }

    fn build_items(&mut self) -> Vec<ListItem> {
        self.visible_items.clear();
        let mut items = Vec::new();
        
        let root_nodes: Vec<String> = self.root_nodes.clone();
        for root in root_nodes {
            self.build_node_items(&root, 0, &mut items);
        }
        
        items
    }

    fn build_node_items(
        &mut self,
        node_id: &str,
        depth: usize,
        items: &mut Vec<ListItem>,
    ) {
        let node = &self.nodes[node_id];
        let prefix = "  ".repeat(depth);
        
        // Determine symbol based on children and expanded state
        let symbol = if node.children.is_empty() {
            "• "
        } else if node.expanded {
            "▼ "
        } else {
            "▶ "
        };
        
        items.push(ListItem::new(format!("{}{}{}", prefix, symbol, node.id)));
        self.visible_items.push(node_id.to_string());
        
        if node.expanded {
            let children = node.children.clone();
            for child in children {
                self.build_node_items(&child, depth + 1, items);
            }
        }
    }

    fn select_previous(&mut self) {
        let _ = self.build_items();
        let current = self.list_state.selected().unwrap_or(0);
        if current > 0 {
            self.list_state.select(Some(current - 1));
        }
    }

    fn select_next(&mut self) {
        let current = self.list_state.selected();
        let items = self.build_items();
        if let Some(current) = current {
            if current < items.len() - 1 {
                self.list_state.select(Some(current + 1));
            }
        } else {
            self.list_state.select(Some(0));
        }
    }

    fn expand_selected(&mut self) {
        if let Some(selected) = self.get_selected_node_id() {
            if let Some(node) = self.nodes.get_mut(&selected) {
                if !node.children.is_empty() {
                    node.expanded = true;
                }
            }
        }
    }

    fn collapse_selected(&mut self) {
        if let Some(selected) = self.get_selected_node_id() {
            if let Some(node) = self.nodes.get_mut(&selected) {
                if !node.children.is_empty() {
                    node.expanded = false;
                    self.build_items();  // Refresh visible items
                }
            }
        }
    }

    fn get_selected_node_id(&self) -> Option<String> {
        self.list_state
            .selected()
            .and_then(|i| self.visible_items.get(i))
            .cloned()
    }

    fn select_first(&mut self) {
        if !self.root_nodes.is_empty() {
            self.list_state.select(Some(0));
        }
    }
}
