//! Icônes de TextEdit, tracées au pinceau : types de fichiers de l'explorateur et des
//! onglets, boutons de la barre de titre, de la barre d'activité et de l'aperçu.

use std::{f32::consts::TAU, path::Path};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, pos2, vec2};

use crate::style;

/// Famille d'un fichier, pour son icône et le nom de son langage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Folder,
    Markdown,
    React,
    TypeScript,
    JavaScript,
    Json,
    Rust,
    Css,
    Html,
    Shell,
    Python,
    Toml,
    Yaml,
    Image,
    Text,
}

impl FileKind {
    pub fn of(path: &Path) -> Self {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match extension.as_str() {
            "md" | "markdown" => Self::Markdown,
            "tsx" | "jsx" => Self::React,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "js" | "mjs" | "cjs" => Self::JavaScript,
            "json" | "jsonc" => Self::Json,
            "rs" => Self::Rust,
            "css" | "scss" | "less" => Self::Css,
            "html" | "htm" | "xml" | "svg" => Self::Html,
            "sh" | "bash" | "zsh" | "fish" => Self::Shell,
            "py" => Self::Python,
            "toml" | "ini" | "conf" | "cfg" => Self::Toml,
            "yml" | "yaml" => Self::Yaml,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => Self::Image,
            _ => Self::Text,
        }
    }

    /// Nom affiché dans la barre d'état.
    pub fn language(self) -> &'static str {
        match self {
            Self::Folder => "Dossier",
            Self::Markdown => "Markdown",
            Self::React => "TypeScript React",
            Self::TypeScript => "TypeScript",
            Self::JavaScript => "JavaScript",
            Self::Json => "JSON",
            Self::Rust => "Rust",
            Self::Css => "CSS",
            Self::Html => "HTML",
            Self::Shell => "Shell",
            Self::Python => "Python",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
            Self::Image => "Image",
            Self::Text => "Texte brut",
        }
    }
}

/// Icône de fichier dans un carré `rect` (16 à 18 points de côté).
pub fn paint_file(painter: &egui::Painter, rect: Rect, kind: FileKind) {
    let badge = |fill: Color32, ink: Color32, text: &str| {
        let square = Rect::from_center_size(rect.center(), vec2(15.0, 15.0));
        painter.rect_filled(square, 3.5, fill);
        let size = if text.chars().count() > 2 { 6.5 } else { 8.0 };
        painter.text(
            square.center() + vec2(0.0, 0.5),
            Align2::CENTER_CENTER,
            text,
            FontId::new(size, style::strong_family()),
            ink,
        );
    };
    match kind {
        FileKind::Folder => paint_folder(painter, rect, Color32::from_rgb(0x5B, 0x9B, 0xF8)),
        FileKind::Markdown => badge(Color32::from_rgb(0x3B, 0x76, 0xF0), Color32::WHITE, "MD"),
        FileKind::TypeScript => badge(Color32::from_rgb(0x31, 0x78, 0xC6), Color32::WHITE, "TS"),
        FileKind::JavaScript => badge(
            Color32::from_rgb(0xF0, 0xD5, 0x3A),
            Color32::from_gray(0x30),
            "JS",
        ),
        FileKind::Rust => badge(Color32::from_rgb(0xDE, 0x8A, 0x5A), Color32::WHITE, "RS"),
        FileKind::Css => badge(Color32::from_rgb(0x7B, 0x61, 0xD9), Color32::WHITE, "#"),
        FileKind::Html => badge(Color32::from_rgb(0xE4, 0x5A, 0x2E), Color32::WHITE, "<>"),
        FileKind::Shell => badge(Color32::from_rgb(0x3C, 0x41, 0x4A), Color32::WHITE, ">_"),
        FileKind::Python => badge(Color32::from_rgb(0x35, 0x72, 0xA5), Color32::WHITE, "py"),
        FileKind::Toml | FileKind::Yaml => paint_document(painter, rect, Color32::from_rgb(0x9A, 0x6A, 0x3C)),
        FileKind::Json => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "{ }",
                FontId::new(12.0, style::strong_family()),
                Color32::from_rgb(0xE0, 0x9E, 0x12),
            );
        }
        FileKind::React => {
            let color = Color32::from_rgb(0x2F, 0x8C, 0xF0);
            let stroke = Stroke::new(1.1, color);
            for angle in [0.0, TAU / 6.0, -TAU / 6.0] {
                painter.add(ellipse(rect.center(), vec2(7.5, 2.9), angle, stroke));
            }
            painter.circle_filled(rect.center(), 1.6, color);
        }
        FileKind::Image => {
            let frame = Rect::from_center_size(rect.center(), vec2(15.0, 12.0));
            let green = Color32::from_rgb(0x2E, 0xA0, 0x6B);
            painter.rect_stroke(frame, 2.5, Stroke::new(1.4, green), egui::StrokeKind::Inside);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(frame.left() + 2.5, frame.bottom() - 2.5),
                    pos2(frame.left() + 6.5, frame.top() + 5.0),
                    pos2(frame.left() + 10.5, frame.bottom() - 2.5),
                ],
                green,
                Stroke::NONE,
            ));
            painter.circle_filled(pos2(frame.right() - 4.0, frame.top() + 4.0), 1.5, green);
        }
        FileKind::Text => paint_document(painter, rect, Color32::from_rgb(0x6B, 0x72, 0x80)),
    }
}

