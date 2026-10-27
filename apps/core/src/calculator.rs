//! Calculatrice : écran et pavé de touches, utilisable aussi au clavier.

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke};

use crate::style;

/// Touches du pavé, ligne par ligne.
const KEYS: [[Key; 4]; 5] = [
    [Key::Clear, Key::Text("("), Key::Text(")"), Key::Operator('÷')],
    [Key::Text("7"), Key::Text("8"), Key::Text("9"), Key::Operator('×')],
    [Key::Text("4"), Key::Text("5"), Key::Text("6"), Key::Operator('−')],
    [Key::Text("1"), Key::Text("2"), Key::Text("3"), Key::Operator('+')],
    [Key::Text("0"), Key::Text(","), Key::Erase, Key::Equals],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Text(&'static str),
    Operator(char),
    Clear,
    Erase,
    Equals,
}

impl Key {
    fn label(self) -> String {
        match self {
            Key::Text(text) => text.to_owned(),
            Key::Operator(operator) => operator.to_string(),
            Key::Clear => "C".to_owned(),
            Key::Erase => "⌫".to_owned(),
            Key::Equals => "=".to_owned(),
        }
    }
}

#[derive(Debug, Default)]
pub struct CalculatorState {
    /// Expression affichée, avec les signes typographiques (× ÷ − ,).
    expression: String,
    result: String,
    error: bool,
}

impl CalculatorState {
    fn press(&mut self, key: Key) {
        // Une nouvelle saisie après « = » repart du résultat pour un opérateur, de zéro sinon.
        if !self.result.is_empty() && !self.error {
            self.expression = match key {
                Key::Operator(_) => self.result.clone(),
                _ => String::new(),
            };
        }
        if !matches!(key, Key::Equals) {
            self.result.clear();
            self.error = false;
        }
        match key {
            Key::Text(text) => self.expression.push_str(text),
            Key::Operator(operator) => self.expression.push(operator),
            Key::Clear => self.expression.clear(),
            Key::Erase => {
                self.expression.pop();
            }
            Key::Equals => self.calculate(),
        }
    }

    fn calculate(&mut self) {
        if self.expression.is_empty() {
            return;
        }
        match calculate(&to_ascii(&self.expression)) {
            Ok(value) if value.is_finite() => {
                self.result = format_number(value);
                self.error = false;
            }
            Ok(_) => self.fail("Résultat non fini"),
            Err(error) => self.fail(&error),
        }
    }

    fn fail(&mut self, message: &str) {
        self.result = message.to_owned();
        self.error = true;
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        self.keyboard(ui);
        style::header(ui, |_| {});
        egui::CentralPanel::no_frame()
            .frame(
                Frame::new()
                    .fill(style::BACKGROUND)
                    .inner_margin(Margin::same(14)),
            )
            .show(ui, |ui| {
                self.display(ui);
                ui.add_space(12.0);
                self.keypad(ui);
            });
    }

    fn keyboard(&mut self, ui: &mut egui::Ui) {
        let events = ui.input(|input| input.events.clone());
        for event in events {
            let key = match event {
                egui::Event::Text(text) => match text.as_str() {
                    "*" | "x" => Some(Key::Operator('×')),
                    "/" => Some(Key::Operator('÷')),
                    "-" => Some(Key::Operator('−')),
                    "+" => Some(Key::Operator('+')),
                    "." | "," => Some(Key::Text(",")),
                    "=" => Some(Key::Equals),
                    "(" => Some(Key::Text("(")),
                    ")" => Some(Key::Text(")")),
                    digit => KEYS
                        .iter()
                        .flatten()
                        .find(|key| matches!(key, Key::Text(text) if *text == digit))
                        .copied(),
                },
                egui::Event::Key {
                    key, pressed: true, ..
                } => match key {
                    egui::Key::Enter => Some(Key::Equals),
                    egui::Key::Backspace => Some(Key::Erase),
                    egui::Key::Escape | egui::Key::Delete => Some(Key::Clear),
                    _ => None,
                },
                _ => None,
            };
            if let Some(key) = key {
                self.press(key);
            }
        }
    }

    fn display(&self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, style::SEPARATOR))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    let expression = if self.expression.is_empty() {
                        "0"
                    } else {
                        &self.expression
                    };
                    let (top, bottom) = if self.result.is_empty() {
                        (String::new(), expression.to_owned())
                    } else {
                        (format!("{expression} ="), self.result.clone())
                    };
                    ui.label(RichText::new(top).size(14.0).color(style::TEXT_SECONDARY));
                    let color = if self.error { style::ERROR } else { style::TEXT };
                    let size = if self.error { 18.0 } else { 32.0 };
                    ui.label(RichText::new(bottom).size(size).color(color));
                });
            });
    }

    fn keypad(&mut self, ui: &mut egui::Ui) {
        let gap = 8.0;
        let width = ((ui.available_width() - 3.0 * gap) / 4.0).floor();
        let height = ((ui.available_height() - 4.0 * gap) / 5.0).clamp(40.0, 64.0);
        ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
        for row in KEYS {
            ui.horizontal(|ui| {
                for key in row {
                    let (fill, ink) = match key {
                        Key::Equals => (style::ACCENT, Color32::WHITE),
                        Key::Operator(_) | Key::Clear | Key::Erase => {
                            (Color32::from_rgb(0xE9, 0xEA, 0xEE), style::TEXT)
                        }
                        Key::Text(_) => (Color32::WHITE, style::TEXT),
                    };
                    let button = egui::Button::new(RichText::new(key.label()).size(18.0).color(ink))
                        .fill(fill)
                        .corner_radius(CornerRadius::same(10))
                        .min_size(egui::vec2(width, height));
                    if ui.add(button).clicked() {
                        self.press(key);
                    }
                }
            });
        }
    }
}

