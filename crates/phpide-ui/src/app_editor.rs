//! The editor surface: breadcrumbs, the text area and its decorations.

use super::*;

impl PhpIdeApp {
    /// Breadcrumbs (REQUISITOS 1.4): trilha estrutural sob o cursor, clicável.
    pub(crate) fn breadcrumbs(&mut self, ui: &mut egui::Ui) {
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        if !phpide_php::Php::is_php_file(&path) {
            return;
        }
        let line0 = buf
            .document
            .line_col(self.cursor_char.min(buf.document.len_chars()))
            .0
            .saturating_sub(1);
        let Some((sp, cache)) = &self.syntax else {
            return;
        };
        if sp != &path {
            return;
        }
        // Blocos vigentes na revisão atual (cache inválido ⇒ sem trilha).
        let blocks = match self.ws.active_buffer() {
            Some(buf) => cache.get(&buf.document).cloned().unwrap_or_default(),
            None => Vec::new(),
        };
        let trail: Vec<(String, usize)> = blocks
            .iter()
            .filter(|b| {
                let end = if b.kind == "namespace_definition" {
                    usize::MAX
                } else {
                    b.end_line0
                };
                b.start_line0 <= line0 && end >= line0
            })
            .map(|b| {
                let label = if b.name.is_empty() {
                    b.kind.clone()
                } else {
                    format!("{} {}", short_kind(&b.kind), b.name)
                };
                (label, b.start_line0)
            })
            .collect();
        if trail.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (i, (label, line)) in trail.iter().enumerate() {
                if i > 0 {
                    ui.label(egui::RichText::new("/").weak());
                }
                if ui.link(label).clicked() {
                    if let Some(buf) = self.ws.active_buffer_mut() {
                        buf.request_cursor(*line, 0);
                    }
                }
            }
        });
        ui.separator();
    }

    /// Pinta a superfície do editor sobre o viewport (Estágio 6.2 + Fase 5):
    /// gutter, estados de breakpoint, chevrons de folding, sticky lines,
    /// hints/Code Vision, blame, markers agregados da scrollbar e lens
    /// preview — geometria exata da galley, projeção de folding quando há
    /// dobras (linhas do documento mapeadas para linhas visuais).
    pub(crate) fn paint_editor_surface(
        &mut self,
        ui: &egui::Ui,
        output: &egui::text_edit::TextEditOutput,
        proj: Option<&folding::FoldProjection>,
    ) {
        use phpide_core::Severity;
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        // Snapshot: empréstimo imutável termina antes das mutações abaixo.
        let (doc_total_lines, cursor_line1, text, blocks, saved_hash, buffer_dirty) = {
            let Some(buf) = self.ws.active_buffer() else {
                return;
            };
            let blocks: Vec<phpide_syntax::Block> = self
                .syntax
                .as_ref()
                .filter(|(p, _)| p == &path)
                .and_then(|(_, cache)| cache.get(&buf.document).cloned())
                .unwrap_or_default();
            (
                buf.document.line_count(),
                buf.document
                    .line_col(self.cursor_char.min(buf.document.len_chars()))
                    .0,
                buf.document.text(),
                blocks,
                blame_gutter::text_hash(buf.document.text().as_str()),
                buf.dirty,
            )
        };
        let Some(first_row) = output.galley.rows.first() else {
            return;
        };
        let row_height = first_row.rect().height();
        let viewport = ui.clip_rect();
        let geom = editor_view::EditorGeometry {
            galley_pos: output.galley_pos,
            viewport,
            row_height,
        };
        // Mapeamento doc→visual (identidade sem folding).
        let (total_lines, doc_to_visual): (usize, Option<Vec<usize>>) = match proj {
            Some(p) => (p.text.lines().count(), Some(p.doc_to_visual.clone())),
            None => (doc_total_lines, None),
        };
        let map_line = |doc_line1: usize| -> usize {
            match &doc_to_visual {
                Some(map) => map
                    .get(doc_line1.saturating_sub(1))
                    .copied()
                    .unwrap_or(usize::MAX),
                None => doc_line1.saturating_sub(1),
            }
        };

        // Gutter: números/bookmarks/coverage na numeração visual; breakpoints
        // entram vazios aqui e são pintados por estado logo abaixo.
        let bps = self.rd.bp_lines_for(&path);
        let bms_doc = self.bookmarks.get(&path).cloned().unwrap_or_default();
        let bms: std::collections::BTreeSet<usize> = bms_doc.iter().map(|&l| map_line(l)).collect();
        let coverage_doc = self
            .rd
            .coverage
            .as_ref()
            .and_then(|report| report.file(&path).cloned());
        let coverage_view = coverage_doc.map(|cov| {
            let mut mapped = phpide_run::coverage::FileCoverage::default();
            for (&k, &v) in cov.lines.iter() {
                let vline = map_line(k as usize);
                if vline != usize::MAX {
                    mapped.lines.insert(vline as u32 + 1, v);
                }
            }
            mapped
        });
        let action = editor_view::paint_gutter(
            ui,
            &geom,
            total_lines,
            cursor_line1,
            &std::collections::BTreeSet::new(),
            &bms,
            output.galley.rows.len(),
            coverage_view.as_ref(),
        );
        if let Some(line1) = action.toggle_breakpoint {
            // Linha visual → documento.
            let doc_line1 = match proj {
                Some(p) => p
                    .visual_to_doc
                    .get(line1.saturating_sub(1))
                    .map(|d| d + 1)
                    .unwrap_or(line1),
                None => line1,
            };
            self.toggle_breakpoint_at(doc_line1);
        }
        // Discos de breakpoint com estado real (UI-ED-001-04).
        let muted = self.rd.bp_muted();
        let palette = crate::design::current(ui.ctx()).palette;
        let num_x = viewport.left() + editor_view::GUTTER_WIDTH - 7.0;
        for doc_line1 in &bps {
            let vline = map_line(*doc_line1);
            if vline == usize::MAX {
                continue;
            }
            let y = geom.row_y(vline);
            if y + row_height < viewport.top() || y > viewport.bottom() {
                continue;
            }
            let status = self.rd.bp_status_for(&path, *doc_line1);
            let visual = status
                .as_ref()
                .map(|s| editor_surface::bp_visual(s, muted))
                .unwrap_or(editor_surface::BpVisual::Enabled);
            editor_surface::paint_breakpoint_disc(
                ui.painter(),
                egui::pos2(num_x, y + row_height * 0.5),
                row_height * 0.32,
                visual,
                &palette,
            );
        }
        // Chevrons de folding (UI-ED-002-03).
        if self.settings.folding {
            let rev_now = self
                .ws
                .active_buffer()
                .map(|b| b.document.revision())
                .unwrap_or(0);
            let cached = self
                .fold_cache
                .as_ref()
                .filter(|(fp, fr, _)| fp == &path && *fr == rev_now)
                .map(|(_, _, r)| r.clone());
            if let Some(ranges) = cached {
                let visuals = self.folds.get(&path).map(|f| f.visuals());
                let folded: std::collections::BTreeSet<usize> = self
                    .folds
                    .get(&path)
                    .map(|f| (0..f.ranges.len()).filter(|&i| f.is_folded(i)).collect())
                    .unwrap_or_default();
                if let Some(visuals) = visuals {
                    let act = editor_surface::paint_fold_chevrons(
                        ui,
                        &geom,
                        viewport.left(),
                        editor_view::GUTTER_WIDTH * 0.42,
                        &ranges,
                        &visuals,
                        &folded,
                        proj.map(|p| p.doc_to_visual.as_slice()),
                        total_lines,
                        self.settings.fold_icons_always,
                    );
                    if let Some(idx) = act.toggle {
                        if let Some(f) = self.folds.get_mut(&path) {
                            f.toggle(idx);
                        }
                    }
                }
            }
        }
        // Sticky lines (UI-ED-004): limite configurável (0 = desliga).
        let sticky_max = self.settings.sticky_lines_max as usize;
        if self.settings.sticky_lines && sticky_max > 0 && !blocks.is_empty() {
            let doc_lines: Vec<String> = text.lines().map(str::to_string).collect();
            let first_visible = match proj {
                Some(p) => p
                    .visual_to_doc
                    .get(geom.first_visible_line())
                    .copied()
                    .unwrap_or(geom.first_visible_line()),
                None => geom.first_visible_line(),
            };
            let jump = editor_view::paint_sticky_lines_max(
                ui,
                &geom,
                &doc_lines,
                &blocks,
                first_visible,
                sticky_max,
                proj.map(|p| p.doc_to_visual.as_slice()),
            );
            if let Some(line0) = jump {
                if let Some(buf) = self.ws.active_buffer_mut() {
                    buf.request_cursor(line0, 0);
                }
            }
        }
        // Inlay hints (UI-ED-005-03) — filtrados pelas configurações.
        if self.settings.inlay_hints {
            let hs = hints::HintSettings {
                enabled: true,
                all_arguments: self.settings.hint_all_arguments,
                pass_by_reference: self.settings.hint_pass_by_reference,
                exclude: self.settings.hint_exclude.clone(),
            };
            let filtered = hs.filter(self.hints.hints());
            editor_surface::paint_inlay_hints(
                ui,
                &geom,
                &output.galley,
                output.galley_pos,
                &filtered,
                proj.map(|p| p.doc_to_visual.as_slice()),
                total_lines,
            );
        }
        // Code Vision (UI-ED-005-05/06).
        if let Some(clicked) = editor_surface::paint_code_vision(
            ui,
            &geom,
            &self.code_vision.lenses,
            proj.map(|p| p.doc_to_visual.as_slice()),
            total_lines,
            editor_view::GUTTER_WIDTH,
        ) {
            if let Some(buf) = self.ws.active_buffer_mut() {
                buf.request_cursor(clicked, 0);
            }
            self.request_usages();
        }
        // Gutter de blame (UI-ED-007-01): dados reais, invalidação por
        // revisão salva; buffer sujo = anotações "stale" sinalizadas.
        if self.settings.blame_gutter {
            let rev = self
                .ws
                .active_buffer()
                .map(|b| b.document.revision())
                .unwrap_or(0);
            let needs_load =
                !buffer_dirty && self.blame.is_stale(&path.to_string_lossy(), saved_hash);
            if needs_load {
                let ctx = ui.ctx().clone();
                self.git
                    .request_blame(&self.jobs, &ctx, &path, saved_hash, rev);
            }
            let stale = buffer_dirty
                || !matches!(&self.blame_revision, Some((p, r)) if p == &path && *r == rev);
            let strip = egui::Rect::from_min_size(
                egui::pos2(viewport.left() + editor_view::GUTTER_WIDTH, viewport.top()),
                egui::vec2(76.0, viewport.height()),
            );
            editor_surface::paint_blame_strip(
                ui,
                &geom,
                strip,
                &|line1| self.blame.annotate(line1),
                total_lines,
                stale,
            );
        }
        // Markers agregados da scrollbar: diagnósticos + Git (UI-ED-006-01).
        let diags_doc: Vec<(u32, Severity)> = self
            .ws
            .diagnostics_for(&path)
            .map(|d| (d.line as u32, d.severity))
            .collect();
        let diags: Vec<(u32, Severity)> = diags_doc
            .iter()
            .map(|(l, s)| ((map_line(*l as usize) as u32).saturating_add(1), *s))
            .filter(|(l, _)| (*l as usize) < usize::MAX)
            .collect();
        if self
            .git_markers
            .as_ref()
            .map(|(p, _, _)| p != &path)
            .unwrap_or(true)
        {
            self.git_markers = None;
        }
        let rev = self
            .ws
            .active_buffer()
            .map(|b| b.document.revision())
            .unwrap_or(0);
        let git_fresh = matches!(&self.git_markers, Some((p, _, _)) if p == &path)
            && lens::markers_are_current(
                self.git_markers
                    .as_ref()
                    .map(|(_, revision, _)| *revision)
                    .unwrap_or(u64::MAX),
                rev,
                buffer_dirty,
            );
        if !git_fresh && !buffer_dirty {
            let ctx = ui.ctx().clone();
            self.git.request_markers(&self.jobs, &ctx, &path, rev);
        }
        // Markers only go stale through unsaved edits; while a refresh after
        // save is in flight nothing is shown instead of outdated lines.
        let git_stale = buffer_dirty
            && self
                .git_markers
                .as_ref()
                .filter(|(p, _, _)| p == &path)
                .is_some();
        let git: Vec<(u32, bool)> = if !git_fresh {
            Vec::new()
        } else {
            self.git_markers
                .as_ref()
                .filter(|(p, _, _)| p == &path)
                .map(|(_, _, mk)| {
                    mk.iter()
                        .map(|(l, added)| {
                            ((map_line(*l as usize) as u32).saturating_add(1), *added)
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        if git_stale {
            ui.painter().text(
                egui::pos2(viewport.right() - 150.0, viewport.top() + 4.0),
                egui::Align2::LEFT_TOP,
                "Git: save to refresh",
                egui::FontId::proportional(10.0),
                ui.visuals().weak_text_color(),
            );
        }
        let markers = lens::aggregate_scrollbar_markers(&diags, &git, total_lines);
        editor_surface::paint_scroll_markers(ui, &geom, &markers);
        // Lens preview (UI-ED-006-02).
        if self.settings.lens_mode {
            if let Some(line0) = editor_surface::scrollbar_hover_line(ui, &geom, total_lines) {
                let pointer = ui
                    .ctx()
                    .input(|input| input.pointer.hover_pos())
                    .unwrap_or_else(|| geom.viewport.center());
                self.lens_hover = Some((line0, pointer, std::time::Instant::now()));
            } else if self
                .lens_hover
                .as_ref()
                .map(|(_, _, t)| t.elapsed() > std::time::Duration::from_millis(400))
                .unwrap_or(false)
            {
                self.lens_hover = None;
            }
            if let Some((line0, pointer, _)) = self.lens_hover {
                let doc_line0 = match proj {
                    Some(p) => p.visual_to_doc.get(line0).copied().unwrap_or(line0),
                    None => line0,
                };
                let doc_lines: Vec<String> = text.lines().map(str::to_string).collect();
                let preview = lens::lens_preview(&doc_lines, doc_line0, 3);
                egui::Area::new(egui::Id::new("lens-preview"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(lens::popup_position(
                        pointer,
                        viewport,
                        egui::vec2(360.0, 168.0),
                    ))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            for l in &preview.lines {
                                ui.label(egui::RichText::new(l).monospace().size(11.0));
                            }
                        });
                    });
            }
        }
        // Resumo de inspeções do arquivo (widget UI-ED-006-03, status bar).
        self.inspections = lens::inspections_summary(&diags_doc);
        // Ocorrências do símbolo (T-PHP-005-02): só na revisão calculada.
        if let Some((op, or_, _word, ranges)) = &self.occurrences {
            if active_path_matches(&self.ws, op, *or_) && !ranges.is_empty() {
                editor_view::paint_occurrences(ui, &output.galley, output.galley_pos, ranges);
            }
        }
    }

    pub(crate) fn editor(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let mut want_completion = false;
        // Hook de demonstração/teste: rola o editor para uma linha no startup
        // (permite capturar sticky lines/gutter sem simulação de input).
        let scroll_hook: Option<f32> = std::env::var("PHPIDE_SCROLL_TO_LINE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .map(|line| (line - 1.0).max(0.0));
        // Offset de scroll calculado pelo mapa centralizado com a altura de
        // linha efetiva do zoom (antes: 18.0 fixo, que desalinhava com zoom).
        let scroll_hook = scroll_hook.map(|line0| {
            let zoom = self.zoom.max(0.01);
            let row = 18.0 * zoom;
            let g = geometry::Geometry::new(row, 600.0, usize::MAX);
            g.line_to_content_y(line0 as usize)
        });
        let editor_bg = crate::design::current(ui.ctx()).palette.bg_editor;
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).fill(editor_bg))
            .show(ui, |ui| {
                self.breadcrumbs(ui);
                let active = self.ws.active;
                if self.ws.active_buffer().is_none() {
                    ui.centered_and_justified(|ui| {
                        ui.label("No file open.");
                    });
                    return;
                }

                let split_layout = self.groups.root_split();
                macro_rules! primary_pane {
                ($ui:expr) => {{
                    let ui = $ui;
            // Salto pendente (palette, busca, problemas): posiciona cursor.
            let pending = self
                .ws
                .active_buffer_mut()
                .and_then(|buf| buf.take_pending_cursor());
            if let Some((line0, col0)) = pending {
                let char_at = self
                    .ws
                    .active_buffer()
                    .map(|buf| {
                        (buf.document.line_start(line0) + col0).min(buf.document.len_chars())
                    })
                    .unwrap_or(0);
                let id = egui::Id::new(("editor", active));
                let mut st = TextEditState::load(ui.ctx(), id).unwrap_or_default();
                let mut cc = egui::text::CCursor::new(char_at);
                cc.prefer_next_row = true;
                st.cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(cc)));
                st.store(ui.ctx(), id);
                self.cursor_char = char_at;
            }

            // Snapshot do estado necessário ao widget (borrow termina aqui).
            let (old, readonly) = {
                let buf = self.ws.active_buffer().unwrap();
                (buf.document.text(), buf.readonly)
            };
            // ---- Fase 5: folding (UI-ED-002) — ranges por revisão ----
            let path_key = self.ws.active_buffer().and_then(|b| b.path.clone());
            if let (Some(path), Some(rev)) = (
                path_key.clone(),
                self.ws.active_buffer().map(|b| b.document.revision()),
            ) {
                if !self.switcher.is_cycling() {
                    self.switcher.touch(&path.to_string_lossy());
                }
                if self.settings.folding {
                    let fresh =
                        matches!(&self.fold_cache, Some((p, r, _)) if p == &path && *r == rev);
                    if !fresh {
                        let ranges =
                            phpide_syntax::folding::folding_ranges(&old, old.lines().count());
                        let doc_lines: Vec<String> = old.lines().map(str::to_string).collect();
                        self.fold_cache = Some((path.clone(), rev, ranges.clone()));
                        self.folds
                            .entry(path)
                            .or_default()
                            .update(ranges, &doc_lines);
                    }
                }
            }
            let fold_ctx = path_key.clone().and_then(|path| {
                if !self.settings.folding {
                    return None;
                }
                let visuals = self.folds.get(&path)?.visuals();
                if visuals.is_empty() {
                    return None;
                }
                Some((path, folding::project(&old, &visuals), visuals))
            });
            let mut edited = match &fold_ctx {
                Some((_, proj, _)) => proj.text.clone(),
                None => old.clone(),
            };
            // Semantic tokens (T-PHP-005-01): spans de byte da revisão
            // vigente do arquivo ativo; aplicados como cor de texto por
            // cima do highlight sintático (sem flicker: só atualiza por
            // revisão e por resposta do servidor).
            let semantic_spans: Option<Vec<(usize, usize, String)>> = self
                .semantic
                .as_ref()
                .filter(|(p, r, _, _)| active_path_matches(&self.ws, p, *r))
                .map(|(_, _, _, spans)| spans.clone());
            let syntax_style = crate::design::current(ui.ctx()).syntax_style();
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                let mut job = PhpLanguage.highlight(text.as_str(), &syntax_style);
                if let Some(spans) = &semantic_spans {
                    apply_semantic_colors(&mut job, spans, &syntax_style);
                }
                job.wrap.max_width = wrap_width;
                ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
            };
            // O gutter é pintado por cima do viewport; o texto ganha um
            // recuo à esquerda para nunca ficar sob ele.
            let scroll_area = egui::ScrollArea::vertical().auto_shrink([false; 2]);
            let scroll_area = match scroll_hook {
                // `line` já é offset em pixels (line_to_content_y).
                Some(offset_y) => scroll_area.vertical_scroll_offset(offset_y),
                None => scroll_area,
            };
            let scroll = scroll_area.show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(editor_view::GUTTER_WIDTH);
                    egui::TextEdit::multiline(&mut edited)
                        .frame(egui::Frame::NONE)
                        .id(egui::Id::new(("editor", active)))
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(40)
                        .lock_focus(true)
                        .interactive(!readonly)
                        .layouter(&mut layouter)
                        .show(ui)
                })
                .inner
            });
            let output = scroll.inner;

            if let Some(cursor) = output.cursor_range {
                self.cursor_char = cursor.primary.index.0;
            }
            // Caret em região dobrada revela a dobra (UI-ED-002-03): o texto
            // projetado some; o caret volta a enxergar a região.
            if let Some((path, proj, _)) = &fold_ctx {
                let vline = edited
                    .chars()
                    .take(self.cursor_char.min(edited.chars().count()))
                    .filter(|c| *c == '\n')
                    .count();
                if let Some(&doc_line) = proj.visual_to_doc.get(vline) {
                    let visuals = self.folds.get(path).map(|f| f.visuals());
                    if let Some(v) = visuals {
                        if v.is_hidden(doc_line) {
                            let target = v.fold_at(doc_line).map(|(s, _)| s);
                            let st = self.folds.get_mut(path).unwrap();
                            if let Some(idx) = target
                                .and_then(|s| st.ranges.iter().position(|r| r.start_line0 == s))
                            {
                                st.toggle(idx);
                            }
                        }
                    }
                }
            }

            // ---- Estágio 6.2+Fase 5: superfície do editor ----
            self.paint_editor_surface(ui, &output, fold_ctx.as_ref().map(|(_, p, _)| p));

            // Com folding, o TextEdit editou o texto projetado: mapeia a
            // edição de volta ao documento (bandas são read-only).
            let (old_for_diff, edited_for_diff) = match &fold_ctx {
                Some((_, proj, _)) => match folding::commit(proj, &edited, &old) {
                    folding::ProjectedCommit::NewDocText(t) => (old.clone(), t),
                    folding::ProjectedCommit::Unchanged => (old.clone(), old.clone()),
                },
                None => (old.clone(), edited.clone()),
            };
            if old_for_diff != edited_for_diff {
                if let Some((at, removed, inserted)) =
                    Self::diff_edit(&old_for_diff, &edited_for_diff)
                {
                    let mut edited_buf = self.ws.active_buffer_mut();
                    if let Some(buf) = edited_buf.take() {
                        if removed > 0 {
                            buf.delete(at, at + removed);
                        }
                        if !inserted.is_empty() {
                            buf.insert(at, &inserted);
                            // Gatilhos de completamento: $, -> e \ (namespace)
                            if matches!(inserted.chars().last(), Some('$' | '>' | '\\')) {
                                want_completion = true;
                            }
                        }
                    }
                }
            }
            }}}
                macro_rules! secondary_pane {
                    ($ui:expr) => {{
                        let ui = $ui;
                        let second = match &self.groups.root {
                            groups::GroupNode::Split { children, .. } => match &children[1] {
                                groups::GroupNode::Leaf { views, .. } => {
                                    views.first().map(|v| v.buffer.clone())
                                }
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(key) = second {
                            let buf_idx = self.ws.buffers.iter().position(|b| {
                                b.path
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .as_deref()
                                    == Some(key.as_str())
                            });
                            if let Some(idx) = buf_idx {
                                ui.separator();
                                let (text2, readonly2) = {
                                    let b = &self.ws.buffers[idx];
                                    (b.document.text(), b.readonly)
                                };
                                let mut edited2 = text2.clone();
                                if let Some((_, sizes)) = self.groups.root_split() {
                                    let mut first = sizes[0];
                                    if ui
                                        .add(egui::Slider::new(&mut first, 0.1..=0.9).text("split"))
                                        .changed()
                                    {
                                        self.groups.resize_root(first);
                                    }
                                }
                                egui::ScrollArea::vertical()
                                    .id_salt(("pane2", idx))
                                    .max_height(ui.available_height())
                                    .show(ui, |ui| {
                                        egui::TextEdit::multiline(&mut edited2)
                                            .id(egui::Id::new(("editor", idx, "pane2")))
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .interactive(!readonly2)
                                            .show(ui);
                                    });
                                if edited2 != text2 {
                                    if let Some((at, removed, inserted)) =
                                        Self::diff_edit(&text2, &edited2)
                                    {
                                        let b = &mut self.ws.buffers[idx];
                                        if removed > 0 {
                                            b.delete(at, at + removed);
                                        }
                                        if !inserted.is_empty() {
                                            b.insert(at, &inserted);
                                        }
                                    }
                                }
                                // Fechar o painel: remove a vista do segundo grupo.
                                if ui.small_button("✕ Unsplit").clicked() {
                                    while self.groups.close_active_in_root_second().is_some() {}
                                }
                            }
                        }
                    }};
                }
                if let Some((axis, sizes)) = split_layout {
                    let panels = groups::split_rect(ui.available_rect_before_wrap(), axis, sizes);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(panels[0]), |ui| {
                        primary_pane!(ui)
                    });
                    ui.scope_builder(egui::UiBuilder::new().max_rect(panels[1]), |ui| {
                        secondary_pane!(ui)
                    });
                } else {
                    primary_pane!(ui);
                }
            });
        if want_completion && !self.completion.open {
            self.trigger_completion();
        }
        // Popup do switcher MRU (UI-ED-003-02): desenha por cima de tudo.
        let mut labels = std::collections::HashMap::new();
        for b in &self.ws.buffers {
            if let Some(p) = &b.path {
                labels.insert(
                    p.to_string_lossy().into_owned(),
                    groups::tab_title(&p.to_string_lossy(), b.dirty, false),
                );
            }
        }
        switcher::popup(ctx, &self.switcher, &labels, 0);
    }
}

