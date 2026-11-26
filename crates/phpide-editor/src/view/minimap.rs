//! The minimap: the whole file in small, to the right of the text.
//!
//! Lines are drawn in blocks of [`BLOCK_LINES`] into images on a worker
//! thread, with the theme's syntax colors, and shown as textures. An edit
//! redraws only the blocks it touched (found from the document's change
//! log; when lines were added or removed, the blocks after the edit too),
//! at most every [`REDRAW_EVERY`] while typing; the old image stays until
//! the new one is ready. Only blocks on screen are drawn. Until the syntax
//! tree is ready the text is drawn without colors and redrawn when it
//! arrives.
//!
//! When the file is taller than the panel, the minimap scrolls in
//! proportion to the editor, as in VS Code: at the top of the file it shows
//! its top, at the bottom its bottom.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use phpide_core::{Document, Revision};
use phpide_php::SyntaxRole;
use ropey::Rope;

use crate::syntax::HighlightSource;

/// Lines per image.
pub const BLOCK_LINES: usize = 256;
/// Height of a line in the minimap, in points (and image pixels).
pub const LINE_PITCH: f32 = 2.0;
/// Images asked of the worker and not back yet.
const MAX_JOBS: usize = 2;
/// A block changed by typing is redrawn at most this often.
pub const REDRAW_EVERY: Duration = Duration::from_millis(150);
/// Blocks kept when they are off screen.
const KEEP_BLOCKS: usize = 48;

/// How text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MinimapRender {
    /// A dot per character, lighter for punctuation: the shape of the text.
    #[default]
    Characters,
    /// A bar per character.
    Blocks,
}

/// When the slider (the visible area) shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MinimapSlider {
    #[default]
    Always,
    /// Only with the pointer over the minimap.
    Hover,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MinimapOptions {
    pub enabled: bool,
    /// Width in points (also the number of columns drawn).
    pub width: f32,
    pub render: MinimapRender,
    pub slider: MinimapSlider,
}

impl Default for MinimapOptions {
    fn default() -> Self {
        MinimapOptions {
            enabled: false,
            width: 100.0,
            render: MinimapRender::Characters,
            slider: MinimapSlider::Always,
        }
    }
}

/// Draws lines `first..first + BLOCK_LINES` of `rope` into an image
/// `cols` pixels wide and two pixels per line.
pub fn render_block(
    rope: &Rope,
    source: &HighlightSource,
    first: usize,
    cols: usize,
    tab_size: usize,
    colors: &[Color32; SyntaxRole::COUNT],
    render: MinimapRender,
) -> ColorImage {
    let height = BLOCK_LINES * LINE_PITCH as usize;
    let mut image = ColorImage::new(
        [cols.max(1), height],
        vec![Color32::TRANSPARENT; cols.max(1) * height],
    );
    let lines = rope.len_lines();
    if first >= lines || cols == 0 {
        return image;
    }
    let last = (first + BLOCK_LINES - 1).min(lines - 1);
    let spans = source.spans(rope, first, last);
    for (offset, line) in (first..=last).enumerate() {
        let line_spans = spans.get(offset).map(Vec::as_slice).unwrap_or(&[]);
        let y = offset * LINE_PITCH as usize;
        let mut cell = 0usize;
        let mut span = 0usize;
        for (column, c) in rope.line(line).chars().enumerate() {
            if cell >= cols || c == '\n' || c == '\r' {
                break;
            }
            if c == '\t' {
                cell += tab_size.max(1) - cell % tab_size.max(1);
                continue;
            }
            if c.is_whitespace() {
                cell += 1;
                continue;
            }
            while span < line_spans.len() && line_spans[span].end <= column {
                span += 1;
            }
            let role = match line_spans.get(span) {
                Some(s) if s.start <= column => s.role,
                _ => SyntaxRole::Plain,
            };
            let color = colors[role as usize];
            let (top, bottom) = match render {
                MinimapRender::Characters if c.is_alphanumeric() || c == '_' || c == '$' => {
                    (1.0, 0.65)
                }
                MinimapRender::Characters => (0.55, 0.3),
                MinimapRender::Blocks => (0.85, 0.0),
            };
            image.pixels[y * cols + cell] = color.gamma_multiply(top);
            if bottom > 0.0 {
                image.pixels[(y + 1) * cols + cell] = color.gamma_multiply(bottom);
            }
            cell += 1;
        }
    }
    image
}

/// What a frame of the minimap shows: its scroll, the slider and the line
/// under a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapGeometry {
    /// Panel height.
    pub height: f32,
    /// Lines of the document.
    pub lines: usize,
    /// Minimap scroll, in points.
    pub scroll: f32,
    /// Slider top (relative to the panel) and height.
    pub slider_top: f32,
    pub slider_height: f32,
}

