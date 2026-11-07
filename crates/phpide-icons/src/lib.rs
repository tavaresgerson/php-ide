//! Iconografia da IDE — `T-UI-ICON-001-01/02/03/04` (REQUISITOS UI-ICON-001).
//!
//! Conjunto visual **próprio** (`assets/*.svg`, traço geométrico 24×24,
//! `currentColor`) escolhido por coerência com a restrição offline: nenhuma
//! dependência de Icones/Iconify em tempo de execução, licença do workspace
//! (`MIT OR Apache-2.0`) e manifesto com nome/origem/autor/URL/licença por
//! arquivo (`assets/manifest.json`).
//!
//! - Código referencia **significados semânticos** (`IconId::Run`,
//!   `IconId::GitBranch`…), nunca nomes de arquivo espalhados;
//! - [`Registry`] resolve semanticamente com **cache** por (tint, escala) e
//!   **fallback** garantido para id desconhecido;
//! - tint acontece por substituição de `currentColor` no SVG (UI-ICON-001-04);
//! - tamanhos 16/20/24, HiDPI por `pixels_per_point` e **nome acessível** +
//!   tooltip por ícone.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Identificadores semânticos do catálogo (UI-ICON-001-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum IconId {
    Run,
    Debug,
    Stop,
    Pause,
    Settings,
    Close,
    Warning,
    Error,
    Info,
    Project,
    Folder,
    File,
    GitBranch,
    Search,
    Bookmark,
    Breakpoint,
    ChevronRight,
    ChevronDown,
    SplitRight,
    SplitDown,
    Switcher,
    Eye,
    Blame,
    Coverage,
    Hint,
    /// Id inexistente/fora do catálogo: resolve para o fallback.
    Unknown,
}

impl IconId {
    /// Slug estável (igual ao nome de arquivo, sem extensão).
    pub const fn slug(self) -> &'static str {
        match self {
            IconId::Run => "run",
            IconId::Debug => "debug",
            IconId::Stop => "stop",
            IconId::Pause => "pause",
            IconId::Settings => "settings",
            IconId::Close => "close",
            IconId::Warning => "warning",
            IconId::Error => "error",
            IconId::Info => "info",
            IconId::Project => "project",
            IconId::Folder => "folder",
            IconId::File => "file",
            IconId::GitBranch => "git-branch",
            IconId::Search => "search",
            IconId::Bookmark => "bookmark",
            IconId::Breakpoint => "breakpoint",
            IconId::ChevronRight => "chevron-right",
            IconId::ChevronDown => "chevron-down",
            IconId::SplitRight => "split-right",
            IconId::SplitDown => "split-down",
            IconId::Switcher => "switcher",
            IconId::Eye => "eye",
            IconId::Blame => "blame",
            IconId::Coverage => "coverage",
            IconId::Hint => "hint",
            IconId::Unknown => "unknown",
        }
    }

    /// Nome acessível (leitor de tela / tooltip padrão).
    pub const fn accessible_name(self) -> &'static str {
        match self {
            IconId::Run => "Executar",
            IconId::Debug => "Depurar",
            IconId::Stop => "Parar",
            IconId::Pause => "Pausar",
            IconId::Settings => "Configurações",
            IconId::Close => "Fechar",
            IconId::Warning => "Aviso",
            IconId::Error => "Erro",
            IconId::Info => "Informação",
            IconId::Project => "Projeto",
            IconId::Folder => "Pasta",
            IconId::File => "Arquivo",
            IconId::GitBranch => "Branch do Git",
            IconId::Search => "Buscar",
            IconId::Bookmark => "Bookmark",
            IconId::Breakpoint => "Breakpoint",
            IconId::ChevronRight => "Expandir",
            IconId::ChevronDown => "Recolher",
            IconId::SplitRight => "Dividir para a direita",
            IconId::SplitDown => "Dividir para baixo",
            IconId::Switcher => "Switcher de abas",
            IconId::Eye => "Pré-visualizar",
            IconId::Blame => "Autoria (blame)",
            IconId::Coverage => "Cobertura",
            IconId::Hint => "Dica",
            IconId::Unknown => "Ícone",
        }
    }

    /// Todos os ids do catálogo (sem `Unknown`): usado em testes e para
    /// varrer o inventário contra o manifesto.
    pub const fn catalog() -> &'static [IconId] {
        &[
            IconId::Run,
            IconId::Debug,
            IconId::Stop,
            IconId::Pause,
            IconId::Settings,
            IconId::Close,
            IconId::Warning,
            IconId::Error,
            IconId::Info,
            IconId::Project,
            IconId::Folder,
            IconId::File,
            IconId::GitBranch,
            IconId::Search,
            IconId::Bookmark,
            IconId::Breakpoint,
            IconId::ChevronRight,
            IconId::ChevronDown,
            IconId::SplitRight,
            IconId::SplitDown,
            IconId::Switcher,
            IconId::Eye,
            IconId::Blame,
            IconId::Coverage,
            IconId::Hint,
        ]
    }
}