/// `true` se o buffer ativo é `path` na revisão `revision` (semantic
/// tokens e ocorrências só valem para o estado em que foram calculados).
/// DocStore da aplicação: buffers abertos (sujeitos a undo de buffer) têm
/// prioridade; arquivos fechados são lidos/escritos em disco (escrita
/// atômica). Versões LSP locais não são rastreadas por arquivo aqui
/// (conflitos de versão são reportados pelo servidor quando aplicável).
pub(crate) struct WsStore<'a>(pub(crate) &'a mut phpide_core::Workspace);

impl workspace_edit::DocStore for WsStore<'_> {
    fn text(&self, path: &std::path::Path) -> Result<String, phpide_core::IdeError> {
        if let Some(buffer) = self
            .0
            .buffers
            .iter()
            .find(|buffer| buffer.path.as_deref() == Some(path))
        {
            return Ok(buffer.document.text());
        }
        std::fs::read_to_string(path).map_err(|error| {
            phpide_core::IdeError::from_io(
                Component::Lsp,
                "workspace_edit_read",
                &path.display().to_string(),
                &error,
            )
        })
    }

    fn set_text(
        &mut self,
        path: &std::path::Path,
        text: &str,
    ) -> Result<(), phpide_core::IdeError> {
        if let Some(buffer) = self
            .0
            .buffers
            .iter_mut()
            .find(|buffer| buffer.path.as_deref() == Some(path))
        {
            buffer.replace_all(text);
            return Ok(());
        }
        phpide_core::fsutil::write_atomic(path, text.as_bytes()).map_err(|error| {
            phpide_core::IdeError::from_io(
                Component::Lsp,
                "workspace_edit_write",
                &path.display().to_string(),
                &error,
            )
        })
    }

    fn lsp_version(&self, path: &std::path::Path) -> Option<i64> {
        self.0
            .buffers
            .iter()
            .find(|buffer| buffer.path.as_deref() == Some(path))
            .map(|buffer| buffer.document.revision() as i64)
    }
}

