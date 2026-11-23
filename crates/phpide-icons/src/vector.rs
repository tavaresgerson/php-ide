use std::collections::HashMap;
use std::sync::OnceLock;

use crate::{svg, IconId};

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Circle(f32, f32, f32),
    Line(f32, f32, f32, f32),
    Poly(Vec<(f32, f32)>),
    Rect(f32, f32, f32, f32),
    Dot(f32, f32),
}

static PRIMS: OnceLock<HashMap<IconId, Vec<Prim>>> = OnceLock::new();

pub fn prims(id: IconId) -> &'static [Prim] {
    PRIMS
        .get_or_init(|| {
            IconId::catalog()
                .iter()
                .chain(std::iter::once(&IconId::Unknown))
                .map(|id| (*id, parse_svg(svg(*id))))
                .collect()
        })
        .get(&id)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn number(value: Option<&str>) -> Option<f32> {
    value?.parse().ok()
}

fn parse_svg(source: &str) -> Vec<Prim> {
    let Ok(document) = roxmltree::Document::parse(source) else {
        return vec![Prim::Circle(12.0, 12.0, 7.0), Prim::Dot(12.0, 12.0)];
    };
    let mut out = Vec::new();
    for node in document.descendants().filter(|node| node.is_element()) {
        match node.tag_name().name() {
            "circle" => {
                let (Some(cx), Some(cy), Some(radius)) = (
                    number(node.attribute("cx")),
                    number(node.attribute("cy")),
                    number(node.attribute("r")),
                ) else {
                    continue;
                };
                if radius <= 1.8 || node.attribute("stroke") == Some("none") {
                    out.push(Prim::Dot(cx, cy));
                } else {
                    out.push(Prim::Circle(cx, cy, radius));
                }
            }
            "line" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
                    number(node.attribute("x1")),
                    number(node.attribute("y1")),
                    number(node.attribute("x2")),
                    number(node.attribute("y2")),
                ) {
                    out.push(Prim::Line(x1, y1, x2, y2));
                }
            }
            "rect" => {
                if let (Some(x), Some(y), Some(width), Some(height)) = (
                    number(node.attribute("x")),
                    number(node.attribute("y")),
                    number(node.attribute("width")),
                    number(node.attribute("height")),
                ) {
                    out.push(Prim::Rect(x, y, width, height));
                }
            }
            "path" => {
                if let Some(path) = node.attribute("d") {
                    out.extend(parse_path(path));
                }
            }
            "polyline" | "polygon" => {
                if let Some(points) = node.attribute("points") {
                    let points = parse_numbers(points)
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| (pair[0], pair[1]))
                        .collect::<Vec<_>>();
                    if !points.is_empty() {
                        out.push(Prim::Poly(points));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn parse_numbers(source: &str) -> Vec<f32> {
    source
        .replace(',', " ")
        .split_whitespace()
        .filter_map(|value| value.parse().ok())
        .collect()
}

fn parse_path(path: &str) -> Vec<Prim> {
    let spaced = path.chars().fold(String::new(), |mut spaced, character| {
        if character.is_ascii_alphabetic() {
            spaced.push(' ');
            spaced.push(character);
            spaced.push(' ');
        } else {
            spaced.push(character);
        }
        spaced
    });
    let tokens: Vec<&str> = spaced.split_whitespace().collect();
    let mut primitives = Vec::new();
    let mut points = Vec::new();
    let mut current = (0.0, 0.0);
    let mut start = current;
    let mut index = 0;
    let next_number = |index: &mut usize| -> Option<f32> {
        let value = tokens.get(*index)?.parse().ok()?;
        *index += 1;
        Some(value)
    };
    while index < tokens.len() {
        let command = tokens[index];
        index += 1;
        match command {
            "M" | "L" => {
                let (Some(x), Some(y)) = (next_number(&mut index), next_number(&mut index)) else {
                    break;
                };
                if command == "M" {
                    if !points.is_empty() {
                        primitives.push(Prim::Poly(std::mem::take(&mut points)));
                    }
                    start = (x, y);
                }
                current = (x, y);
                points.push(current);
            }
            "H" | "h" => {
                let Some(x) = next_number(&mut index) else {
                    break;
                };
                current.0 = if command == "H" { x } else { current.0 + x };
                points.push(current);
            }
            "V" | "v" => {
                let Some(y) = next_number(&mut index) else {
                    break;
                };
                current.1 = if command == "V" { y } else { current.1 + y };
                points.push(current);
            }
            "C" | "c" => {
                let mut values = [0.0; 6];
                for value in &mut values {
                    let Some(number) = next_number(&mut index) else {
                        break;
                    };
                    *value = number;
                }
                let (control1, control2, end) = if command == "C" {
                    (
                        (values[0], values[1]),
                        (values[2], values[3]),
                        (values[4], values[5]),
                    )
                } else {
                    (
                        (current.0 + values[0], current.1 + values[1]),
                        (current.0 + values[2], current.1 + values[3]),
                        (current.0 + values[4], current.1 + values[5]),
                    )
                };
                for step in 1..=8 {
                    let t = step as f32 / 8.0;
                    let u = 1.0 - t;
                    let x = u * u * u * current.0
                        + 3.0 * u * u * t * control1.0
                        + 3.0 * u * t * t * control2.0
                        + t * t * t * end.0;
                    let y = u * u * u * current.1
                        + 3.0 * u * u * t * control1.1
                        + 3.0 * u * t * t * control2.1
                        + t * t * t * end.1;
                    points.push((x, y));
                }
                current = end;
            }
            "Z" | "z" => {
                points.push(start);
            }
            _ => {}
        }
    }
    if points.len() == 2 {
        let distance = (points[0].0 - points[1].0).abs() + (points[0].1 - points[1].1).abs();
        if distance <= 0.05 {
            primitives.push(Prim::Dot(points[0].0, points[0].1));
            return primitives;
        }
    }
    if !points.is_empty() {
        primitives.push(Prim::Poly(points));
    }
    primitives
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_has_vector_primitives_from_the_same_svg() {
        for id in IconId::catalog() {
            assert!(!prims(*id).is_empty(), "{}: {:?}", id.slug(), prims(*id));
        }
        assert!(prims(IconId::Eye)
            .iter()
            .any(|prim| matches!(prim, Prim::Poly(points) if points.len() > 10)));
        assert!(prims(IconId::Info)
            .iter()
            .any(|prim| matches!(prim, Prim::Dot(_, _))));
    }
}