/// Signes typographiques de l'affichage → syntaxe du calcul.
fn to_ascii(expression: &str) -> String {
    expression
        .chars()
        .map(|character| match character {
            '×' => '*',
            '÷' => '/',
            '−' => '-',
            ',' => '.',
            other => other,
        })
        .collect()
}

/// Nombre à la française, sans zéros superflus : 2,5 ; 1e21 reste en notation courte.
fn format_number(value: f64) -> String {
    let rounded = (value * 1e10).round() / 1e10;
    let text = if rounded.abs() >= 1e15 {
        format!("{rounded:e}")
    } else {
        rounded.to_string()
    };
    text.replace('.', ",").replace('-', "−")
}

pub fn calculate(expression: &str) -> Result<f64, String> {
    let mut parser = ExpressionParser {
        chars: expression.chars().filter(|ch| !ch.is_whitespace()).collect(),
        position: 0,
    };
    let value = parser.sum()?;
    if parser.position != parser.chars.len() {
        return Err("Expression invalide".into());
    }
    Ok(value)
}

struct ExpressionParser {
    chars: Vec<char>,
    position: usize,
}

impl ExpressionParser {
    fn sum(&mut self) -> Result<f64, String> {
        let mut value = self.product()?;
        while let Some(operator @ ('+' | '-')) = self.peek() {
            self.position += 1;
            let right = self.product()?;
            value = if operator == '+' {
                value + right
            } else {
                value - right
            };
        }
        Ok(value)
    }

    fn product(&mut self) -> Result<f64, String> {
        let mut value = self.atom()?;
        while let Some(operator @ ('*' | '/')) = self.peek() {
            self.position += 1;
            let right = self.atom()?;
            if operator == '/' && right == 0.0 {
                return Err("Division par zéro".into());
            }
            value = if operator == '*' {
                value * right
            } else {
                value / right
            };
        }
        Ok(value)
    }

    fn atom(&mut self) -> Result<f64, String> {
        if self.peek() == Some('-') {
            self.position += 1;
            return Ok(-self.atom()?);
        }
        if self.peek() == Some('(') {
            self.position += 1;
            let value = self.sum()?;
            if self.peek() != Some(')') {
                return Err("Parenthèse fermante manquante".into());
            }
            self.position += 1;
            return Ok(value);
        }
        let start = self.position;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit() || ch == '.') {
            self.position += 1;
        }
        if start == self.position {
            return Err("Nombre attendu".into());
        }
        self.chars[start..self.position]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|_| "Nombre invalide".into())
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorites_arithmetiques_et_parentheses() {
        assert_eq!(calculate("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(calculate("(2 + 3) * 4").unwrap(), 20.0);
        assert_eq!(calculate("-2 + 5").unwrap(), 3.0);
    }

    #[test]
    fn refuse_les_expressions_invalides() {
        assert!(calculate("2 / 0").is_err());
        assert!(calculate("1 + (2").is_err());
        assert!(calculate("1 + 2x").is_err());
    }

    #[test]
    fn le_pave_calcule_avec_les_signes_francais() {
        let mut calculator = CalculatorState::default();
        for key in [
            Key::Text("7"),
            Key::Text(","),
            Key::Text("5"),
            Key::Operator('×'),
            Key::Text("2"),
            Key::Operator('−'),
            Key::Text("1"),
            Key::Equals,
        ] {
            calculator.press(key);
        }
        assert_eq!(calculator.result, "14");
        // Un opérateur après « = » reprend le résultat.
        calculator.press(Key::Operator('÷'));
        calculator.press(Key::Text("4"));
        calculator.press(Key::Equals);
        assert_eq!(calculator.result, "3,5");
        assert_eq!(format_number(-0.1 - 0.2), "−0,3");
    }
}
