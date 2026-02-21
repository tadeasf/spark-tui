use std::collections::HashSet;
use std::sync::Arc;

use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, TableState, Tabs},
};
use tokio::sync::mpsc;
use tracing::warn;
use tui_scrollview::ScrollViewState;

use crate::fetch::client::SparkHttpClient;

use super::tabs::{jobs, suspects};
use super::widgets::{help, status_line, summary_bar};
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
        let len = Tab::titles().len();
        Tab::from_index((self.index() + 1) % len)
    }

    pub fn prev(self) -> Self {
        let len = Tab::titles().len();
        Tab::from_index((self.index() + len - 1) % len)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    JobDetail,
    SqlDetail,
    StageDetail,
}

pub struct App {
    pub active_tab: Tab,
    pub view_mode: ViewMode,
    pub data: Option<Arc<DataPayload>>,
    pub error_msg: Option<String>,
    pub cluster_id: String,
    pub should_quit: bool,
    pub job_table_state: TableState,
    pub suspect_table_state: TableState,
    pub detail_table_state: TableState,
    pub sql_scroll_state: ScrollViewState,
    pub stage_detail_scroll_state: ScrollViewState,
    show_help: bool,
    return_tab: Option<Tab>,
    client: Arc<SparkHttpClient>,
    tx: mpsc::UnboundedSender<Action>,
    pending_task_fetches: HashSet<i64>,
}