impl MinimapGeometry {
    /// `editor_scroll` of `max_scroll`; `first_line` and `visible_lines`
    /// are what the editor shows.
    pub fn new(
        height: f32,
        lines: usize,
        editor_scroll: f32,
        max_scroll: f32,
        first_line: usize,
        visible_lines: usize,
    ) -> Self {
        let content = lines as f32 * LINE_PITCH;
        let frac = if max_scroll > 0.0 {
            (editor_scroll / max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let scroll = frac * (content - height).max(0.0);
        MinimapGeometry {
            height,
            lines,
            scroll,
            slider_top: first_line as f32 * LINE_PITCH - scroll,
            slider_height: (visible_lines as f32 * LINE_PITCH).max(4.0),
        }
    }

    /// The line drawn at `y` (relative to the panel), clamped.
    pub fn line_at(&self, y: f32) -> usize {
        (((y + self.scroll) / LINE_PITCH).max(0.0) as usize).min(self.lines.saturating_sub(1))
    }

    /// Top of `line` (relative to the panel).
    pub fn line_top(&self, line: usize) -> f32 {
        line as f32 * LINE_PITCH - self.scroll
    }

    /// Editor scroll per point of slider movement: the slider travels the
    /// part of the panel the file fills.
    pub fn drag_ratio(&self, max_scroll: f32) -> f32 {
        let travel =
            (self.height.min(self.lines as f32 * LINE_PITCH) - self.slider_height).max(1.0);
        max_scroll / travel
    }
}

struct Block {
    image: Option<Arc<ColorImage>>,
    texture: Option<TextureHandle>,
    /// Text changed since the image was drawn.
    stale: bool,
    /// The image has syntax colors.
    colored: bool,
    /// Drawn with these options and colors.
    key: u64,
    pending: bool,
    /// When the last image was asked for.
    asked: Option<Instant>,
    used: u64,
}

struct Job {
    block: usize,
    key: u64,
    rope: Rope,
    source: HighlightSource,
    cols: usize,
    tab_size: usize,
    colors: [Color32; SyntaxRole::COUNT],
    render: MinimapRender,
    ctx: egui::Context,
}

struct Done {
    block: usize,
    key: u64,
    colored: bool,
    image: ColorImage,
}

/// The blocks of one view.
pub struct MinimapState {
    blocks: HashMap<usize, Block>,
    revision: Option<Revision>,
    lines: usize,
    /// The worker, started with the first image.
    worker: Option<mpsc::Sender<Job>>,
    sender: mpsc::Sender<Done>,
    receiver: mpsc::Receiver<Done>,
    jobs: usize,
    frame: u64,
}

impl Default for MinimapState {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        MinimapState {
            blocks: HashMap::new(),
            revision: None,
            lines: 0,
            worker: None,
            sender,
            receiver,
            jobs: 0,
            frame: 0,
        }
    }
}

/// The worker yields the CPU to the UI thread (Linux: `nice` of this
/// thread only).
fn lower_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: plain syscalls on the calling thread's own id.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 10);
    }
}

/// Draws images one after another; ends with the view.
fn start_worker(done: mpsc::Sender<Done>) -> mpsc::Sender<Job> {
    let (sender, jobs) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("minimap".into())
        .spawn(move || {
            lower_priority();
            while let Ok(job) = jobs.recv() {
                let image = render_block(
                    &job.rope,
                    &job.source,
                    job.block * BLOCK_LINES,
                    job.cols,
                    job.tab_size,
                    &job.colors,
                    job.render,
                );
                let sent = done.send(Done {
                    block: job.block,
                    key: job.key,
                    colored: job.source.is_colored(),
                    image,
                });
                if sent.is_err() {
                    return;
                }
                job.ctx.request_repaint();
            }
        })
        .expect("minimap thread");
    sender
}

/// Everything a block is drawn with, for [`MinimapState::update`].
pub struct BlockInputs<'a> {
    pub source: HighlightSource,
    pub cols: usize,
    pub tab_size: usize,
    pub colors: &'a [Color32; SyntaxRole::COUNT],
    pub render: MinimapRender,
}

impl BlockInputs<'_> {
    fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (self.cols, self.tab_size, self.render as u8).hash(&mut hasher);
        for color in self.colors {
            color.to_array().hash(&mut hasher);
        }
        hasher.finish()
    }
}