/// Dossier plein, onglet en haut à gauche.
fn paint_folder(painter: &egui::Painter, rect: Rect, color: Color32) {
    let body = Rect::from_min_max(
        pos2(rect.center().x - 8.0, rect.center().y - 4.5),
        pos2(rect.center().x + 8.0, rect.center().y + 6.0),
    );
    let tab = Rect::from_min_size(body.left_top() - vec2(0.0, 2.0), vec2(7.0, 4.0));
    painter.rect_filled(tab, 1.5, color.gamma_multiply(0.85));
    painter.rect_filled(body, 2.5, color);
    painter.hline(
        body.left() + 1.0..=body.right() - 1.0,
        body.top() + 2.5,
        Stroke::new(1.0, Color32::from_white_alpha(70)),
    );
}

/// Feuille au coin replié, avec deux lignes de texte.
fn paint_document(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.4, color);
    let (left, right, top, bottom) = (c.x - 5.5, c.x + 5.5, c.y - 7.0, c.y + 7.0);
    painter.add(egui::Shape::closed_line(
        vec![
            pos2(left, top),
            pos2(right - 3.5, top),
            pos2(right, top + 3.5),
            pos2(right, bottom),
            pos2(left, bottom),
        ],
        stroke,
    ));
    painter.line(
        vec![
            pos2(right - 3.5, top),
            pos2(right - 3.5, top + 3.5),
            pos2(right, top + 3.5),
        ],
        stroke,
    );
    for y in [c.y + 0.5, c.y + 3.5] {
        painter.hline(left + 2.5..=right - 2.5, y, Stroke::new(1.1, color));
    }
}

/// Ellipse de demi-axes `radius`, tournée de `angle`.
fn ellipse(center: Pos2, radius: egui::Vec2, angle: f32, stroke: Stroke) -> egui::Shape {
    let (sin, cos) = angle.sin_cos();
    let points = (0..32)
        .map(|step| {
            let t = step as f32 / 32.0 * TAU;
            let (x, y) = (radius.x * t.cos(), radius.y * t.sin());
            pos2(center.x + x * cos - y * sin, center.y + x * sin + y * cos)
        })
        .collect();
    egui::Shape::closed_line(points, stroke)
}

/// Symboles des boutons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Files,
    Search,
    Settings,
    Sidebar,
    Split,
    Save,
    Eye,
    Info,
    Expand,
    Collapse,
    ChevronRight,
    ChevronDown,
    FolderOutline,
    Plus,
    Close,
    Up,
    Down,
    /// Flèche vers le haut avec sa tige : dossier parent.
    Parent,
    /// Presse-papier : planchette et pince, pour « Copier ».
    Copy,
    /// Coche : action réussie (« Copié »).
    Check,
}