impl App {
    pub fn new(
        cluster_id: String,
        client: Arc<SparkHttpClient>,
        tx: mpsc::UnboundedSender<Action>,
    ) -> Self {
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
            sql_scroll_state: ScrollViewState::default(),
            stage_detail_scroll_state: ScrollViewState::default(),
            show_help: false,
            return_tab: None,
            client,
            tx,
            pending_task_fetches: HashSet::new(),
        }
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Key(key) => self.handle_key(key),
            Action::DataUpdate(payload) => {
                let mut payload = *payload;
                self.error_msg = None;

                // Preserve selection across refresh
                #[allow(clippy::collapsible_if)]
                if self.view_mode == ViewMode::JobDetail
                    || self.view_mode == ViewMode::SqlDetail
                    || self.view_mode == ViewMode::StageDetail
                {
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

                // Preserve on-demand fetched task data across poller refreshes
                if let Some(old_data) = &self.data {
                    let mut merged = (*payload.stage_tasks).clone();
                    for (stage_id, tasks) in old_data.stage_tasks.iter() {
                        merged.entry(*stage_id).or_insert_with(|| tasks.clone());
                    }
                    payload.stage_tasks = Arc::new(merged);
                }

                self.data = Some(Arc::new(payload));
            }
            Action::FetchError(err) => {
                self.error_msg = Some(err.to_string());
            }
            Action::TaskDataLoaded(stage_id, tasks) => {
                self.pending_task_fetches.remove(&stage_id);
                if let Some(arc_data) = self.data.take() {
                    let mut data = Arc::unwrap_or_clone(arc_data);
                    let mut map = (*data.stage_tasks).clone();
                    map.insert(stage_id, tasks);
                    data.stage_tasks = Arc::new(map);
                    self.data = Some(Arc::new(data));
                }
            }
            Action::TaskFetchFailed(stage_id, err) => {
                self.pending_task_fetches.remove(&stage_id);
                warn!("Failed to fetch tasks for stage {}: {}", stage_id, err);
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
            ViewMode::SqlDetail | ViewMode::StageDetail => None, // Scroll-based, no table
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // When help overlay is shown, intercept keys
        if self.show_help {
            match key.code {
                KeyCode::Char('h') | KeyCode::Esc => self.show_help = false,
                KeyCode::Char('q') => self.should_quit = true,
                _ => {} // swallow all other keys
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.handle_escape(),
            KeyCode::Tab if self.view_mode == ViewMode::List => {
                self.active_tab = self.active_tab.next();
            }
            KeyCode::BackTab if self.view_mode == ViewMode::List => {
                self.active_tab = self.active_tab.prev();
            }
            KeyCode::Char('s') if self.view_mode == ViewMode::JobDetail => {
                self.open_sql_detail();
            }
            KeyCode::Enter => self.handle_enter(),
            KeyCode::Down | KeyCode::Char('j') => self.handle_navigation_down(),
            KeyCode::Up | KeyCode::Char('k') => self.handle_navigation_up(),
            KeyCode::Home | KeyCode::Char('g') => self.handle_navigation_home(),
            KeyCode::End | KeyCode::Char('G') => self.handle_navigation_end(),
            KeyCode::Char('h') => self.show_help = true,
            _ => {}
        }
    }

    fn handle_escape(&mut self) {
        match self.view_mode {
            ViewMode::List => self.should_quit = true,
            ViewMode::JobDetail => {
                self.view_mode = ViewMode::List;
                if let Some(tab) = self.return_tab.take() {
                    self.active_tab = tab;
                }
            }
            ViewMode::SqlDetail => self.view_mode = ViewMode::JobDetail,
            ViewMode::StageDetail => self.view_mode = ViewMode::JobDetail,
        }
    }

    fn open_sql_detail(&mut self) {
        if let Some(data) = &self.data
            && let Some(idx) = self.job_table_state.selected()
            && let Some(job) = data.jobs.get(idx)
            && job.sql_id.is_some()
        {
            self.sql_scroll_state = ScrollViewState::default();
            self.view_mode = ViewMode::SqlDetail;
        }
    }

    fn handle_navigation_down(&mut self) {
        match self.view_mode {
            ViewMode::SqlDetail => {
                self.sql_scroll_state.scroll_down();
            }
            ViewMode::StageDetail => {
                self.stage_detail_scroll_state.scroll_down();
            }
            _ => {
                if let Some(ts) = self.active_table_state() {
                    ts.select_next();
                }
            }
        }
    }

    fn handle_navigation_up(&mut self) {
        match self.view_mode {
            ViewMode::SqlDetail => {
                self.sql_scroll_state.scroll_up();
            }
            ViewMode::StageDetail => {
                self.stage_detail_scroll_state.scroll_up();
            }
            _ => {
                if let Some(ts) = self.active_table_state() {
                    ts.select_previous();
                }
            }
        }
    }

    fn handle_navigation_home(&mut self) {
        match self.view_mode {
            ViewMode::SqlDetail => {
                self.sql_scroll_state.scroll_to_top();
            }
            ViewMode::StageDetail => {
                self.stage_detail_scroll_state.scroll_to_top();
            }
            _ => {
                if let Some(ts) = self.active_table_state() {
                    ts.select_first();
                }
            }
        }
    }

    fn handle_navigation_end(&mut self) {
        match self.view_mode {
            ViewMode::SqlDetail => {
                self.sql_scroll_state.scroll_to_bottom();
            }
            ViewMode::StageDetail => {
                self.stage_detail_scroll_state.scroll_to_bottom();
            }
            _ => {
                if let Some(ts) = self.active_table_state() {
                    ts.select_last();
                }
            }
        }
    }

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
                Tab::Suspects => self.enter_suspect_job(),
            },
            ViewMode::JobDetail => self.enter_stage_detail(),
            ViewMode::SqlDetail | ViewMode::StageDetail => {}
        }
    }

    fn enter_suspect_job(&mut self) {
        let Some(idx) = self.suspect_table_state.selected() else {
            return;
        };
        let Some(data) = &self.data else { return };
        let Some(suspect) = data.suspects.get(idx) else {
            return;
        };
        let Some(job_id) = suspect.job_id else { return };
        let Some(job_idx) = data.jobs.iter().position(|j| j.job_id == job_id) else {
            return;
        };
        self.return_tab = Some(self.active_tab);
        self.active_tab = Tab::Jobs;
        self.job_table_state.select(Some(job_idx));
        self.detail_table_state = TableState::default();
        self.detail_table_state.select_first();
        self.view_mode = ViewMode::JobDetail;
    }

    fn enter_stage_detail(&mut self) {
        let Some(stage_idx) = self.detail_table_state.selected() else {
            return;
        };
        let Some(data) = &self.data else { return };
        let Some(job_idx) = self.job_table_state.selected() else {
            return;
        };
        let Some(job) = data.jobs.get(job_idx) else {
            return;
        };
        let job_stages: Vec<&crate::fetch::types::SparkStage> = data
            .stages
            .iter()
            .filter(|s| job.stage_ids.contains(&s.stage_id))
            .collect();
        let Some(stage) = job_stages.get(stage_idx) else {
            return;
        };
        self.stage_detail_scroll_state = ScrollViewState::default();
        self.view_mode = ViewMode::StageDetail;
        self.trigger_task_fetch(stage.stage_id, stage.attempt_id, &data.app_id.clone());
    }

    fn trigger_task_fetch(&mut self, stage_id: i64, attempt_id: i64, app_id: &str) {
        if let Some(data) = &self.data
            && (data.stage_tasks.contains_key(&stage_id)
                || self.pending_task_fetches.contains(&stage_id))
        {
            return;
        }
        self.pending_task_fetches.insert(stage_id);
        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        let app_id = app_id.to_string();
        tokio::spawn(async move {
            match client.get_task_list(&app_id, stage_id, attempt_id).await {
                Ok(tasks) => {
                    let _ = tx.send(Action::TaskDataLoaded(stage_id, tasks));
                }
                Err(e) => {
                    let _ = tx.send(Action::TaskFetchFailed(stage_id, e.to_string()));
                }
            }
        });
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
        mut rx: mpsc::UnboundedReceiver<Action>,
    ) -> std::io::Result<()> {
        let mut prev_view = self.view_mode;
        let mut prev_tab = self.active_tab;

        while !self.should_quit {
            // Physical terminal clear only on view/tab transitions (not scroll)
            if self.view_mode != prev_view || self.active_tab != prev_tab {
                terminal.clear()?;
                prev_view = self.view_mode;
                prev_tab = self.active_tab;
            }

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
        // Reset every cell in the buffer to prevent stale content
        f.render_widget(Clear, f.area());

        let show_summary = self.view_mode == ViewMode::List && self.data.is_some();
        let chunks = if show_summary {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // tab bar
                    Constraint::Length(2), // summary bar
                    Constraint::Fill(1),   // content area
                    Constraint::Length(1), // status bar
                ])
                .split(f.area())
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // tab bar
                    Constraint::Length(0), // no summary bar
                    Constraint::Fill(1),   // content area
                    Constraint::Length(1), // status bar
                ])
                .split(f.area())
        };

        self.render_tab_bar(f, chunks[0]);
        if show_summary && let Some(data) = &self.data {
            summary_bar::render_summary_bar(f, chunks[1], &data.summary);
        }
        self.render_content(f, chunks[2]);
        self.render_status_bar(f, chunks[3]);

        if self.show_help {
            match self.view_mode {
                ViewMode::SqlDetail => {
                    if let Some(data) = &self.data
                        && let Some(idx) = self.job_table_state.selected()
                        && let Some(job) = data.jobs.get(idx)
                    {
                        help::render_sql_help_overlay(f, f.area(), job, &data.suspects);
                    }
                }
                _ => help::render_help_overlay(f, f.area()),
            }
        }
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
                Style::default()
                    .fg(Color::Cyan)
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
                            &data.critical_stages,
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
                                &data.critical_stages,
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
                            jobs::render_sql_detail(
                                f,
                                area,
                                job,
                                &mut self.sql_scroll_state,
                                &data.suspects,
                            );
                        } else {
                            self.view_mode = ViewMode::List;
                        }
                    } else {
                        self.view_mode = ViewMode::List;
                    }
                }
                ViewMode::StageDetail => {
                    if let Some(job_idx) = self.job_table_state.selected() {
                        if let Some(job) = data.jobs.get(job_idx) {
                            if let Some(stage_idx) = self.detail_table_state.selected() {
                                let job_stages: Vec<&crate::fetch::types::SparkStage> = data
                                    .stages
                                    .iter()
                                    .filter(|s| job.stage_ids.contains(&s.stage_id))
                                    .collect();
                                if let Some(stage) = job_stages.get(stage_idx) {
                                    let tasks =
                                        data.stage_tasks.get(&stage.stage_id).map(|v| v.as_slice());
                                    let loading =
                                        self.pending_task_fetches.contains(&stage.stage_id);
                                    let sql_hint = data
                                        .stage_sql_hints
                                        .get(&stage.stage_id)
                                        .map(|s| s.as_str());
                                    jobs::render_stage_detail(
                                        f,
                                        area,
                                        stage,
                                        tasks,
                                        loading,
                                        &mut self.stage_detail_scroll_state,
                                        data.cluster_resources.total_executor_memory,
                                        sql_hint,
                                    );
                                } else {
                                    self.view_mode = ViewMode::JobDetail;
                                }
                            } else {
                                self.view_mode = ViewMode::JobDetail;
                            }
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
            ViewMode::List => "q:quit Tab:switch j/k:nav Enter:detail h:help",
            ViewMode::JobDetail => "Esc:back j/k:nav Enter:stage s:sql h:help",
            ViewMode::SqlDetail => "Esc:back j/k:scroll g/G:top/bot h:hints",
            ViewMode::StageDetail => "Esc:back j/k:scroll g/G:top/bot h:help",
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