/// SVG bruto de um id (embed; fallback nunca falha).
pub fn svg(id: IconId) -> &'static str {
    match id {
        IconId::Run => include_str!("../assets/run.svg"),
        IconId::Debug => include_str!("../assets/debug.svg"),
        IconId::Stop => include_str!("../assets/stop.svg"),
        IconId::Pause => include_str!("../assets/pause.svg"),
        IconId::Settings => include_str!("../assets/settings.svg"),
        IconId::Close => include_str!("../assets/close.svg"),
        IconId::Warning => include_str!("../assets/warning.svg"),
        IconId::Error => include_str!("../assets/error.svg"),
        IconId::Info => include_str!("../assets/info.svg"),
        IconId::Project => include_str!("../assets/project.svg"),
        IconId::Folder => include_str!("../assets/folder.svg"),
        IconId::File => include_str!("../assets/file.svg"),
        IconId::GitBranch => include_str!("../assets/git-branch.svg"),
        IconId::Search => include_str!("../assets/search.svg"),
        IconId::Bookmark => include_str!("../assets/bookmark.svg"),
        IconId::Breakpoint => include_str!("../assets/breakpoint.svg"),
        IconId::ChevronRight => include_str!("../assets/chevron-right.svg"),
        IconId::ChevronDown => include_str!("../assets/chevron-down.svg"),
        IconId::SplitRight => include_str!("../assets/split-right.svg"),
        IconId::SplitDown => include_str!("../assets/split-down.svg"),
        IconId::Switcher => include_str!("../assets/switcher.svg"),
        IconId::Eye => include_str!("../assets/eye.svg"),
        IconId::Blame => include_str!("../assets/blame.svg"),
        IconId::Coverage => include_str!("../assets/coverage.svg"),
        IconId::Hint => include_str!("../assets/hint.svg"),
        IconId::Unknown => FALLBACK_SVG,
    }
}

/// Fallback embutido: círculo com ponto — neutro, sempre disponível.
const FALLBACK_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\"><circle cx=\"12\" cy=\"12\" r=\"7\"/><circle cx=\"12\" cy=\"12\" r=\"1.6\" fill=\"currentColor\" stroke=\"none\"/></svg>";

/// Cor em `#RRGGBB` para injetar no `currentColor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// Tamanhos canônicos do catálogo (UI-ICON-001-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IconSize {
    S16 = 16,
    S20 = 20,
    S24 = 24,
}

impl IconSize {
    pub const fn px(self) -> f32 {
        self as u16 as f32
    }
}

/// SVG já resolvido para (tint, tamanho lógico, HiDPI).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedIcon {
    /// SVG com a cor injetada e largura/altura ajustadas.
    pub svg: String,
    /// Lado em **pontos de tela** (tamanho lógico × pixels_per_point).
    pub side_points: f32,
}

/// Registry com cache global: `(IconId, Rgb, IconSize, ppp × 100) → svg`.
#[derive(Default)]
pub struct Registry {
    cache: Mutex<HashMap<(IconId, Rgb, IconSize, u32), ResolvedIcon>>,
}

static GLOBAL: OnceLock<Registry> = OnceLock::new();

/// Registry compartilhado (UI referencia significados, não arquivos).
pub fn global() -> &'static Registry {
    GLOBAL.get_or_init(Registry::default)
}

impl Registry {
    /// Resolve o ícone com tint, tamanho e HiDPI. `Unknown` (ou id futuro
    /// sem asset) cai no fallback — nunca panica, nunca falha.
    pub fn resolve(
        &self,
        id: IconId,
        tint: Rgb,
        size: IconSize,
        pixels_per_point: f32,
    ) -> ResolvedIcon {
        let ppp_f = pixels_per_point.max(0.5);
        let ppp = (ppp_f * 100.0) as u32;
        let key = (id, tint, size, ppp);
        if let Some(hit) = self.cache.lock().unwrap().get(&key) {
            return hit.clone();
        }
        let resolved = ResolvedIcon {
            svg: tint_svg(svg(id), &tint.hex(), (size.px() * ppp_f).round()),
            side_points: size.px() * ppp_f,
        };
        self.cache.lock().unwrap().insert(key, resolved.clone());
        resolved
    }

