use std::collections::HashSet;
use std::sync::Arc;

use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, TableState, Tabs},
};
use tokio::sync::mpsc;
use tracing::warn;

use crate::fetch::client::SparkHttpClient;

use super::tabs::{jobs, suspects};
use super::widgets::{status_line, summary_bar};
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
    StageDetail,
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
    pub stage_detail_scroll: u16,
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
            sql_scroll: 0,
            stage_detail_scroll: 0,
            client,
            tx,
            pending_task_fetches: HashSet::new(),
        }
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Key(key) => self.handle_key(key),
            Action::DataUpdate(mut payload) => {
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
                    let mut merged =
                        (*payload.stage_tasks).clone();
                    for (stage_id, tasks) in old_data.stage_tasks.iter() {
                        merged.entry(*stage_id).or_insert_with(|| tasks.clone());
                    }
                    payload.stage_tasks = Arc::new(merged);
                }

                self.data = Some(payload);
            }
            Action::FetchError(err) => {
                self.error_msg = Some(err.to_string());
            }
            Action::TaskDataLoaded(stage_id, tasks) => {
                self.pending_task_fetches.remove(&stage_id);
                if let Some(data) = &self.data {
                    let mut map = (*data.stage_tasks).clone();
                    map.insert(stage_id, tasks);
                    let mut new_data = data.clone();
                    new_data.stage_tasks = Arc::new(map);
                    self.data = Some(new_data);
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
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => match self.view_mode {
                ViewMode::List => self.should_quit = true,
                ViewMode::JobDetail => self.view_mode = ViewMode::List,
                ViewMode::SqlDetail => self.view_mode = ViewMode::JobDetail,
                ViewMode::StageDetail => self.view_mode = ViewMode::JobDetail,
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
            KeyCode::Down | KeyCode::Char('j') if self.view_mode == ViewMode::StageDetail => {
                self.stage_detail_scroll = self.stage_detail_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.view_mode == ViewMode::StageDetail => {
                self.stage_detail_scroll = self.stage_detail_scroll.saturating_sub(1);
            }
            KeyCode::Home | KeyCode::Char('g') if self.view_mode == ViewMode::StageDetail => {
                self.stage_detail_scroll = 0;
            }
            KeyCode::End | KeyCode::Char('G') if self.view_mode == ViewMode::StageDetail => {
                self.stage_detail_scroll = u16::MAX;
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
            ViewMode::JobDetail => {
                // Enter on a stage → StageDetail
                if let Some(stage_idx) = self.detail_table_state.selected() {
                    if let Some(data) = &self.data {
                        if let Some(job_idx) = self.job_table_state.selected() {
                            if let Some(job) = data.jobs.get(job_idx) {
                                // Verify the stage exists in this job
                                let job_stages: Vec<&crate::fetch::types::SparkStage> = data
                                    .stages
                                    .iter()
                                    .filter(|s| job.stage_ids.contains(&s.stage_id))
                                    .collect();
                                if let Some(stage) = job_stages.get(stage_idx) {
                                    self.stage_detail_scroll = 0;
                                    self.view_mode = ViewMode::StageDetail;

                                    // Trigger on-demand task fetch if not already loaded/pending
                                    let sid = stage.stage_id;
                                    let aid = stage.attempt_id;
                                    if !data.stage_tasks.contains_key(&sid)
                                        && !self.pending_task_fetches.contains(&sid)
                                    {
                                        self.pending_task_fetches.insert(sid);
                                        let client = Arc::clone(&self.client);
                                        let tx = self.tx.clone();
                                        let app_id = data.app_id.clone();
                                        tokio::spawn(async move {
                                            match client
                                                .get_task_list(&app_id, sid, aid)
                                                .await
                                            {
                                                Ok(tasks) => {
                                                    let _ =
                                                        tx.send(Action::TaskDataLoaded(sid, tasks));
                                                }
                                                Err(e) => {
                                                    let _ = tx.send(Action::TaskFetchFailed(
                                                        sid,
                                                        e.to_string(),
                                                    ));
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ViewMode::SqlDetail | ViewMode::StageDetail => {
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
        if show_summary
            && let Some(data) = &self.data
        {
            summary_bar::render_summary_bar(f, chunks[1], &data.summary);
        }
        self.render_content(f, chunks[2]);
        self.render_status_bar(f, chunks[3]);
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
                                    let tasks = data
                                        .stage_tasks
                                        .get(&stage.stage_id)
                                        .map(|v| v.as_slice());
                                    let loading =
                                        self.pending_task_fetches.contains(&stage.stage_id);
                                    jobs::render_stage_detail(
                                        f,
                                        area,
                                        stage,
                                        tasks,
                                        loading,
                                        self.stage_detail_scroll,
                                        data.cluster_resources.total_executor_memory,
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
            ViewMode::List => "q:quit Tab:switch j/k:nav Enter:detail",
            ViewMode::JobDetail => "Esc:back j/k:nav Enter:stage s:sql q:quit",
            ViewMode::SqlDetail => "Esc:back j/k:scroll g/G:top/bottom q:quit",
            ViewMode::StageDetail => "Esc:back j/k:scroll g/G:top/bottom q:quit",
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
