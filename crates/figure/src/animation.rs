//! `EffectAnimationMapper`: the effect `*_animation.bin` XML -> Nitro `animations` json.
//! Property order follows the C# models, including `[JsonPropertyOrder]` (unordered
//! properties first in declaration order, then the ordered ones).

use std::path::Path;

use habbo_core::outln;
use indexmap::IndexMap;
use roxmltree::Node as X;

use crate::stj::{Node, ObjectBuilder};
use crate::util::{get_files, read_all_text, try_parse_bool, try_parse_int};
use crate::xml::{self, attr};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimation {
    pub name: Option<String>,
    pub desc: Option<String>,
    pub reset_on_toggle: Option<bool>,
    pub directions: Option<Vec<AssetAnimationDirection>>,
    pub shadows: Option<Vec<IdOnly>>,
    pub adds: Option<Vec<AssetAnimationAdd>>,
    pub removes: Option<Vec<IdOnly>>,
    pub sprites: Option<Vec<AssetAnimationSprite>>,
    pub frames: Option<Vec<AssetAnimationFrame>>,
    pub avatars: Option<Vec<AssetAnimationAvatar>>,
    pub overrides: Option<Vec<AssetAnimationOverride>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationDirection {
    pub offset: Option<i32>,
}

/// `AssetAnimationShadow` / `AssetAnimationRemove`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IdOnly {
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationAdd {
    pub id: Option<String>,
    pub align: Option<String>,
    pub blend: Option<String>,
    pub ink: Option<i32>,
    pub base: Option<String>,
    pub action: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationSprite {
    pub id: Option<String>,
    pub directions: Option<i32>,
    pub member: Option<String>,
    pub ink: Option<i32>,
    pub static_y: Option<i32>,
    pub direction_list: Option<Vec<AssetAnimationSpriteDirection>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationSpriteDirection {
    pub id: Option<i32>,
    pub dx: Option<i32>,
    pub dy: Option<i32>,
    pub dz: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationFrame {
    pub repeats: Option<i32>,
    pub directions: Option<Vec<AssetAnimationDirection>>,
    pub fxs: Option<Vec<AssetAnimationFramePart>>,
    pub bodyparts: Option<Vec<AssetAnimationFramePart>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationFramePart {
    pub id: Option<String>,
    pub frame: Option<i32>,
    pub dx: Option<i32>,
    pub dy: Option<i32>,
    pub dz: Option<i32>,
    pub dd: Option<i32>,
    pub base: Option<String>,
    pub action: Option<String>,
    pub items: Option<Vec<AssetAnimationFramePartItem>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationFramePartItem {
    pub id: Option<String>,
    pub base: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationAvatar {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub ink: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetAnimationOverride {
    pub name: Option<String>,
    pub override_: Option<String>,
    pub frames: Option<Vec<AssetAnimationFrame>>,
}

pub type Animations = IndexMap<String, AssetAnimation>;

fn list<T>(items: &Option<Vec<T>>, f: impl Fn(&T) -> Node) -> Option<Node> {
    items.as_ref().map(|items| Node::Array(items.iter().map(f).collect()))
}

fn direction_node(d: &AssetAnimationDirection) -> Node {
    ObjectBuilder::new().opt_int("offset", d.offset).build()
}

fn id_node(d: &IdOnly) -> Node {
    ObjectBuilder::new().opt_str("id", &d.id).build()
}

fn frame_part_node(p: &AssetAnimationFramePart) -> Node {
    // [JsonPropertyOrder(1)] base, [JsonPropertyOrder(2)] action come after the others.
    ObjectBuilder::new()
        .opt_str("id", &p.id)
        .opt_int("frame", p.frame)
        .opt_int("dx", p.dx)
        .opt_int("dy", p.dy)
        .opt_int("dz", p.dz)
        .opt_int("dd", p.dd)
        .opt(
            "items",
            list(&p.items, |i| ObjectBuilder::new().opt_str("id", &i.id).opt_str("base", &i.base).build()),
        )
        .opt_str("base", &p.base)
        .opt_str("action", &p.action)
        .build()
}

fn frame_node(f: &AssetAnimationFrame) -> Node {
    ObjectBuilder::new()
        .opt_int("repeats", f.repeats)
        .opt("directions", list(&f.directions, direction_node))
        .opt("fxs", list(&f.fxs, frame_part_node))
        .opt("bodyparts", list(&f.bodyparts, frame_part_node))
        .build()
}

impl AssetAnimation {
    pub fn to_node(&self) -> Node {
        ObjectBuilder::new()
            .opt_str("name", &self.name)
            .opt_str("desc", &self.desc)
            .opt("resetOnToggle", self.reset_on_toggle.map(Node::bool))
            .opt("directions", list(&self.directions, direction_node))
            .opt("shadows", list(&self.shadows, id_node))
            .opt(
                "adds",
                list(&self.adds, |a| {
                    ObjectBuilder::new()
                        .opt_str("id", &a.id)
                        .opt_str("align", &a.align)
                        .opt_str("blend", &a.blend)
                        .opt_int("ink", a.ink)
                        .opt_str("base", &a.base)
                        .opt_str("action", &a.action)
                        .build()
                }),
            )
            .opt("removes", list(&self.removes, id_node))
            .opt(
                "sprites",
                list(&self.sprites, |s| {
                    ObjectBuilder::new()
                        .opt_str("id", &s.id)
                        .opt_int("directions", s.directions)
                        .opt_str("member", &s.member)
                        .opt_int("ink", s.ink)
                        .opt_int("staticY", s.static_y)
                        .opt(
                            "directionList",
                            list(&s.direction_list, |d| {
                                ObjectBuilder::new()
                                    .opt_int("id", d.id)
                                    .opt_int("dx", d.dx)
                                    .opt_int("dy", d.dy)
                                    .opt_int("dz", d.dz)
                                    .build()
                            }),
                        )
                        .build()
                }),
            )
            .opt("frames", list(&self.frames, frame_node))
            .opt(
                "avatars",
                list(&self.avatars, |a| {
                    ObjectBuilder::new()
                        .opt_str("background", &a.background)
                        .opt_str("foreground", &a.foreground)
                        .opt_int("ink", a.ink)
                        .build()
                }),
            )
            .opt(
                "overrides",
                list(&self.overrides, |o| {
                    ObjectBuilder::new()
                        .opt_str("name", &o.name)
                        .opt_str("override", &o.override_)
                        .opt("frames", list(&o.frames, frame_node))
                        .build()
                }),
            )
            .build()
    }
}

pub fn animations_node(animations: &Animations) -> Node {
    Node::Object(animations.iter().map(|(k, v)| (k.clone(), v.to_node())).collect())
}

/// `ParseAnimationFileAsync`: the first `*_animation.bin` in the binaryData folder.
pub fn parse_animation_file(binary_output_path: &Path) -> anyhow::Result<Animations> {
    let files = get_files(binary_output_path, "*_animation.bin")?;
    let Some(animation_file) = files.first() else {
        outln!("❌ Animation file not found in {}", binary_output_path.display());
        return Ok(Animations::new());
    };
    let content = read_all_text(animation_file)?;
    match parse_animation_xml(&content) {
        Ok(animations) => Ok(animations),
        Err(error) => {
            outln!("❌ Error parsing animation file {}: {error}", animation_file.display());
            Ok(Animations::new())
        }
    }
}

/// `MapAnimationXML` over the root element: one animation, keyed by its `desc`.
pub fn parse_animation_xml(text: &str) -> Result<Animations, roxmltree::Error> {
    let doc = xml::parse(text)?;
    let mut output = Animations::new();
    map_animation_xml(&doc.root_element(), &mut output);
    Ok(output)
}

fn int_attr(node: &X, name: &str) -> Option<i32> {
    try_parse_int(node.attribute(name))
}

fn local_lower(node: &X) -> String {
    node.tag_name().name().to_lowercase()
}

fn map_add(e: &X) -> AssetAnimationAdd {
    AssetAnimationAdd {
        id: attr(e, "id"),
        align: attr(e, "align"),
        blend: attr(e, "blend"),
        base: attr(e, "base"),
        action: attr(e, "action"),
        ink: int_attr(e, "ink"),
    }
}

fn map_animation_xml(root: &X, output: &mut Animations) {
    let mut animation = AssetAnimation {
        name: attr(root, "name"),
        desc: attr(root, "desc"),
        reset_on_toggle: try_parse_bool(root.attribute("resetOnToggle")),
        ..Default::default()
    };

    for element in xml::child_elements(root) {
        match local_lower(&element).as_str() {
            "direction" => {
                let list = animation.directions.get_or_insert_with(Vec::new);
                if let Some(offset) = int_attr(&element, "offset") {
                    list.push(AssetAnimationDirection { offset: Some(offset) });
                }
            }
            "shadow" => animation.shadows.get_or_insert_with(Vec::new).push(IdOnly { id: attr(&element, "id") }),
            name @ ("adds" | "add") => {
                let list = animation.adds.get_or_insert_with(Vec::new);
                if name == "adds" {
                    list.extend(xml::elements(&element, "add").map(|a| map_add(&a)));
                } else {
                    list.push(map_add(&element));
                }
            }
            "remove" => animation.removes.get_or_insert_with(Vec::new).push(IdOnly { id: attr(&element, "id") }),
            "avatar" => animation.avatars.get_or_insert_with(Vec::new).push(AssetAnimationAvatar {
                ink: int_attr(&element, "ink"),
                foreground: attr(&element, "foreground"),
                background: attr(&element, "background"),
            }),
            "sprite" => {
                let directions: Vec<AssetAnimationSpriteDirection> = xml::elements(&element, "direction")
                    .map(|d| AssetAnimationSpriteDirection {
                        id: int_attr(&d, "id"),
                        dx: int_attr(&d, "dx"),
                        dy: int_attr(&d, "dy"),
                        dz: int_attr(&d, "dz"),
                    })
                    .collect();
                animation.sprites.get_or_insert_with(Vec::new).push(AssetAnimationSprite {
                    id: attr(&element, "id"),
                    member: attr(&element, "member"),
                    ink: int_attr(&element, "ink"),
                    static_y: int_attr(&element, "staticY"),
                    directions: int_attr(&element, "directions"),
                    direction_list: (!directions.is_empty()).then_some(directions),
                });
            }
            "frame" => animation.frames.get_or_insert_with(Vec::new).push(map_frame(&element)),
            "override" => {
                let mut ov = AssetAnimationOverride {
                    name: attr(&element, "name"),
                    override_: attr(&element, "override"),
                    frames: None,
                };
                if let Some(frames) = xml::element(&element, "frames") {
                    ov.frames = Some(xml::elements(&frames, "frame").map(|f| map_frame(&f)).collect());
                } else {
                    let frames: Vec<_> = xml::elements(&element, "frame").map(|f| map_frame(&f)).collect();
                    if !frames.is_empty() {
                        ov.frames = Some(frames);
                    }
                }
                animation.overrides.get_or_insert_with(Vec::new).push(ov);
            }
            _ => {}
        }
    }

    if let Some(desc) = animation.desc.clone().filter(|d| !d.is_empty()) {
        output.insert(desc, animation);
    }
}

fn map_part(child: &X) -> AssetAnimationFramePart {
    AssetAnimationFramePart {
        id: attr(child, "id"),
        action: attr(child, "action"),
        frame: int_attr(child, "frame"),
        dx: int_attr(child, "dx"),
        dy: int_attr(child, "dy"),
        dz: int_attr(child, "dz"),
        dd: int_attr(child, "dd"),
        ..Default::default()
    }
}

fn map_frame(frame_el: &X) -> AssetAnimationFrame {
    let mut frame = AssetAnimationFrame { repeats: int_attr(frame_el, "repeats"), ..Default::default() };

    for child in xml::child_elements(frame_el) {
        match local_lower(&child).as_str() {
            "direction" => {
                if let Some(offset) = int_attr(&child, "offset") {
                    frame.directions.get_or_insert_with(Vec::new).push(AssetAnimationDirection { offset: Some(offset) });
                }
            }
            "fx" => frame.fxs.get_or_insert_with(Vec::new).push(map_part(&child)),
            "bodypart" => {
                let mut part = map_part(&child);
                part.base = attr(&child, "base");
                let items: Vec<_> = xml::elements(&child, "item")
                    .map(|i| AssetAnimationFramePartItem { id: attr(&i, "id"), base: attr(&i, "base") })
                    .collect();
                if !items.is_empty() {
                    part.items = Some(items);
                }
                frame.bodyparts.get_or_insert_with(Vec::new).push(part);
            }
            _ => {}
        }
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stj::to_string_indented;

    const XML: &str = r#"<animation name="Dance" desc="dance.1" resetOnToggle="true">
  <direction offset="2"/>
  <direction offset="x"/>
  <shadow id="std"/>
  <add id="fx1" align="top" blend="ADD" ink="33" base="b"/>
  <adds><add id="fx2"/><Add id="ignored"/></adds>
  <remove id="lh"/>
  <avatar ink="5" foreground="fg" background="bg"/>
  <sprite id="s1" member="m" directions="8" staticY="1"><direction id="0" dx="1" dy="-2" dz="3"/></sprite>
  <sprite id="s2"/>
  <frame repeats="2">
    <direction offset="1"/>
    <fx id="fx1" frame="3" dx="1" dd="2" action="Wave"/>
    <bodypart id="torso" action="Dance" base="std" frame="1"><item id="i" base="ib"/></bodypart>
  </frame>
  <frame/>
  <override name="o" override="x"><frames><frame><bodypart id="head"/></frame></frames></override>
  <override name="p"><frame repeats="1"/></override>
</animation>"#;

    #[test]
    fn parses_every_section() {
        let animations = parse_animation_xml(XML).unwrap();
        let a = &animations["dance.1"];
        assert_eq!(a.reset_on_toggle, Some(true));
        assert_eq!(a.directions.as_ref().unwrap().len(), 1);
        assert_eq!(a.adds.as_ref().unwrap().len(), 2);
        assert_eq!(a.adds.as_ref().unwrap()[0].ink, Some(33));
        let sprites = a.sprites.as_ref().unwrap();
        assert_eq!(sprites[0].direction_list.as_ref().unwrap()[0].dy, Some(-2));
        assert!(sprites[1].direction_list.is_none());
        assert_eq!(a.frames.as_ref().unwrap().len(), 2);
        assert_eq!(a.overrides.as_ref().unwrap()[1].frames.as_ref().unwrap()[0].repeats, Some(1));
    }

    #[test]
    fn json_property_order_and_omissions() {
        let animations = parse_animation_xml(XML).unwrap();
        let json = to_string_indented(&animations_node(&animations)).replace("\r\n", "\n");
        let sp = |n: usize| " ".repeat(n);
        // Body part: id, frame, items, then base and action ([JsonPropertyOrder]).
        let torso = format!(
            "{{
{p}\"id\": \"torso\",
{p}\"frame\": 1,
{p}\"items\": [
{i}{{
{q}\"id\": \"i\",
{q}\"base\": \"ib\"
{i}}}
{p}],
{p}\"base\": \"std\",
{p}\"action\": \"Dance\"
{c}}}",
            p = sp(12), i = sp(14), q = sp(16), c = sp(10)
        );
        assert!(json.contains(&torso), "{json}");
        assert!(json.contains("\"avatars\": [
      {
        \"background\": \"bg\",
        \"foreground\": \"fg\",
        \"ink\": 5
      }
    ]"));
        let override_frames = format!(
            "\"frames\": [
{a}{{
{b}\"bodyparts\": [
{c}{{
{d}\"id\": \"head\"
{c}}}
{b}]
{a}}}
{e}]",
            a = sp(10), b = sp(12), c = sp(14), d = sp(16), e = sp(8)
        );
        assert!(json.contains(&override_frames), "{json}");
        // An empty frame is an empty object.
        assert!(json.contains("},\n      {}\n    ],"));
        assert!(json.starts_with("{\n  \"dance.1\": {\n    \"name\": \"Dance\",\n    \"desc\": \"dance.1\",\n    \"resetOnToggle\": true,"));
    }

    #[test]
    fn no_desc_means_no_animation() {
        assert!(parse_animation_xml("<animation name=\"x\"/>").unwrap().is_empty());
    }
}