    /// Estatística do cache (testes/observabilidade).
    pub fn cached_entries(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

/// Substitui `currentColor` pela cor e ajusta width/height do `<svg>` para
/// o lado **físico** (pontos × pixels_per_point). Renderizadores que
/// respeitam width/height usam o valor físico; viewBox preserva o desenho
/// 24×24 em qualquer escala.
fn tint_svg(svg: &str, color: &str, physical_side: f32) -> String {
    let with_color = svg.replace("currentColor", color);
    let side = format!("{physical_side}");
    with_color.replacen(
        "<svg ",
        &format!("<svg width=\"{side}\" height=\"{side}\" "),
        1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_assets_and_manifest() {
        // Todo id semântico resolve para SVG não vazio e bem formado.
        for id in IconId::catalog() {
            let s = svg(*id);
            assert!(s.starts_with("<svg"), "{} sem tag svg", id.slug());
            assert!(s.contains("</svg>"), "{} sem fechamento", id.slug());
            assert!(
                s.contains("viewBox=\"0 0 24 24\""),
                "{} sem viewBox",
                id.slug()
            );
        }
        // Manifesto cobre o catálogo inteiro (UI-ICON-001-02): nome, origem,
        // autor, URL e licença por entrada.
        let manifest = include_str!("../assets/manifest.json");
        for id in IconId::catalog() {
            assert!(
                manifest.contains(&format!("\"name\": \"{}\"", id.slug())),
                "manifest sem {}",
                id.slug()
            );
        }
        for campo in ["\"origin\"", "\"author\"", "\"url\"", "\"license\""] {
            assert!(manifest.contains(campo), "manifest sem campo {campo}");
        }
    }

    #[test]
    fn unknown_falls_back_and_cache_hits() {
        let reg = Registry::default();
        let a = reg.resolve(IconId::Unknown, Rgb(255, 0, 0), IconSize::S16, 1.0);
        assert!(
            a.svg.contains("#ff0000"),
            "tint aplicada no fallback: {}",
            a.svg
        );
        assert!(a.svg.contains("<circle"), "fallback é o círculo neutro");
        let b = reg.resolve(IconId::Run, Rgb(255, 0, 0), IconSize::S16, 1.0);
        assert_ne!(a.svg, b.svg, "run ≠ fallback");
        // Cache: mesma chave devolve entrada sem crescer o mapa.
        let before = reg.cached_entries();
        let _ = reg.resolve(IconId::Run, Rgb(255, 0, 0), IconSize::S16, 1.0);
        assert_eq!(reg.cached_entries(), before);
        let _ = reg.resolve(IconId::Run, Rgb(0, 255, 0), IconSize::S16, 1.0);
        assert_eq!(reg.cached_entries(), before + 1);
    }

    #[test]
    fn sizes_hidpi_and_tint() {
        let reg = Registry::default();
        let s16 = reg.resolve(IconId::Settings, Rgb(10, 20, 30), IconSize::S16, 1.0);
        assert_eq!(s16.side_points, 16.0);
        let s16_retina = reg.resolve(IconId::Settings, Rgb(10, 20, 30), IconSize::S16, 2.0);
        assert_eq!(
            s16_retina.side_points, 32.0,
            "HiDPI dobra os pontos físicos"
        );
        assert_ne!(s16.svg, s16_retina.svg);
        assert!(s16.svg.contains("#0a141e"), "tint injetada: {}", s16.svg);
        assert!(
            s16.svg.contains("width=\"16\""),
            "lado físico 1x: {}",
            s16.svg
        );
        let s24 = reg.resolve(IconId::Settings, Rgb(10, 20, 30), IconSize::S24, 1.0);
        assert_eq!(s24.side_points, 24.0);
    }

    #[test]
    fn accessible_names_present() {
        for id in IconId::catalog() {
            assert!(
                !id.accessible_name().is_empty(),
                "{} sem nome acessível",
                id.slug()
            );
        }
        assert_eq!(IconId::GitBranch.accessible_name(), "Branch do Git");
    }
}