impl MinimapState {
    /// Marks the blocks the edits since the last call touched.
    fn follow(&mut self, doc: &Document) {
        let revision = doc.revision();
        let lines = doc.line_count();
        let Some(seen) = self.revision.filter(|r| *r != revision) else {
            self.revision = Some(revision);
            self.lines = lines;
            return;
        };
        let touched = doc.changes_since(seen).map(|changes| {
            let mut lo = usize::MAX;
            let mut hi = 0;
            for e in changes.iter().flat_map(|c| c.byte_edits.iter()) {
                lo = lo.min(e.start_point.row);
                hi = hi.max(e.old_end_point.row).max(e.new_end_point.row);
            }
            (lo, hi)
        });
        let shifted = lines != self.lines;
        for (&index, block) in self.blocks.iter_mut() {
            let first = index * BLOCK_LINES;
            let last = first + BLOCK_LINES - 1;
            block.stale |= match touched {
                Some((lo, hi)) if lo <= hi => {
                    if shifted {
                        last >= lo
                    } else {
                        first <= hi && last >= lo
                    }
                }
                Some(_) => false,
                None => true,
            };
        }
        self.revision = Some(revision);
        self.lines = lines;
    }

    /// Takes finished images and asks for the blocks `visible` needs.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        doc: &Document,
        visible: std::ops::Range<usize>,
        inputs: BlockInputs<'_>,
    ) {
        self.frame += 1;
        self.follow(doc);
        while let Ok(done) = self.receiver.try_recv() {
            self.jobs = self.jobs.saturating_sub(1);
            if let Some(block) = self.blocks.get_mut(&done.block) {
                block.pending = false;
                block.key = done.key;
                block.colored = done.colored;
                // The texture keeps its id; only its pixels change.
                if let Some(texture) = &mut block.texture {
                    texture.set(done.image.clone(), TextureOptions::NEAREST);
                }
                block.image = Some(Arc::new(done.image));
            }
        }
        let key = inputs.key();
        let colored = inputs.source.is_colored();
        let now = Instant::now();
        let mut wait: Option<Duration> = None;
        for index in visible.clone() {
            let block = self.blocks.entry(index).or_insert(Block {
                image: None,
                texture: None,
                stale: true,
                colored: false,
                key: 0,
                pending: false,
                asked: None,
                used: 0,
            });
            block.used = self.frame;
            let missing = block.image.is_none() || block.key != key || colored && !block.colored;
            // Typing: the old image stays a moment, so a burst of keys
            // costs one redraw.
            let rested = block
                .asked
                .is_none_or(|at| now.duration_since(at) >= REDRAW_EVERY);
            if block.stale && !missing && !rested {
                let left = REDRAW_EVERY - now.duration_since(block.asked.unwrap_or(now));
                wait = Some(wait.map_or(left, |w| w.min(left)));
            }
            let wanted = missing || block.stale && rested;
            if wanted && !block.pending && self.jobs < MAX_JOBS {
                block.pending = true;
                block.stale = false;
                block.asked = Some(now);
                self.jobs += 1;
                let worker = self
                    .worker
                    .get_or_insert_with(|| start_worker(self.sender.clone()));
                let _ = worker.send(Job {
                    block: index,
                    key,
                    rope: doc.rope().clone(),
                    source: inputs.source.clone(),
                    cols: inputs.cols,
                    tab_size: inputs.tab_size,
                    colors: *inputs.colors,
                    render: inputs.render,
                    ctx: ctx.clone(),
                });
            }
            if block.texture.is_none() {
                if let Some(image) = &block.image {
                    block.texture = Some(ctx.load_texture(
                        format!("minimap-{index}-{}", self.frame),
                        ColorImage::clone(image),
                        TextureOptions::NEAREST,
                    ));
                }
            }
        }
        if let Some(wait) = wait {
            ctx.request_repaint_after(wait);
        }
        // Off-screen blocks beyond the budget go, oldest first.
        if self.blocks.len() > KEEP_BLOCKS {
            let mut idle: Vec<(u64, usize)> = self
                .blocks
                .iter()
                .filter(|(i, b)| !visible.contains(i) && !b.pending)
                .map(|(i, b)| (b.used, *i))
                .collect();
            idle.sort();
            let extra = self.blocks.len() - KEEP_BLOCKS;
            for (_, index) in idle.into_iter().take(extra) {
                self.blocks.remove(&index);
            }
        }
    }

    /// The texture of a block, when drawn.
    pub fn texture(&self, index: usize) -> Option<&TextureHandle> {
        self.blocks.get(&index).and_then(|b| b.texture.as_ref())
    }

    /// Drops the textures (kept images are uploaded again on the next
    /// frame): for a render in another egui context.
    pub fn reload_textures(&mut self) {
        for block in self.blocks.values_mut() {
            block.texture = None;
        }
    }

    /// Images drawn so far are current (tests).
    pub fn idle(&self) -> bool {
        self.jobs == 0 && self.blocks.values().all(|b| !b.pending && !b.stale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> [Color32; SyntaxRole::COUNT] {
        let mut colors = [Color32::from_rgb(200, 200, 200); SyntaxRole::COUNT];
        colors[SyntaxRole::Keyword as usize] = Color32::from_rgb(255, 0, 0);
        colors
    }

    #[test]
    fn blocks_draw_characters_with_their_colors() {
        let rope = Rope::from_str("<?php\n\tfunction a() {}\n");
        let plain = render_block(
            &rope,
            &HighlightSource::Plain,
            0,
            40,
            4,
            &colors(),
            MinimapRender::Characters,
        );
        assert_eq!(plain.size, [40, BLOCK_LINES * 2]);
        // "<?php": '<' is punctuation (lighter), 'p' a letter.
        let px = |x: usize, y: usize| plain.pixels[y * 40 + x];
        assert!(px(2, 0).a() > px(0, 0).a());
        assert!(px(2, 1).a() > 0 && px(2, 1).a() < px(2, 0).a());
        // The tab moves "function" to column 4.
        assert_eq!(px(3, 2), Color32::TRANSPARENT);
        assert_ne!(px(4, 2), Color32::TRANSPARENT);
        // Past the text: nothing.
        assert_eq!(px(30, 2), Color32::TRANSPARENT);
        assert_eq!(px(0, 10), Color32::TRANSPARENT);

        let blocks = render_block(
            &rope,
            &HighlightSource::Plain,
            0,
            40,
            4,
            &colors(),
            MinimapRender::Blocks,
        );
        assert_ne!(blocks.pixels[2], Color32::TRANSPARENT);
        assert_eq!(blocks.pixels[40 + 2], Color32::TRANSPARENT, "a gap row");
    }

    #[test]
    fn tree_colors_reach_the_image() {
        let doc = Document::new("<?php\nfunction a() {}\n");
        let mut syntax = crate::Syntax::for_path(Some(std::path::Path::new("a.php")));
        syntax.sync(&doc);
        let source = syntax.highlight_source(&doc);
        assert!(source.is_colored());
        let image = render_block(
            doc.rope(),
            &source,
            0,
            20,
            4,
            &colors(),
            MinimapRender::Characters,
        );
        // "function" is a keyword: red.
        let keyword = image.pixels[2 * 20];
        assert!(keyword.r() > keyword.g(), "{keyword:?}");
    }

    #[test]
    fn geometry_scrolls_in_proportion() {
        // 1,000 lines (2,000 points) in a 500-point panel.
        let top = MinimapGeometry::new(500.0, 1000, 0.0, 10_000.0, 0, 25);
        assert_eq!(top.scroll, 0.0);
        assert_eq!((top.slider_top, top.slider_height), (0.0, 50.0));
        assert_eq!(top.line_at(100.0), 50);
        let bottom = MinimapGeometry::new(500.0, 1000, 10_000.0, 10_000.0, 975, 25);
        assert_eq!(bottom.scroll, 1500.0);
        assert_eq!(bottom.slider_top, 450.0, "the slider reaches the bottom");
        assert_eq!(bottom.line_at(499.0), 999);
        assert_eq!(bottom.line_top(999), 498.0);
        // The slider travels 450 points for 10,000 points of editor scroll.
        assert!((top.drag_ratio(10_000.0) - 10_000.0 / 450.0).abs() < 0.01);

        // A short file: no minimap scroll; the slider travels the file.
        let short = MinimapGeometry::new(500.0, 100, 500.0, 1000.0, 25, 25);
        assert_eq!(short.scroll, 0.0);
        assert_eq!(short.slider_top, 50.0);
        assert!((short.drag_ratio(1000.0) - 1000.0 / 150.0).abs() < 0.01);
    }

    #[test]
    fn edits_mark_only_the_blocks_they_touch() {
        let text: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        let mut buffer = phpide_core::Buffer::untitled();
        buffer.replace_all(&text);
        let mut state = MinimapState::default();
        state.follow(&buffer.document);
        for index in 0..4 {
            state.blocks.insert(
                index,
                Block {
                    image: None,
                    texture: None,
                    stale: false,
                    colored: false,
                    key: 0,
                    pending: false,
                    asked: None,
                    used: 0,
                },
            );
        }
        let stale = |state: &MinimapState| -> Vec<usize> {
            let mut v: Vec<usize> = state
                .blocks
                .iter()
                .filter(|(_, b)| b.stale)
                .map(|(i, _)| *i)
                .collect();
            v.sort();
            v
        };
        // Same line count: only block 1 (lines 256..511).
        let at = buffer.document.rope().line_to_char(300);
        buffer.insert(at, "x");
        state.follow(&buffer.document);
        assert_eq!(stale(&state), [1]);
        for b in state.blocks.values_mut() {
            b.stale = false;
        }
        // A new line shifts every block from there on.
        let at = buffer.document.rope().line_to_char(600);
        buffer.insert(at, "new\n");
        state.follow(&buffer.document);
        assert_eq!(stale(&state), [2, 3]);
    }
}
