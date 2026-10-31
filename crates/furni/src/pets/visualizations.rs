//! `SWF_Pets_Compiler/Mapper/Visualizations`: the pets flavour of the visualization model
//! (sorted directions, integer z, frame offsets, postures and gestures, empty blocks left out).

use std::collections::BTreeMap;

use anyhow::Result;
use indexmap::IndexMap;

use crate::net::try_parse_int;
use crate::stj::{Obj, J};
use crate::visualizations::{parse_layers, Animation as FurniAnimation, Color};
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
pub struct Layer {
    pub ink: Option<String>,
    pub alpha: Option<i32>,
    pub z: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub tag: Option<String>,
    pub ignore_mouse: Option<bool>,
}

impl Layer {
    pub fn from_xml(xml: Node) -> Layer {
        Layer {
            ink: attr(xml, "ink").map(str::to_string),
            alpha: try_parse_int(attr(xml, "alpha")),
            z: try_parse_int(attr(xml, "z")),
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
            .opt("z", int_j(self.z))
            .opt("x", int_j(self.x))
            .opt("y", int_j(self.y))
            .opt("tag", self.tag.as_ref().map(|s| J::str(s.clone())))
            .opt("ignoreMouse", self.ignore_mouse.map(J::Bool))
            .build()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Offset {
    pub direction: i32,
    pub x: Option<i32>,
    pub y: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub id: i32,
    pub random_x: Option<i32>,
    pub random_y: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// `null` when empty.
    pub offsets: Option<IndexMap<i32, Offset>>,
}

impl Frame {
    pub fn from_xml(xml: Node) -> Frame {
        let mut offsets = IndexMap::new();
        if let Some(element) = element(xml, "offsets") {
            for offset in elements(element, "offset") {
                if let Some(direction) = try_parse_int(attr(offset, "direction")) {
                    offsets.insert(
                        direction,
                        Offset {
                            direction: try_parse_int(attr(offset, "direction")).unwrap_or(0),
                            x: try_parse_int(attr(offset, "x")),
                            y: try_parse_int(attr(offset, "y")),
                        },
                    );
                }
            }
        }
        Frame {
            id: try_parse_int(attr(xml, "id")).unwrap_or(0),
            random_x: try_parse_int(attr(xml, "randomX")),
            random_y: try_parse_int(attr(xml, "randomY")),
            x: try_parse_int(attr(xml, "x")),
            y: try_parse_int(attr(xml, "y")),
            offsets: (!offsets.is_empty()).then_some(offsets),
        }
    }

    /// The pets `FrameConverter.Write`.
    pub fn to_json(&self) -> J {
        let offsets = self.offsets.as_ref().filter(|o| !o.is_empty()).map(|o| {
            int_map(o, |offset| {
                Obj::new()
                    .put("direction", J::int(offset.direction as i64))
                    .opt("x", int_j(offset.x))
                    .opt("y", int_j(offset.y))
                    .build()
            })
        });
        Obj::new()
            .put("id", J::int(self.id as i64))
            .opt("randomX", int_j(self.random_x))
            .opt("randomY", int_j(self.random_y))
            .opt("x", int_j(self.x))
            .opt("y", int_j(self.y))
            .opt("offsets", offsets)
            .build()
    }
}

/// The pets `FrameSequence`: only `frames`.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSequence {
    pub frames: IndexMap<i32, Frame>,
}

impl FrameSequence {
    pub fn from_xml(xml: Node) -> FrameSequence {
        FrameSequence { frames: elements(xml, "frame").enumerate().map(|(i, f)| (i as i32, Frame::from_xml(f))).collect() }
    }

    pub fn to_json(&self) -> J {
        Obj::new().put("frames", int_map(&self.frames, Frame::to_json)).build()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimationLayer {
    pub frame_repeat: Option<i32>,
    pub loop_count: Option<i32>,
    pub random: Option<i32>,
    pub frame_sequences: Option<IndexMap<i32, FrameSequence>>,
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

    pub fn to_json(&self) -> J {
        Obj::new()
            .opt("frameRepeat", int_j(self.frame_repeat))
            .opt("loopCount", int_j(self.loop_count))
            .opt("random", int_j(self.random))
            .opt("frameSequences", self.frame_sequences.as_ref().map(|s| int_map(s, FrameSequence::to_json)))
            .build()
    }
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

impl Animation {
    pub fn from_xml(xml: Node) -> Animation {
        let base = FurniAnimation::from_xml(xml);
        let mut layers = IndexMap::new();
        for layer in elements(xml, "animationLayer") {
            if let Some(id) = try_parse_int(attr(layer, "id")) {
                layers.insert(id, AnimationLayer::from_xml(layer));
            }
        }
        Animation {
            id: base.id,
            transition_to: base.transition_to,
            transition_from: base.transition_from,
            immediate_change_from: base.immediate_change_from,
            random_start: base.random_start,
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

/// `Posture` / `Gesture`: `{ id, animationId }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Posture {
    pub id: Option<String>,
    pub animation_id: i32,
}

impl Posture {
    pub fn from_xml(xml: Node) -> Posture {
        Posture { id: attr(xml, "id").map(str::to_string), animation_id: try_parse_int(attr(xml, "animationId")).unwrap_or(0) }
    }

    pub fn to_json(&self) -> J {
        Obj::new()
            .opt("id", self.id.as_ref().map(|s| J::str(s.clone())))
            .put("animationId", J::int(self.animation_id as i64))
            .build()
    }
}

pub type Gesture = Posture;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PostureCollection {
    pub default_posture: Option<String>,
    pub postures: Vec<Posture>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Visualization {
    pub angle: i32,
    pub layer_count: i32,
    pub size: i32,
    pub layers: IndexMap<i32, Layer>,
    pub directions: BTreeMap<i32, BTreeMap<i32, Layer>>,
    pub animations: Option<IndexMap<i32, Animation>>,
    pub colors: IndexMap<i32, Color>,
    pub postures: Option<PostureCollection>,
    pub gestures: Vec<Gesture>,
}

impl Visualization {
    pub fn from_xml(xml: Node) -> Result<Visualization> {
        let mut directions = BTreeMap::new();
        if let Some(element) = element(xml, "directions") {
            for direction in elements(element, "direction") {
                if let Some(id) = try_parse_int(attr(direction, "id")) {
                    let layers: BTreeMap<i32, Layer> = parse_layers(Some(direction), Layer::from_xml).into_iter().collect();
                    directions.insert(id, layers);
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

        let mut colors = IndexMap::new();
        if let Some(element) = element(xml, "colors") {
            for color in elements(element, "color") {
                if let Some(id) = try_parse_int(attr(color, "id")) {
                    colors.insert(id, Color::from_xml(color)?);
                }
            }
        }

        let postures = element(xml, "postures").map(|element| PostureCollection {
            default_posture: attr(element, "defaultPosture").map(str::to_string),
            postures: elements(element, "posture").map(Posture::from_xml).collect(),
        });

        let gestures = element(xml, "gestures").map(|element| elements(element, "gesture").map(Posture::from_xml).collect()).unwrap_or_default();

        Ok(Visualization {
            size: try_parse_int(attr(xml, "size")).unwrap_or(0),
            layer_count: try_parse_int(attr(xml, "layerCount")).unwrap_or(0),
            angle: try_parse_int(attr(xml, "angle")).unwrap_or(0),
            layers: parse_layers(element(xml, "layers"), Layer::from_xml),
            directions,
            animations: (!animations.is_empty()).then_some(animations),
            colors,
            postures,
            gestures,
        })
    }

    /// The `Postures` getter: `null` unless it holds at least one posture.
    pub fn visible_postures(&self) -> Option<&PostureCollection> {
        self.postures.as_ref().filter(|p| !p.postures.is_empty())
    }

    /// `SWF_Pets_To_Nitro.IsVisualizationEmpty`.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
            && self.directions.is_empty()
            && self.animations.as_ref().is_none_or(|a| a.is_empty())
            && self.colors.is_empty()
            && self.visible_postures().is_none()
            && self.gestures.is_empty()
    }

    pub fn to_json(&self) -> J {
        Obj::new()
            .put("angle", J::int(self.angle as i64))
            .put("layerCount", J::int(self.layer_count as i64))
            .put("size", J::int(self.size as i64))
            .opt("layers", (!self.layers.is_empty()).then(|| int_map(&self.layers, Layer::to_json)))
            .opt(
                "directions",
                (!self.directions.is_empty()).then(|| {
                    sorted_map(&self.directions, |layers| Obj::new().put("layers", sorted_map(layers, Layer::to_json)).build())
                }),
            )
            .opt("animations", self.animations.as_ref().map(|a| int_map(a, Animation::to_json)))
            .opt("colors", (!self.colors.is_empty()).then(|| int_map(&self.colors, Color::to_json)))
            .opt(
                "postures",
                self.visible_postures().map(|p| {
                    Obj::new()
                        .opt("defaultPosture", p.default_posture.as_ref().map(|s| J::str(s.clone())))
                        .put("postures", J::Arr(p.postures.iter().map(Posture::to_json).collect()))
                        .build()
                }),
            )
            .opt("gestures", (!self.gestures.is_empty()).then(|| J::Arr(self.gestures.iter().map(Posture::to_json).collect())))
            .build()
    }
}

/// The pets `VisualizationsMapper.MapVisualizationsXml`: only `graphics/visualization`.
pub fn map_visualizations_xml(root: Node) -> Result<Vec<Visualization>> {
    let mut list = Vec::new();
    for graphics in elements(root, "graphics") {
        for visualization in elements(graphics, "visualization") {
            list.push(Visualization::from_xml(visualization)?);
        }
    }
    Ok(list)
}

pub fn parse_visualizations_text(text: &str) -> Result<Vec<Visualization>> {
    let doc = crate::xml::parse(text)?;
    map_visualizations_xml(doc.root_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nl(s: &str) -> String {
        s.replace('\n', habbo_core::paths::NEWLINE)
    }

    #[test]
    fn maps_pet_visualization() {
        let xml = r#"<visualizationData>
  <graphics>
    <visualization size="64" layerCount="1" angle="45">
      <layers><layer id="0" z="2" alpha="1.5"/></layers>
      <directions><direction id="4"><layer id="1"/><layer id="0" z="-1"/></direction><direction id="2"/></directions>
      <animations><animation id="1"><animationLayer id="0"><frameSequence><frame id="3" randomX="1">
        <offsets><offset direction="2" x="1"/><offset direction="0" y="-2"/></offsets>
      </frame></frameSequence></animationLayer></animation></animations>
      <postures defaultPosture="std"><posture id="std" animationId="0"/><posture animationId="2"/></postures>
      <gestures><gesture id="agr" animationId="10"/></gestures>
    </visualization>
    <visualization size="32"><postures/></visualization>
  </graphics>
  <visualization size="1"/>
</visualizationData>"#;
        let list = parse_visualizations_text(xml).unwrap();
        assert_eq!(list.len(), 2);
        assert!(!list[0].is_empty());
        assert!(list[1].is_empty());
        let json = list[0].to_json().to_string_indented().unwrap();
        let expected = r#"{
  "angle": 45,
  "layerCount": 1,
  "size": 64,
  "layers": {
    "0": {
      "z": 2
    }
  },
  "directions": {
    "2": {
      "layers": {}
    },
    "4": {
      "layers": {
        "0": {
          "z": -1
        },
        "1": {}
      }
    }
  },
  "animations": {
    "1": {
      "layers": {
        "0": {
          "frameSequences": {
            "0": {
              "frames": {
                "0": {
                  "id": 3,
                  "randomX": 1,
                  "offsets": {
                    "2": {
                      "direction": 2,
                      "x": 1
                    },
                    "0": {
                      "direction": 0,
                      "y": -2
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  },
  "postures": {
    "defaultPosture": "std",
    "postures": [
      {
        "id": "std",
        "animationId": 0
      },
      {
        "animationId": 2
      }
    ]
  },
  "gestures": [
    {
      "id": "agr",
      "animationId": 10
    }
  ]
}"#;
        assert_eq!(json, nl(expected));
        assert_eq!(list[1].to_json().to_string_indented().unwrap(), nl("{\n  \"angle\": 0,\n  \"layerCount\": 0,\n  \"size\": 32\n}"));
    }
}
