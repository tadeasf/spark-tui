use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Tabs},
};
use tokio::sync::mpsc;

use super::state::{App, Tab, ViewMode};
use crate::tui::Action;
use crate::tui::tabs::{job_detail, jobs_list, sql_detail, stage_detail, suspects};
use crate::tui::widgets::{help, status_line, summary_bar};

impl App {
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
                        jobs_list::render_jobs_tab(f, area, &data.jobs, &mut self.job_table_state);
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
                            job_detail::render_job_detail(
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
                            sql_detail::render_sql_detail(
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
                                    stage_detail::render_stage_detail(
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
