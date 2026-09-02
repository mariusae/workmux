//! Worktree table rendering for the dashboard worktree view.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Cell, Paragraph, Row, Table},
};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::super::agent;
use super::super::app::{App, WorktreeHistory};
use super::format;
use super::format::{
    AgentStatusFormat, format_agent_status_summary, format_git_status, format_pr_status, truncate,
};

/// Render the worktree table in the given area.
pub fn render_worktree_table(f: &mut Frame, app: &mut App, area: Rect) {
    // Don't render headers for an empty table - avoids a visual blink
    // as column widths jump when data arrives on the next frame
    if app.worktrees.is_empty() {
        return;
    }

    let show_check_counts = app.config.dashboard.show_check_counts();

    // Show the GitHub column when at least one worktree has a PR or checks.
    let show_pr_column = app.worktrees.iter().any(|worktree| {
        worktree.pr_info.is_some() || app.get_checks_for_worktree(worktree).is_some()
    });

    let header = format::resource_table_header(
        format::ResourceHeaderState {
            palette: &app.palette,
            spinner_frame: app.spinner_frame,
            git_fetching: app
                .is_git_fetching
                .load(std::sync::atomic::Ordering::Relaxed),
            pr_fetching: app.is_pr_fetching(),
        },
        show_pr_column,
        &["Mux", "Age", "Agent"],
    );

    // Pre-compute row data
    let row_data: Vec<_> = app
        .worktrees
        .iter()
        .enumerate()
        .map(|(idx, wt)| {
            let jump_key = if idx < 9 {
                format!("{}", idx + 1)
            } else {
                String::new()
            };

            let project = agent::extract_project_name(&wt.path);

            // Main worktree: show branch name (handle is just the repo dir name)
            // Other worktrees: show branch inline when it differs from the handle
            let worktree_display = if wt.is_main {
                wt.branch.clone()
            } else if wt.branch != wt.handle {
                format!("{} \u{2192}{}", wt.handle, wt.branch)
            } else {
                wt.handle.clone()
            };
            let worktree_display = truncate(&worktree_display, 25);

            // Git status
            let git_status = app.git_statuses.get(&wt.path);
            let git_spans = format_git_status(git_status, app.spinner_frame, &app.palette);

            // PR status (only computed if column is shown)
            let pr_spans = if show_pr_column {
                Some(format_pr_status(
                    wt.pr_info.as_ref(),
                    app.get_checks_for_worktree(wt),
                    show_check_counts,
                    app.spinner_frame,
                    &app.palette,
                ))
            } else {
                None
            };

            // Agent status summary
            let agent_spans = format_agent_status_summary(
                wt.agent_status.as_ref(),
                &app.config.status_icons,
                app.spinner_frame,
                &app.palette,
                AgentStatusFormat::TableCell,
            );

            let is_current = app.current_worktree.as_ref().is_some_and(|cwd| {
                if let (Ok(cwd_canonical), Ok(wt_canonical)) =
                    (cwd.canonicalize(), wt.path.canonicalize())
                {
                    cwd_canonical == wt_canonical
                } else {
                    wt.path == *cwd
                }
            });

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let age = wt
                .created_at
                .map(|ts| agent::format_age(now.saturating_sub(ts)));

            (
                jump_key,
                project,
                worktree_display,
                wt.is_main,
                is_current,
                git_spans,
                pr_spans,
                agent_spans,
                wt.has_mux_window,
                age,
            )
        })
        .collect();

    // Calculate dynamic column widths
    let project_names: Vec<String> = row_data.iter().map(|r| r.1.clone()).collect();
    let max_project_width = format::calc_column_width(&project_names, 5, 20, 2);

    let worktree_names: Vec<String> = row_data.iter().map(|r| r.2.clone()).collect();
    let max_worktree_width = format::calc_column_width(&worktree_names, 8, 25, 1);

    let max_git_width = row_data
        .iter()
        .map(|(_, _, _, _, _, git, _, _, _, _)| {
            git.iter()
                .map(|(text, _)| text.chars().count())
                .sum::<usize>()
        })
        .max()
        .unwrap_or(4)
        .clamp(4, 30)
        + 1;

    let max_pr_width = if show_pr_column {
        row_data
            .iter()
            .filter_map(|(_, _, _, _, _, _, pr, _, _, _)| pr.as_ref())
            .map(|spans| {
                spans
                    .iter()
                    .map(|(text, _)| text.chars().count())
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(4)
            .clamp(4, 16)
            + 1
    } else {
        0
    };

    let rows: Vec<Row> = row_data
        .into_iter()
        .map(
            |(
                jump_key,
                project,
                worktree_display,
                is_main,
                is_current,
                git_spans,
                pr_spans,
                agent_spans,
                has_mux_window,
                age,
            )| {
                let worktree_style = format::make_row_style(is_current, is_main, &app.palette);
                let git_line = format::spans_to_line(git_spans);

                let mux_cell = if has_mux_window {
                    Cell::from("\u{25cf}").style(Style::default().fg(app.palette.success))
                } else {
                    Cell::from("-").style(Style::default().fg(app.palette.dimmed))
                };

                let agent_line = format::spans_to_line(agent_spans);

                let age_cell = Cell::from(age.unwrap_or_default())
                    .style(Style::default().fg(app.palette.dimmed));

                let mut cells = vec![
                    Cell::from(jump_key).style(Style::default().fg(app.palette.keycap)),
                    Cell::from(project),
                    Cell::from(worktree_display).style(worktree_style),
                    Cell::from(git_line),
                ];

                if let Some(pr_spans) = pr_spans {
                    let pr_line = format::spans_to_line(pr_spans);
                    cells.push(Cell::from(pr_line));
                }

                cells.extend([mux_cell, age_cell]);
                cells.push(Cell::from(agent_line));

                let row = Row::new(cells);
                if is_current {
                    row.style(Style::default().bg(app.palette.current_row_bg))
                } else {
                    row
                }
            },
        )
        .collect();

    let mut constraints = vec![
        Constraint::Length(2),                    // #
        Constraint::Length(max_project_width),    // Project
        Constraint::Length(max_worktree_width),   // Worktree (+ branch when different)
        Constraint::Length(max_git_width as u16), // Git
    ];
    if show_pr_column {
        constraints.push(Constraint::Length(max_pr_width as u16));
    }
    constraints.extend([
        Constraint::Length(4), // Mux
        Constraint::Length(4), // Age
    ]);
    constraints.push(Constraint::Fill(1)); // Agent

    let highlight_symbol = Text::from(Line::from(Span::styled(
        "▌ ",
        Style::default().fg(app.palette.info),
    )));
    let table = Table::new(rows, constraints)
        .header(header)
        .block(Block::default())
        .row_highlight_style(Style::default().bg(app.palette.highlight_row_bg))
        .highlight_symbol(highlight_symbol);

    f.render_stateful_widget(table, area, &mut app.worktree_table_state);
}

/// Render the worktree preview: info panel (left) + repository history (right).
pub fn render_worktree_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let selected_worktree = app
        .worktree_table_state
        .selected()
        .and_then(|idx| app.worktrees.get(idx));

    // Split preview area into info panel (left) and history (right)
    let chunks = Layout::horizontal([
        Constraint::Length(40), // Info panel: fixed width
        Constraint::Fill(1),    // Repository history: remaining space
    ])
    .split(area);

    render_info_panel(f, app, chunks[0], selected_worktree);
    render_worktree_history(f, app, chunks[1], selected_worktree);
}

/// Render the info panel showing worktree metadata.
fn render_info_panel(
    f: &mut Frame,
    app: &App,
    area: Rect,
    worktree: Option<&crate::workflow::types::WorktreeInfo>,
) {
    let label_style = Style::default().fg(app.palette.dimmed);
    let text_style = Style::default().fg(app.palette.text);

    let title = if let Some(wt) = worktree {
        format!(" {} ", wt.handle)
    } else {
        " Info ".to_string()
    };

    let block = format::panel_block(title, &app.palette);

    let Some(wt) = worktree else {
        let paragraph = Paragraph::new(Text::raw("(no worktree selected)")).block(block);
        f.render_widget(paragraph, area);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    let identity_label = if app
        .worktree_preview
        .as_ref()
        .and_then(|preview| preview.vcs_kind)
        == Some(crate::vcs::VcsKind::Sapling)
    {
        "Label   "
    } else {
        "Branch  "
    };

    // Branch or Sapling worktree label
    lines.push(Line::from(vec![
        Span::styled(identity_label, label_style),
        Span::styled(&wt.branch, text_style),
    ]));

    // Git status details (base branch, ahead/behind, diff stats)
    let git_status = app.git_statuses.get(&wt.path);
    if let Some(status) = git_status {
        // Base branch + ahead/behind
        let mut base_spans = vec![Span::styled("Base    ", label_style)];
        if !status.base_branch.is_empty() {
            base_spans.push(Span::styled(&status.base_branch, text_style));
        } else {
            base_spans.push(Span::styled("main", text_style));
        }
        if status.ahead > 0 || status.behind > 0 {
            base_spans.push(Span::styled(" (", label_style));
            if status.ahead > 0 {
                base_spans.push(Span::styled(
                    format!("\u{2191}{}", status.ahead),
                    Style::default().fg(app.palette.info),
                ));
            }
            if status.ahead > 0 && status.behind > 0 {
                base_spans.push(Span::styled(" ", label_style));
            }
            if status.behind > 0 {
                base_spans.push(Span::styled(
                    format!("\u{2193}{}", status.behind),
                    Style::default().fg(app.palette.accent),
                ));
            }
            base_spans.push(Span::styled(")", label_style));
        }
        lines.push(Line::from(base_spans));

        // Committed diff stats
        if status.lines_added > 0 || status.lines_removed > 0 {
            let mut diff_spans = vec![Span::styled("Diff    ", label_style)];
            if status.lines_added > 0 {
                diff_spans.push(Span::styled(
                    format!("+{}", status.lines_added),
                    Style::default().fg(app.palette.success),
                ));
            }
            if status.lines_added > 0 && status.lines_removed > 0 {
                diff_spans.push(Span::styled(" ", text_style));
            }
            if status.lines_removed > 0 {
                diff_spans.push(Span::styled(
                    format!("-{}", status.lines_removed),
                    Style::default().fg(app.palette.danger),
                ));
            }
            diff_spans.push(Span::styled(" committed", label_style));
            lines.push(Line::from(diff_spans));
        }

        // Uncommitted changes
        if status.uncommitted_added > 0 || status.uncommitted_removed > 0 {
            let mut uc_spans = vec![Span::styled("        ", label_style)];
            if status.uncommitted_added > 0 {
                uc_spans.push(Span::styled(
                    format!("+{}", status.uncommitted_added),
                    Style::default().fg(app.palette.success),
                ));
            }
            if status.uncommitted_added > 0 && status.uncommitted_removed > 0 {
                uc_spans.push(Span::styled(" ", text_style));
            }
            if status.uncommitted_removed > 0 {
                uc_spans.push(Span::styled(
                    format!("-{}", status.uncommitted_removed),
                    Style::default().fg(app.palette.danger),
                ));
            }
            uc_spans.push(Span::styled(" uncommitted", label_style));
            lines.push(Line::from(uc_spans));
        }

        // Rebase indicator
        if status.is_rebasing {
            let git_icons = crate::nerdfont::git_icons();
            lines.push(Line::from(vec![
                Span::styled("        ", label_style),
                Span::styled(
                    format!("{} ", git_icons.rebase),
                    Style::default().fg(app.palette.warning),
                ),
                Span::styled("rebase in progress", label_style),
            ]));
        }

        // Conflict indicator
        if status.has_conflict {
            lines.push(Line::from(vec![
                Span::styled("        ", label_style),
                Span::styled(
                    "conflict with base",
                    Style::default().fg(app.palette.danger),
                ),
            ]));
        }
    }

    // PR info
    if let Some(ref pr) = wt.pr_info {
        let pr_icons = crate::nerdfont::pr_icons();
        let (icon, color) = if pr.is_draft {
            (pr_icons.draft, app.palette.dimmed)
        } else {
            match pr.state.as_str() {
                "OPEN" => (pr_icons.open, app.palette.success),
                "MERGED" => (pr_icons.merged, app.palette.accent),
                "CLOSED" => (pr_icons.closed, app.palette.danger),
                _ => ("?", app.palette.dimmed),
            }
        };
        let mut pr_spans = vec![
            Span::styled("PR      ", label_style),
            Span::styled(format!("#{} ", pr.number), Style::default().fg(color)),
            Span::styled(icon, Style::default().fg(color)),
        ];
        // Check status
        if let Some(ref checks) = pr.checks {
            use crate::github::CheckState;
            let check_icons = crate::nerdfont::check_icons();
            let (check_icon, check_color) = match checks {
                CheckState::Success => (check_icons.success.to_string(), app.palette.success),
                CheckState::Failure { .. } => (check_icons.failure.to_string(), app.palette.danger),
                CheckState::Pending { .. } => (check_icons.pending.to_string(), app.palette.accent),
            };
            pr_spans.push(Span::styled(" ", text_style));
            pr_spans.push(Span::styled(check_icon, Style::default().fg(check_color)));
        }
        lines.push(Line::from(pr_spans));

        // PR title (truncated to fit)
        let inner_width = area.width.saturating_sub(2) as usize; // border
        let title_max = inner_width.saturating_sub(8); // label width
        let truncated_title = if pr.title.chars().count() > title_max {
            format!(
                "{}...",
                pr.title
                    .chars()
                    .take(title_max.saturating_sub(3))
                    .collect::<String>()
            )
        } else {
            pr.title.clone()
        };
        lines.push(Line::from(vec![
            Span::styled("        ", label_style),
            Span::styled(truncated_title, Style::default().fg(color)),
        ]));

        // Check detail: failing check name or pending elapsed time
        let detail_spans = super::format::format_pr_details(pr, app.spinner_frame, &app.palette);
        if !detail_spans.is_empty() {
            let mut line_spans = vec![Span::styled("        ", label_style)];
            line_spans.extend(detail_spans);
            lines.push(Line::from(line_spans));
        }
    }

    // Agent status
    if wt.agent_status.is_some() {
        let mut agent_spans = vec![Span::styled("Agent   ", label_style)];
        for (text, style) in format_agent_status_summary(
            wt.agent_status.as_ref(),
            &app.config.status_icons,
            app.spinner_frame,
            &app.palette,
            AgentStatusFormat::DetailLine,
        ) {
            agent_spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(agent_spans));
    }

    // Mux window
    let mux_spans = vec![
        Span::styled("Mux     ", label_style),
        if wt.has_mux_window {
            Span::styled("\u{25cf} active", Style::default().fg(app.palette.success))
        } else {
            Span::styled("- none", Style::default().fg(app.palette.dimmed))
        },
    ];
    lines.push(Line::from(mux_spans));

    let paragraph = Paragraph::new(Text::from(lines)).block(block);
    f.render_widget(paragraph, area);
}

fn history_panel_title(history: Option<&WorktreeHistory>) -> &'static str {
    match history.and_then(|history| history.vcs_kind) {
        Some(crate::vcs::VcsKind::Sapling) => " Sapling Stack ",
        Some(crate::vcs::VcsKind::Git) => " Git Log ",
        None => " History ",
    }
}

static SHIMMER_START: OnceLock<Instant> = OnceLock::new();

fn shimmer_intensity(index: usize, length: usize, elapsed: Duration) -> f64 {
    if length == 0 {
        return 0.0;
    }

    let sweep = (length + 20) as f64;
    let position = ((elapsed.as_secs_f64() % 2.0) / 2.0 * sweep).floor();
    let distance = ((index + 10) as f64 - position).abs();
    if distance > 5.0 {
        0.0
    } else {
        0.5 * (1.0 + (std::f64::consts::PI * distance / 5.0).cos())
    }
}

fn blend_rgb(from: Color, to: Color, amount: f64) -> Option<Color> {
    let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (from, to) else {
        return None;
    };
    let blend =
        |a: u8, b: u8| (f64::from(a) * (1.0 - amount) + f64::from(b) * amount).floor() as u8;
    Some(Color::Rgb(blend(fr, tr), blend(fg, tg), blend(fb, tb)))
}

fn shimmer_history_title_at(
    elapsed: Duration,
    palette: &super::theme::ThemePalette,
) -> Line<'static> {
    const TITLE: &str = "History";
    let mut spans = Vec::with_capacity(TITLE.len() + 2);
    spans.push(Span::raw(" "));

    for (index, character) in TITLE.chars().enumerate() {
        let intensity = shimmer_intensity(index, TITLE.chars().count(), elapsed);
        let style = if let Some(color) = blend_rgb(palette.dimmed, palette.header, intensity * 0.9)
        {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else if intensity < 0.2 {
            Style::default()
                .fg(palette.header)
                .add_modifier(Modifier::DIM)
                .remove_modifier(Modifier::BOLD)
        } else if intensity < 0.6 {
            Style::default()
                .fg(palette.header)
                .remove_modifier(Modifier::BOLD | Modifier::DIM)
        } else {
            Style::default()
                .fg(palette.header)
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM)
        };
        spans.push(Span::styled(character.to_string(), style));
    }

    spans.push(Span::raw(" "));
    Line::from(spans)
}

