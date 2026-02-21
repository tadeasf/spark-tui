use std::sync::Arc;

use crossterm::event::KeyCode;
use ratatui::widgets::TableState;
use tracing::warn;
use tui_scrollview::ScrollViewState;

use super::state::{App, Tab, ViewMode};
use crate::tui::Action;

impl App {
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

    pub(super) fn trigger_task_fetch(&mut self, stage_id: i64, attempt_id: i64, app_id: &str) {
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
}