pub(crate) fn active_path_matches(ws: &Workspace, path: &Path, revision: u64) -> bool {
    ws.active_buffer()
        .map(|b| b.path.as_deref() == Some(path) && b.document.revision() == revision)
        .unwrap_or(false)
}

/// Semantic color of an LSP token type (T-PHP-005-01), from the theme's
/// syntax palette. `None` keeps the syntactic color.
pub(crate) fn semantic_color(
    token_type: &str,
    syntax: &phpide_php::SyntaxStyle,
) -> Option<egui::Color32> {
    phpide_php::SyntaxRole::from_semantic_token(token_type).map(|role| syntax.color(role))
}

/// Sobrepõe cores semânticas no job de layout: a seção cuja posição de
/// início cai dentro de um span de token recebe a cor do tipo. Spans e
/// seções estão ordenados — caminhada única O(n+m) por frame.
pub(crate) fn apply_semantic_colors(
    job: &mut egui::text::LayoutJob,
    spans: &[(usize, usize, String)],
    syntax: &phpide_php::SyntaxStyle,
) {
    let mut span_idx = 0usize;
    for section in &mut job.sections {
        let section_start = section.byte_range.start.0;
        while span_idx < spans.len() && spans[span_idx].1 <= section_start {
            span_idx += 1;
        }
        if span_idx >= spans.len() {
            break;
        }
        let (s, e, ty) = &spans[span_idx];
        if section_start >= *s && section_start < *e {
            if let Some(color) = semantic_color(ty, syntax) {
                section.format.color = color;
            }
        }
    }
}