fn history_panel_title_line(
    history: Option<&WorktreeHistory>,
    palette: &super::theme::ThemePalette,
) -> Line<'static> {
    match history {
        Some(history) => Line::from(history_panel_title(Some(history))),
        None => {
            shimmer_history_title_at(SHIMMER_START.get_or_init(Instant::now).elapsed(), palette)
        }
    }
}

/// Render the selected worktree's Git log or Sapling smartlog.
fn render_worktree_history(
    f: &mut Frame,
    app: &App,
    area: Rect,
    worktree: Option<&crate::workflow::types::WorktreeInfo>,
) {
    let title = if worktree.is_some() {
        history_panel_title_line(app.worktree_preview.as_ref(), &app.palette)
    } else {
        Line::from(history_panel_title(None))
    };
    let block = format::panel_block(title, &app.palette);

    let text = match (&app.worktree_preview, worktree) {
        (Some(history), Some(_)) => match &history.content {
            Err(error) => Text::from(Line::styled(error, Style::default().fg(app.palette.danger))),
            Ok(log) if log.trim().is_empty() => {
                let empty = if history.vcs_kind == Some(crate::vcs::VcsKind::Sapling) {
                    "(no draft stack)"
                } else {
                    "(no commits)"
                };
                Text::raw(empty)
            }
            Ok(log) if history.vcs_kind == Some(crate::vcs::VcsKind::Sapling) => Text::from(
                log.lines()
                    .map(|line| Line::styled(line, Style::default().fg(app.palette.text)))
                    .collect::<Vec<_>>(),
            ),
            Ok(log) => {
                let hash_style = Style::default().fg(app.palette.accent);
                let date_style = Style::default().fg(app.palette.dimmed);
                let msg_style = Style::default().fg(app.palette.text);

                let lines: Vec<Line> = log
                    .lines()
                    .map(|line| {
                        let parts: Vec<&str> = line.splitn(3, '\t').collect();
                        if parts.len() == 3 {
                            Line::from(vec![
                                Span::styled(parts[0], hash_style),
                                Span::styled("  ", date_style),
                                Span::styled(parts[1], date_style),
                                Span::styled("  ", msg_style),
                                Span::styled(parts[2], msg_style),
                            ])
                        } else {
                            // Fallback for lines that don't match format
                            Line::styled(line, msg_style)
                        }
                    })
                    .collect();
                Text::from(lines)
            }
        },
        (None, Some(_)) => Text::raw(""),
        (_, None) => Text::raw(""),
    };

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_title_tracks_repository_backend() {
        let git = WorktreeHistory {
            vcs_kind: Some(crate::vcs::VcsKind::Git),
            content: Ok(String::new()),
        };
        let sapling = WorktreeHistory {
            vcs_kind: Some(crate::vcs::VcsKind::Sapling),
            content: Ok(String::new()),
        };

        assert_eq!(history_panel_title(Some(&git)), " Git Log ");
        assert_eq!(history_panel_title(Some(&sapling)), " Sapling Stack ");
        assert_eq!(history_panel_title(None), " History ");
    }

    #[test]
    fn shimmer_sweeps_across_history_title() {
        assert_eq!(shimmer_intensity(0, 7, Duration::ZERO), 0.0);
        assert_eq!(shimmer_intensity(3, 7, Duration::from_secs(1)), 1.0);
        assert!(
            shimmer_intensity(3, 7, Duration::from_secs(1))
                > shimmer_intensity(6, 7, Duration::from_secs(1))
        );
    }

    #[test]
    fn shimmer_title_preserves_text() {
        let palette = crate::ui::theme::ThemePalette::for_scheme(
            crate::config::ThemeScheme::Default,
            crate::config::ThemeMode::Dark,
        );
        let title = shimmer_history_title_at(Duration::from_secs(1), &palette);
        assert_eq!(title.to_string(), " History ");
    }
}