/// Symbole `icon` centré dans `rect`, au trait de couleur `color`.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.6, color);
    let p = |x: f32, y: f32| pos2(c.x + x, c.y + y);
    match icon {
        Icon::Files => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-6.5, -9.0),
                    p(2.0, -9.0),
                    p(7.0, -4.0),
                    p(7.0, 9.0),
                    p(-6.5, 9.0),
                ],
                stroke,
            ));
            painter.line(vec![p(2.0, -9.0), p(2.0, -4.0), p(7.0, -4.0)], stroke);
            painter.hline(c.x - 3.0..=c.x + 3.5, c.y + 1.0, stroke);
            painter.hline(c.x - 3.0..=c.x + 3.5, c.y + 4.5, stroke);
        }
        Icon::Search => {
            painter.circle_stroke(p(-1.5, -1.5), 5.5, stroke);
            painter.line_segment([p(2.6, 2.6), p(7.0, 7.0)], Stroke::new(1.9, color));
        }
        Icon::Settings => {
            for tooth in 0..8 {
                let angle = tooth as f32 / 8.0 * TAU;
                let (sin, cos) = angle.sin_cos();
                painter.line_segment(
                    [p(cos * 5.5, sin * 5.5), p(cos * 8.0, sin * 8.0)],
                    Stroke::new(2.6, color),
                );
            }
            painter.circle_stroke(c, 5.6, stroke);
            painter.circle_stroke(c, 2.4, stroke);
        }
        Icon::Sidebar | Icon::Split => {
            let frame = Rect::from_center_size(c, vec2(18.0, 15.0));
            painter.rect_stroke(frame, 3.0, stroke, egui::StrokeKind::Inside);
            let x = if icon == Icon::Sidebar {
                frame.left() + 6.5
            } else {
                c.x
            };
            painter.vline(x, frame.top() + 1.0..=frame.bottom() - 1.0, stroke);
            if icon == Icon::Sidebar {
                let side =
                    Rect::from_min_max(frame.left_top() + vec2(1.5, 1.5), pos2(x, frame.bottom() - 1.5));
                painter.rect_filled(side, 1.5, color.gamma_multiply(0.25));
            }
        }
        Icon::Save => {
            let frame = Rect::from_center_size(c, vec2(16.0, 16.0));
            painter.rect_stroke(frame, 3.0, stroke, egui::StrokeKind::Inside);
            painter.rect_stroke(
                Rect::from_min_max(p(-4.0, -8.0), p(3.0, -3.0)),
                1.0,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                Rect::from_min_max(p(-5.0, 1.0), p(5.0, 8.0)),
                1.0,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
        }
        Icon::Eye => {
            let points: Vec<Pos2> = (0..=24)
                .map(|step| {
                    let t = step as f32 / 24.0;
                    let x = -8.0 + 16.0 * t;
                    p(x, -5.5 * (1.0 - (x / 8.0).powi(2)))
                })
                .chain((1..24).map(|step| {
                    let t = step as f32 / 24.0;
                    let x = 8.0 - 16.0 * t;
                    p(x, 5.5 * (1.0 - (x / 8.0).powi(2)))
                }))
                .collect();
            painter.add(egui::Shape::closed_line(points, stroke));
            painter.circle_filled(c, 2.6, color);
        }
        Icon::Info => {
            painter.circle_stroke(c, 7.0, Stroke::new(1.4, color));
            painter.circle_filled(p(0.0, -3.2), 1.1, color);
            painter.vline(c.x, c.y - 0.8..=c.y + 3.8, Stroke::new(1.5, color));
        }
        Icon::Expand | Icon::Collapse => {
            // Quatre coins, tournés vers l'extérieur (agrandir) ou vers l'intérieur.
            let (near, far) = if icon == Icon::Expand {
                (3.0, 7.0)
            } else {
                (7.0, 3.0)
            };
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let corner = p(sx * far, sy * far);
                painter.line(
                    vec![
                        pos2(corner.x, c.y + sy * near),
                        corner,
                        pos2(c.x + sx * near, corner.y),
                    ],
                    stroke,
                );
            }
        }
        Icon::ChevronRight => {
            painter.line(vec![p(-2.0, -4.0), p(2.0, 0.0), p(-2.0, 4.0)], stroke);
        }
        Icon::ChevronDown => {
            painter.line(vec![p(-4.0, -2.0), p(0.0, 2.0), p(4.0, -2.0)], stroke);
        }
        Icon::FolderOutline => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-8.0, -6.0),
                    p(-3.0, -6.0),
                    p(-1.0, -4.0),
                    p(8.0, -4.0),
                    p(8.0, 6.0),
                    p(-8.0, 6.0),
                ],
                Stroke::new(1.4, color),
            ));
            painter.hline(c.x - 8.0..=c.x + 8.0, c.y - 1.0, Stroke::new(1.2, color));
        }
        Icon::Plus => {
            painter.hline(c.x - 6.0..=c.x + 6.0, c.y, stroke);
            painter.vline(c.x, c.y - 6.0..=c.y + 6.0, stroke);
        }
        Icon::Close => {
            painter.line_segment([p(-4.0, -4.0), p(4.0, 4.0)], stroke);
            painter.line_segment([p(4.0, -4.0), p(-4.0, 4.0)], stroke);
        }
        Icon::Up => {
            painter.line(vec![p(-5.0, 3.0), p(0.0, -2.5), p(5.0, 3.0)], stroke);
        }
        Icon::Down => {
            painter.line(vec![p(-5.0, -2.5), p(0.0, 3.0), p(5.0, -2.5)], stroke);
        }
        Icon::Parent => {
            painter.line_segment([p(0.0, -6.0), p(0.0, 6.5)], stroke);
            painter.line(vec![p(-5.0, -1.0), p(0.0, -6.0), p(5.0, -1.0)], stroke);
        }
        Icon::Copy => {
            let board = Rect::from_min_max(p(-6.0, -6.5), p(6.0, 8.0));
            painter.rect_stroke(board, 2.0, stroke, egui::StrokeKind::Middle);
            let clip = Rect::from_min_max(p(-3.0, -8.5), p(3.0, -4.5));
            painter.rect_filled(clip, 1.5, color);
            painter.hline(c.x - 3.0..=c.x + 3.0, c.y + 0.5, stroke);
            painter.hline(c.x - 3.0..=c.x + 1.5, c.y + 4.0, stroke);
        }
        Icon::Check => {
            painter.line(
                vec![p(-5.5, 0.5), p(-1.5, 4.5), p(5.5, -4.0)],
                Stroke::new(1.9, color),
            );
        }
    }
}

