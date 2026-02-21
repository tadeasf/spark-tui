use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, TableState, Tabs},
};
use tokio::sync::mpsc;

use super::tabs::{jobs, suspects};
use super::widgets::status_line;
use super::{Action, DataPayload};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Jobs,
    Suspects,
}

impl Tab {
    pub fn titles() -> Vec<&'static str> {
        vec!["Jobs", "Suspects"]
    }

    pub fn index(self) -> usize {
        match self {
            Tab::Jobs => 0,
            Tab::Suspects => 1,
        }
    }

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Tab::Jobs,
            1 => Tab::Suspects,
            _ => Tab::Jobs,
        }
    }

    pub fn next(self) -> Self {
        Tab::from_index((self.index() + 1) % 2)
    }

    pub fn prev(self) -> Self {
        Tab::from_index((self.index() + 1) % 2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    JobDetail,
    SqlDetail,
}

pub struct App {
    pub active_tab: Tab,
    pub view_mode: ViewMode,
    pub data: Option<DataPayload>,
    pub error_msg: Option<String>,
    pub cluster_id: String,
    pub should_quit: bool,
    pub job_table_state: TableState,
    pub suspect_table_state: TableState,
    pub detail_table_state: TableState,
    pub sql_scroll: u16,
}

impl App {
    pub fn new(cluster_id: String) -> Self {
        Self {
            active_tab: Tab::Jobs,
            view_mode: ViewMode::List,
            data: None,
            error_msg: None,
            cluster_id,
            should_quit: false,
            job_table_state: TableState::default(),
            suspect_table_state: TableState::default(),
            detail_table_state: TableState::default(),
            sql_scroll: 0,
        }
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Key(key) => self.handle_key(key),
            Action::DataUpdate(payload) => {
                self.error_msg = None;

                // Preserve selection across refresh
                #[allow(clippy::collapsible_if)]
                if self.view_mode == ViewMode::JobDetail || self.view_mode == ViewMode::SqlDetail {
                    if let Some(old_data) = &self.data {
                        if let Some(sel_idx) = self.job_table_state.selected() {
                            if let Some(old_job) = old_data.jobs.get(sel_idx) {
                                let old_job_id = old_job.job_id;
                                // Find the same job in the new data
                                if let Some(new_idx) =
                                    payload.jobs.iter().position(|j| j.job_id == old_job_id)
                                {
                                    self.job_table_state.select(Some(new_idx));
                                } else {
                                    // Job disappeared, revert to list
                                    self.view_mode = ViewMode::List;
                                }
                            }
                        }
                    }
                }

                self.data = Some(payload);
            }
            Action::FetchError(err) => {
                self.error_msg = Some(err.to_string());
            }
            Action::Resize(_, _) => {}
            Action::Mouse(_) => {}
        }
    }

    fn active_table_state(&mut self) -> Option<&mut TableState> {
        match self.view_mode {
            ViewMode::JobDetail => Some(&mut self.detail_table_state),
            ViewMode::List => match self.active_tab {
                Tab::Jobs => Some(&mut self.job_table_state),
                Tab::Suspects => Some(&mut self.suspect_table_state),
            },
            ViewMode::SqlDetail => None, // Scroll-based, no table
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => match self.view_mode {
                ViewMode::List => self.should_quit = true,
                ViewMode::JobDetail => self.view_mode = ViewMode::List,
                ViewMode::SqlDetail => self.view_mode = ViewMode::JobDetail,
            },
            KeyCode::Tab if self.view_mode == ViewMode::List => {
                self.active_tab = self.active_tab.next();
            }
            KeyCode::BackTab if self.view_mode == ViewMode::List => {
                self.active_tab = self.active_tab.prev();
            }
            KeyCode::Char('s') if self.view_mode == ViewMode::JobDetail => {
                // Open SQL detail if the selected job has sql_id
                #[allow(clippy::collapsible_if)]
                if let Some(data) = &self.data {
                    if let Some(idx) = self.job_table_state.selected() {
                        if let Some(job) = data.jobs.get(idx) {
                            if job.sql_id.is_some() {
                                self.sql_scroll = 0;
                                self.view_mode = ViewMode::SqlDetail;
                            }
                        }
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') if self.view_mode == ViewMode::SqlDetail => {
                self.sql_scroll = self.sql_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.view_mode == ViewMode::SqlDetail => {
                self.sql_scroll = self.sql_scroll.saturating_sub(1);
            }
            KeyCode::Home | KeyCode::Char('g') if self.view_mode == ViewMode::SqlDetail => {
                self.sql_scroll = 0;
            }
            KeyCode::End | KeyCode::Char('G') if self.view_mode == ViewMode::SqlDetail => {
                self.sql_scroll = u16::MAX; // Will be clamped by Paragraph
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(ts) = self.active_table_state() {
                    ts.select_next();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(ts) = self.active_table_state() {
                    ts.select_previous();
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                if let Some(ts) = self.active_table_state() {
                    ts.select_first();
                }
            }
            KeyCode::End | KeyCode::Char('G') => {
                if let Some(ts) = self.active_table_state() {
                    ts.select_last();
                }
            }
            KeyCode::Enter => self.handle_enter(),
            _ => {}
        }
    }

    #[allow(clippy::collapsible_if)]
    fn handle_enter(&mut self) {
        match self.view_mode {
            ViewMode::List => match self.active_tab {
                Tab::Jobs => {
                    if self.job_table_state.selected().is_some() {
                        self.detail_table_state = TableState::default();
                        self.detail_table_state.select_first();
                        self.view_mode = ViewMode::JobDetail;
                    }
                }
                Tab::Suspects => {
                    if let Some(idx) = self.suspect_table_state.selected() {
                        if let Some(data) = &self.data {
                            if let Some(suspect) = data.suspects.get(idx) {
                                if let Some(job_id) = suspect.job_id {
                                    if let Some(job_idx) =
                                        data.jobs.iter().position(|j| j.job_id == job_id)
                                    {
                                        self.active_tab = Tab::Jobs;
                                        self.job_table_state.select(Some(job_idx));
                                        self.detail_table_state = TableState::default();
                                        self.detail_table_state.select_first();
                                        self.view_mode = ViewMode::JobDetail;
                                    }
                                }
                            }
                        }
                    }
                }
            },
            ViewMode::JobDetail | ViewMode::SqlDetail => {
                // No deeper drill-down
            }
        }
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
        mut rx: mpsc::UnboundedReceiver<Action>,
    ) -> std::io::Result<()> {
        while !self.should_quit {
            terminal.draw(|f| self.render(f))?;

            if let Some(action) = rx.recv().await {
                self.handle_action(action);
            } else {
                break;
            }
        }
        Ok(())
    }

    fn render(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // tab bar
                Constraint::Fill(1),   // content area
                Constraint::Length(1), // status bar
            ])
            .split(f.area());

        self.render_tab_bar(f, chunks[0]);
        self.render_content(f, chunks[1]);
        self.render_status_bar(f, chunks[2]);
    }

    fn render_tab_bar(&self, f: &mut Frame, area: Rect) {
        let titles: Vec<Line> = Tab::titles()
            .into_iter()
            .map(|t| Line::from(Span::raw(t)))
            .collect();

        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::ALL))
            .select(self.active_tab.index())
            .highlight_style(
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_widget(tabs, area);
    }

    fn render_content(&mut self, f: &mut Frame, area: Rect) {
        // Clone data to avoid borrow conflicts with &mut self
        let data = self.data.clone();

        match &data {
            Some(data) => match self.view_mode {
                ViewMode::List => match self.active_tab {
                    Tab::Jobs => {
                        jobs::render_jobs_tab(f, area, &data.jobs, &mut self.job_table_state);
                    }
                    Tab::Suspects => {
                        suspects::render_suspects_tab(
                            f,
                            area,
                            &data.suspects,
                            &mut self.suspect_table_state,
                        );
                    }
                },
                ViewMode::JobDetail => {
                    if let Some(idx) = self.job_table_state.selected() {
                        if let Some(job) = data.jobs.get(idx) {
                            jobs::render_job_detail(
                                f,
                                area,
                                job,
                                &data.stages,
                                &data.sql_executions,
                                &mut self.detail_table_state,
                            );
                        } else {
                            // Selection out of bounds, revert
                            self.view_mode = ViewMode::List;
                        }
                    } else {
                        self.view_mode = ViewMode::List;
                    }
                }
                ViewMode::SqlDetail => {
                    if let Some(idx) = self.job_table_state.selected() {
                        if let Some(job) = data.jobs.get(idx) {
                            jobs::render_sql_detail(f, area, job, self.sql_scroll);
                        } else {
                            self.view_mode = ViewMode::List;
                        }
                    } else {
                        self.view_mode = ViewMode::List;
                    }
                }
            },
            None => {
                let msg = if self.error_msg.is_some() {
                    "Error loading data. See status bar."
                } else {
                    "Loading data..."
                };
                let block = Block::default().borders(Borders::ALL).title(msg);
                f.render_widget(block, area);
            }
        }
    }

    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        let app_id = self
            .data
            .as_ref()
            .map(|d| d.app_id.as_str())
            .unwrap_or("discovering...");
        let last_updated = self
            .data
            .as_ref()
            .map(|d| d.last_updated.as_str())
            .unwrap_or("--:--:--");

        let hint = match self.view_mode {
            ViewMode::List => "q:quit Tab:switch j/k:nav Enter:detail",
            ViewMode::JobDetail => "Esc:back j/k:nav s:sql q:quit",
            ViewMode::SqlDetail => "Esc:back j/k:scroll g/G:top/bottom q:quit",
        };

        status_line::render_status_line(
            f,
            area,
            &self.cluster_id,
            app_id,
            last_updated,
            self.error_msg.as_deref(),
            hint,
        );
    }
}
