//! `Mapper/Visualizations` (furniture): `Visualization`, `Layer`, `Animation`,
//! `AnimationLayer`, `FrameSequence`, `Frame`, `Color`, `ColorLayer` and
//! `VisualizationsMapper`, with the json System.Text.Json writes for them.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use indexmap::IndexMap;

use crate::net::{try_parse_f64, try_parse_int};
use crate::stj::{Obj, J};
use crate::xml::{attr, element, elements, Node};

fn int_j(v: Option<i32>) -> Option<J> {
    v.map(|v| J::int(v as i64))
}

fn int_map<T>(map: &IndexMap<i32, T>, f: impl Fn(&T) -> J) -> J {
    J::Obj(map.iter().map(|(k, v)| (k.to_string(), f(v))).collect())
}

fn sorted_map<T>(map: &BTreeMap<i32, T>, f: impl Fn(&T) -> J) -> J {
    J::Obj(map.iter().map(|(k, v)| (k.to_string(), f(v))).collect())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Visualization {
    pub angle: i32,
    pub layer_count: i32,
    pub size: i32,
    pub layers: IndexMap<i32, Layer>,
    /// `Dictionary<int, object>` holding `new { layers }`.
    pub directions: IndexMap<i32, IndexMap<i32, Layer>>,
    /// `null` when empty.
    pub animations: Option<IndexMap<i32, Animation>>,
    pub colors: BTreeMap<i32, Color>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub ink: Option<String>,
    pub alpha: Option<i32>,
    pub z: Option<f64>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub tag: Option<String>,
    pub ignore_mouse: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub id: i32,
    pub transition_to: Option<i32>,
    pub transition_from: Option<i32>,
    pub immediate_change_from: Option<String>,
    pub random_start: Option<bool>,
    pub layers: IndexMap<i32, AnimationLayer>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimationLayer {
    pub frame_repeat: Option<i32>,
    pub loop_count: Option<i32>,
    pub random: Option<i32>,
    /// `null` when empty.
    pub frame_sequences: Option<IndexMap<i32, FrameSequence>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameSequence {
    pub loop_count: Option<i32>,
    pub random: Option<i32>,
    pub frames: IndexMap<i32, Frame>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub id: i32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub random_x: Option<i32>,
    pub random_y: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Color {
    pub id: i32,
    pub layers: IndexMap<i32, ColorLayer>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorLayer {
    pub id: i32,
    pub color: i32,
}

impl Layer {
    pub fn from_xml(xml: Node) -> Layer {
        Layer {
            ink: attr(xml, "ink").map(str::to_string),
            alpha: try_parse_int(attr(xml, "alpha")),
            z: try_parse_f64(attr(xml, "z")),
            x: try_parse_int(attr(xml, "x")),
            y: try_parse_int(attr(xml, "y")),
            tag: attr(xml, "tag").map(str::to_string),
            ignore_mouse: (attr(xml, "ignoreMouse") == Some("1")).then_some(true),
        }
    }

    pub fn to_json(&self) -> J {
        Obj::new()
            .opt("ink", self.ink.as_ref().map(|s| J::str(s.clone())))
            .opt("alpha", int_j(self.alpha))
            .opt("z", self.z.map(J::f64))
            .opt("x", int_j(self.x))
            .opt("y", int_j(self.y))
            .opt("tag", self.tag.as_ref().map(|s| J::str(s.clone())))
            .opt("ignoreMouse", self.ignore_mouse.map(J::Bool))
            .build()
    }
}

impl Frame {
    pub fn from_xml(xml: Node) -> Frame {
        Frame {
            id: try_parse_int(attr(xml, "id")).unwrap_or(0),
            x: try_parse_int(attr(xml, "x")),
            y: try_parse_int(attr(xml, "y")),
            random_x: try_parse_int(attr(xml, "randomX")),
            random_y: try_parse_int(attr(xml, "randomY")),
        }
    }

    /// `FrameConverter.Write`.
    pub fn to_json(&self) -> J {
        Obj::new()
            .put("id", J::int(self.id as i64))
            .opt("x", int_j(self.x))
            .opt("y", int_j(self.y))
            .opt("randomX", int_j(self.random_x))
            .opt("randomY", int_j(self.random_y))
            .build()
    }
}

impl FrameSequence {
    pub fn from_xml(xml: Node) -> FrameSequence {
        FrameSequence {
            loop_count: try_parse_int(attr(xml, "loopCount")),
            random: try_parse_int(attr(xml, "random")),
            frames: elements(xml, "frame").enumerate().map(|(i, f)| (i as i32, Frame::from_xml(f))).collect(),
        }
    }

    pub fn to_json(&self) -> J {
        Obj::new()
            .opt("loopCount", int_j(self.loop_count))
            .opt("random", int_j(self.random))
            .put("frames", int_map(&self.frames, Frame::to_json))
            .build()
    }
}

impl AnimationLayer {
    pub fn from_xml(xml: Node) -> AnimationLayer {
        let sequences: IndexMap<i32, FrameSequence> =
            elements(xml, "frameSequence").enumerate().map(|(i, s)| (i as i32, FrameSequence::from_xml(s))).collect();
        AnimationLayer {
            frame_repeat: try_parse_int(attr(xml, "frameRepeat")),
            loop_count: try_parse_int(attr(xml, "loopCount")),
            random: try_parse_int(attr(xml, "random")),
            frame_sequences: (!sequences.is_empty()).then_some(sequences),
        }
    }

    pub fn to_json_with(&self, sequence: impl Fn(&FrameSequence) -> J) -> J {
        Obj::new()
            .opt("frameRepeat", int_j(self.frame_repeat))
            .opt("loopCount", int_j(self.loop_count))
            .opt("random", int_j(self.random))
            .opt("frameSequences", self.frame_sequences.as_ref().map(|s| int_map(s, &sequence)))
            .build()
    }

    pub fn to_json(&self) -> J {
        self.to_json_with(FrameSequence::to_json)
    }
}

impl Animation {
    pub fn from_xml(xml: Node) -> Animation {
        let mut layers = IndexMap::new();
        for layer in elements(xml, "animationLayer") {
            if let Some(id) = try_parse_int(attr(layer, "id")) {
                layers.insert(id, AnimationLayer::from_xml(layer));
            }
        }
        Animation {
            id: try_parse_int(attr(xml, "id")).unwrap_or(0),
            transition_to: try_parse_int(attr(xml, "transitionTo")),
            transition_from: try_parse_int(attr(xml, "transitionFrom")),
            immediate_change_from: attr(xml, "immediateChangeFrom").map(str::to_string),
            random_start: (attr(xml, "randomStart") == Some("1")).then_some(true),
            layers,
        }
    }

    pub fn to_json(&self) -> J {
        Obj::new()
            .opt("transitionTo", int_j(self.transition_to))
            .opt("transitionFrom", int_j(self.transition_from))
            .opt("immediateChangeFrom", self.immediate_change_from.as_ref().map(|s| J::str(s.clone())))
            .opt("randomStart", self.random_start.map(J::Bool))
            .put("layers", int_map(&self.layers, AnimationLayer::to_json))
            .build()
    }
}

/// `Convert.ToInt32(hex, 16)`, with the .NET error messages.
pub fn convert_hex_to_int32(value: &str) -> Result<i32> {
    let mut rest = value;
    if let Some(r) = rest.strip_prefix('-') {
        let _ = r;
        bail!("String cannot contain a minus sign if the base is not 10.");
    }
    if let Some(r) = rest.strip_prefix('+') {
        rest = r;
    }
    if rest.len() >= 2 && (rest.starts_with("0x") || rest.starts_with("0X")) {
        rest = &rest[2..];
    }
    let digits: usize = rest.chars().take_while(|c| c.is_ascii_hexdigit()).count();
    if digits == 0 {
        bail!("Could not find any recognizable digits.");
    }
    let mut acc: u64 = 0;
    for c in rest[..digits].chars() {
        acc = acc * 16 + c.to_digit(16).unwrap() as u64;
        if acc > u32::MAX as u64 {
            bail!("Value was either too large or too small for a UInt32.");
        }
    }
    if digits < rest.len() {
        bail!("Additional non-parsable characters are at the end of the string.");
    }
    Ok(acc as u32 as i32)
}

impl ColorLayer {
    pub fn from_xml(xml: Node) -> Result<ColorLayer> {
        let mut color = 0;
        if let Some(hex) = attr(xml, "color").filter(|h| !h.is_empty()) {
            color = convert_hex_to_int32(hex)?;
        }
        Ok(ColorLayer { id: try_parse_int(attr(xml, "id")).unwrap_or(0), color })
    }

    pub fn to_json(&self) -> J {
        Obj::new().put("color", J::int(self.color as i64)).build()
    }
}

impl Color {
    pub fn from_xml(xml: Node) -> Result<Color> {
        let mut layers = IndexMap::new();
        for layer in elements(xml, "colorLayer") {
            if let Some(id) = try_parse_int(attr(layer, "id")) {
                layers.insert(id, ColorLayer::from_xml(layer)?);
            }
        }
        Ok(Color { id: try_parse_int(attr(xml, "id")).unwrap_or(0), layers })
    }

    pub fn to_json(&self) -> J {
        Obj::new().put("layers", int_map(&self.layers, ColorLayer::to_json)).build()
    }
}

/// `layers` of a `<layers>` / `<direction>` element, keyed by parsed id.
pub(crate) fn parse_layers<T>(parent: Option<Node>, make: impl Fn(Node) -> T) -> IndexMap<i32, T> {
    let mut layers = IndexMap::new();
    if let Some(parent) = parent {
        for layer in elements(parent, "layer") {
            if let Some(id) = try_parse_int(attr(layer, "id")) {
                layers.insert(id, make(layer));
            }
        }
    }
    layers
}

impl Visualization {
    pub fn from_xml(xml: Node) -> Result<Visualization> {
        let mut directions = IndexMap::new();
        if let Some(element) = element(xml, "directions") {
            for direction in elements(element, "direction") {
                if let Some(id) = try_parse_int(attr(direction, "id")) {
                    directions.insert(id, parse_layers(Some(direction), Layer::from_xml));
                }
            }
        }

        let mut animations = IndexMap::new();
        if let Some(element) = element(xml, "animations") {
            for animation in elements(element, "animation") {
                if let Some(id) = try_parse_int(attr(animation, "id")) {
                    animations.insert(id, Animation::from_xml(animation));
                }
            }
        }

        let mut colors = BTreeMap::new();
        if let Some(element) = element(xml, "colors") {
            for color in elements(element, "color") {
                if let Some(id) = try_parse_int(attr(color, "id")) {
                    colors.insert(id, Color::from_xml(color)?);
                }
            }
        }

        Ok(Visualization {
            size: try_parse_int(attr(xml, "size")).unwrap_or(0),
            layer_count: try_parse_int(attr(xml, "layerCount")).unwrap_or(0),
            angle: try_parse_int(attr(xml, "angle")).unwrap_or(0),
            layers: parse_layers(element(xml, "layers"), Layer::from_xml),
            directions,
            animations: (!animations.is_empty()).then_some(animations),
            colors,
        })
    }

    pub fn to_json(&self) -> J {
        Obj::new()
            .put("angle", J::int(self.angle as i64))
            .put("layerCount", J::int(self.layer_count as i64))
            .put("size", J::int(self.size as i64))
            .put("layers", int_map(&self.layers, Layer::to_json))
            .put(
                "directions",
                int_map(&self.directions, |layers| Obj::new().put("layers", int_map(layers, Layer::to_json)).build()),
            )
            .opt("animations", self.animations.as_ref().map(|a| int_map(a, Animation::to_json)))
            .put("colors", sorted_map(&self.colors, Color::to_json))
            .build()
    }
}

/// `VisualizationsMapper.MapVisualizationsXml`: every `graphics/visualization`, then every
/// top-level `visualization` (no size is excluded).
pub fn map_visualizations_xml(root: Node) -> Result<Vec<Visualization>> {
    let mut visualizations = Vec::new();
    for graphics in elements(root, "graphics") {
        for visualization in elements(graphics, "visualization") {
            visualizations.push(Visualization::from_xml(visualization)?);
        }
    }
    for visualization in elements(root, "visualization") {
        visualizations.push(Visualization::from_xml(visualization)?);
    }
    Ok(visualizations)
}

/// Parses `*_visualization.bin` text.
pub fn parse_visualizations_text(text: &str) -> Result<Vec<Visualization>> {
    let doc = crate::xml::parse(text)?;
    map_visualizations_xml(doc.root_element())
}

pub fn visualizations_json(list: &[Visualization]) -> J {
    J::Arr(list.iter().map(Visualization::to_json).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nl(s: &str) -> String {
        s.replace('\n', habbo_core::paths::NEWLINE)
    }

    #[test]
    fn maps_visualization_xml_to_json() {
        let xml = r#"<visualizationData type="chair">
  <graphics>
    <visualization size="64" layerCount="2" angle="45">
      <layers><layer id="1" z="-0.5" alpha="128" ink="ADD"/><layer id="0" ignoreMouse="1" tag="t" x="3"/></layers>
      <directions><direction id="2"><layer id="0" z="10"/></direction><direction id="0"/></directions>
      <colors><color id="2"><colorLayer id="0" color="FFFFFF"/></color><color id="1"><colorLayer id="1" color="ff"/></color></colors>
      <animations>
        <animation id="0" transitionTo="1" randomStart="1">
          <animationLayer id="0" frameRepeat="2">
            <frameSequence loopCount="3"><frame id="1" x="2" randomY="4"/><frame id="5"/></frameSequence>
          </animationLayer>
          <animationLayer id="1"/>
        </animation>
      </animations>
    </visualization>
  </graphics>
  <visualization size="1" layerCount="0" angle="45"/>
</visualizationData>"#;
        let list = parse_visualizations_text(xml).unwrap();
        assert_eq!(list.len(), 2);
        let json = visualizations_json(&list).to_string_indented().unwrap();
        let expected = r#"[
  {
    "angle": 45,
    "layerCount": 2,
    "size": 64,
    "layers": {
      "1": {
        "ink": "ADD",
        "alpha": 128,
        "z": -0.5
      },
      "0": {
        "x": 3,
        "tag": "t",
        "ignoreMouse": true
      }
    },
    "directions": {
      "2": {
        "layers": {
          "0": {
            "z": 10
          }
        }
      },
      "0": {
        "layers": {}
      }
    },
    "animations": {
      "0": {
        "transitionTo": 1,
        "randomStart": true,
        "layers": {
          "0": {
            "frameRepeat": 2,
            "frameSequences": {
              "0": {
                "loopCount": 3,
                "frames": {
                  "0": {
                    "id": 1,
                    "x": 2,
                    "randomY": 4
                  },
                  "1": {
                    "id": 5
                  }
                }
              }
            }
          },
          "1": {}
        }
      }
    },
    "colors": {
      "1": {
        "layers": {
          "1": {
            "color": 255
          }
        }
      },
      "2": {
        "layers": {
          "0": {
            "color": 16777215
          }
        }
      }
    }
  },
  {
    "angle": 45,
    "layerCount": 0,
    "size": 1,
    "layers": {},
    "directions": {},
    "colors": {}
  }
]"#;
        assert_eq!(json, nl(expected));
    }

    #[test]
    fn hex_colors_like_convert_to_int32() {
        assert_eq!(convert_hex_to_int32("FFFFFFFF").unwrap(), -1);
        assert_eq!(convert_hex_to_int32("0x10").unwrap(), 16);
        assert!(convert_hex_to_int32("#fff").is_err());
        assert!(convert_hex_to_int32("1FFFFFFFF").is_err());
        let err = parse_visualizations_text(
            r#"<v><graphics><visualization size="1"><colors><color id="0"><colorLayer id="0" color="zz"/></color></colors></visualization></graphics></v>"#,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "Could not find any recognizable digits.");
    }
}