/// Logo de TextEdit : carré arrondi bleu, « </> » blanc.
pub fn paint_logo(painter: &egui::Painter, rect: Rect) {
    let top = style::accent().lerp_to_gamma(Color32::from_rgb(0x7C, 0x5C, 0xF6), 0.35);
    painter.rect_filled(rect, rect.width() * 0.26, top);
    let c = rect.center();
    let s = rect.width() / 28.0;
    let stroke = Stroke::new(2.0 * s, Color32::WHITE);
    let p = |x: f32, y: f32| pos2(c.x + x * s, c.y + y * s);
    painter.line(vec![p(-4.5, -5.0), p(-9.0, 0.0), p(-4.5, 5.0)], stroke);
    painter.line(vec![p(4.5, -5.0), p(9.0, 0.0), p(4.5, 5.0)], stroke);
    painter.line_segment([p(2.0, -7.0), p(-2.0, 7.0)], stroke);
}

/// Pastille « terminal » des blocs de code de l'aperçu.
pub fn paint_terminal(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 5.0, style::accent());
    let c = rect.center();
    let stroke = Stroke::new(1.5, Color32::WHITE);
    painter.line(
        vec![
            pos2(c.x - 5.0, c.y - 3.5),
            pos2(c.x - 1.5, c.y),
            pos2(c.x - 5.0, c.y + 3.5),
        ],
        stroke,
    );
    painter.hline(c.x..=c.x + 5.0, c.y + 4.0, stroke);
}
