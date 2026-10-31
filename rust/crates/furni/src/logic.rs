//! `Mapper/Logic`: `AssetLogicData` and friends, `LogicMapper`, `MapParticleSystem`,
//! `MapPlanetSystem` and the json each one serializes to (`FloatToFixedDecimalConverter`
//! lives in [`crate::stj::float_to_fixed_decimal`]).

use crate::net::{try_parse_f32, try_parse_int};
use crate::stj::{FloatStyle, Obj, J};
use crate::xml::{attr, element, elements, Node};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetLogicData {
    pub model: Option<AssetLogicModel>,
    pub mask_type: Option<String>,
    pub credits: Option<String>,
    pub sound_sample: Option<SoundSample>,
    pub action: Option<ActionData>,
    pub planet_systems: Vec<AssetLogicPlanetSystem>,
    pub particle_systems: Vec<ParticleSystem>,
    pub custom_vars: CustomVars,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetLogicModel {
    pub dimensions: Option<AssetDimension>,
    pub directions: Vec<i32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetDimension {
    pub x: f32,
    pub y: f32,
    pub z: Option<f32>,
    pub center_z: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SoundSample {
    pub id: i32,
    pub no_pitch: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActionData {
    pub link: Option<String>,
    pub start_state: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetLogicPlanetSystem {
    pub id: Option<i32>,
    pub name: Option<String>,
    pub parent: Option<String>,
    pub radius: Option<f32>,
    pub arc_speed: Option<f32>,
    pub arc_offset: Option<f32>,
    pub blend: Option<i32>,
    pub height: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticleSystem {
    pub size: Option<i32>,
    pub canvas_id: Option<i32>,
    pub offset_y: Option<f32>,
    pub blend: Option<f32>,
    pub bg_color: Option<String>,
    pub emitters: Option<Vec<ParticleSystemEmitter>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticleSystemEmitter {
    pub id: Option<i32>,
    pub name: Option<String>,
    pub sprite_id: Option<i32>,
    pub max_num_particles: Option<i32>,
    pub particles_per_frame: Option<i32>,
    pub burst_pulse: i32,
    pub fuse_time: Option<i32>,
    pub simulation: Option<ParticleSystemSimulation>,
    pub particles: Option<Vec<ParticleSystemParticle>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticleSystemSimulation {
    pub force: Option<f32>,
    pub direction: Option<f32>,
    pub gravity: Option<f32>,
    pub air_friction: Option<f32>,
    pub shape: Option<String>,
    pub energy: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticleSystemParticle {
    pub is_emitter: bool,
    pub life_time: Option<i32>,
    pub fade: bool,
    pub frames: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomVars {
    pub variables: Vec<String>,
}

fn int_j(v: Option<i32>) -> Option<J> {
    v.map(|v| J::int(v as i64))
}

fn str_j(v: &Option<String>) -> Option<J> {
    v.as_ref().map(|s| J::str(s.clone()))
}

fn f32_j(v: Option<f32>, style: FloatStyle) -> Option<J> {
    v.map(|v| J::f32_with(v, style))
}

impl AssetDimension {
    /// `z` always goes through `FloatToFixedDecimalConverter` (property attribute).
    pub fn to_json(&self, style: FloatStyle) -> J {
        Obj::new()
            .put("x", J::f32_with(self.x, style))
            .put("y", J::f32_with(self.y, style))
            .opt("z", self.z.map(J::f32_fixed))
            .opt("centerZ", f32_j(self.center_z, style))
            .build()
    }
}

impl SoundSample {
    pub fn to_json(&self) -> J {
        Obj::new().put("id", J::int(self.id as i64)).put("noPitch", J::Bool(self.no_pitch)).build()
    }
}

impl ActionData {
    pub fn to_json(&self) -> J {
        Obj::new()
            .opt("link", self.link.as_ref().filter(|l| !l.is_empty()).map(|l| J::str(l.clone())))
            .opt("startState", int_j(self.start_state))
            .build()
    }
}

impl AssetLogicPlanetSystem {
    pub fn to_json(&self, style: FloatStyle) -> J {
        Obj::new()
            .opt("id", int_j(self.id))
            .opt("name", str_j(&self.name))
            .opt("parent", str_j(&self.parent))
            .opt("radius", f32_j(self.radius, style))
            .opt("arcSpeed", f32_j(self.arc_speed, style))
            .opt("arcOffset", f32_j(self.arc_offset, style))
            .opt("blend", int_j(self.blend))
            .opt("height", f32_j(self.height, style))
            .build()
    }
}

impl ParticleSystem {
    pub fn to_json(&self, style: FloatStyle) -> J {
        Obj::new()
            .opt("size", int_j(self.size))
            .opt("canvasId", int_j(self.canvas_id))
            .opt("offsetY", f32_j(self.offset_y, style))
            .opt("blend", f32_j(self.blend, style))
            .opt("bgColor", str_j(&self.bg_color))
            .opt("emitters", self.emitters.as_ref().map(|e| J::Arr(e.iter().map(|e| e.to_json(style)).collect())))
            .build()
    }
}

impl ParticleSystemEmitter {
    pub fn to_json(&self, style: FloatStyle) -> J {
        Obj::new()
            .opt("id", int_j(self.id))
            .opt("name", str_j(&self.name))
            .opt("spriteId", int_j(self.sprite_id))
            .opt("maxNumParticles", int_j(self.max_num_particles))
            .opt("particlesPerFrame", int_j(self.particles_per_frame))
            .put("burstPulse", J::int(self.burst_pulse as i64))
            .opt("fuseTime", int_j(self.fuse_time))
            .opt("simulation", self.simulation.as_ref().map(|s| s.to_json(style)))
            .opt("particles", self.particles.as_ref().map(|p| J::Arr(p.iter().map(|p| p.to_json()).collect())))
            .build()
    }
}

impl ParticleSystemSimulation {
    pub fn to_json(&self, style: FloatStyle) -> J {
        Obj::new()
            .opt("force", f32_j(self.force, style))
            .opt("direction", f32_j(self.direction, style))
            .opt("gravity", f32_j(self.gravity, style))
            .opt("airFriction", f32_j(self.air_friction, style))
            .opt("shape", str_j(&self.shape))
            .opt("energy", f32_j(self.energy, style))
            .build()
    }
}

impl ParticleSystemParticle {
    pub fn to_json(&self) -> J {
        Obj::new()
            .put("isEmitter", J::Bool(self.is_emitter))
            .opt("lifeTime", int_j(self.life_time))
            .put("fade", J::Bool(self.fade))
            .put("frames", J::Arr(self.frames.iter().map(|f| J::str(f.clone())).collect()))
            .build()
    }
}

impl CustomVars {
    pub fn to_json(&self) -> J {
        Obj::new()
            .opt(
                "variables",
                (!self.variables.is_empty()).then(|| J::Arr(self.variables.iter().map(|v| J::str(v.clone())).collect())),
            )
            .build()
    }
}

/// The anonymous `logicObject` both converters build from an optional `AssetLogicData`.
/// `None` is the furniture `logicData?.` path (the pets C# code dereferences it and fails
/// before getting here).
pub fn logic_object_json(logic: Option<&AssetLogicData>, style: FloatStyle) -> J {
    let model = logic.and_then(|l| l.model.as_ref());
    let model_json = Obj::new()
        .opt("dimensions", model.and_then(|m| m.dimensions.as_ref()).map(|d| d.to_json(style)))
        .opt("directions", model.map(|m| J::Arr(m.directions.iter().map(|d| J::int(*d as i64)).collect())))
        .build();
    let Some(logic) = logic else {
        return Obj::new().put("model", model_json).build();
    };
    Obj::new()
        .put("model", model_json)
        .opt("action", logic.action.as_ref().map(|a| a.to_json()))
        .opt("maskType", str_j(&logic.mask_type))
        .opt("credits", str_j(&logic.credits))
        .opt("soundSample", logic.sound_sample.as_ref().map(|s| s.to_json()))
        .opt(
            "planetSystems",
            (!logic.planet_systems.is_empty())
                .then(|| J::Arr(logic.planet_systems.iter().map(|p| p.to_json(style)).collect())),
        )
        .opt(
            "particleSystems",
            (!logic.particle_systems.is_empty())
                .then(|| J::Arr(logic.particle_systems.iter().map(|p| p.to_json(style)).collect())),
        )
        .opt("customVars", (!logic.custom_vars.variables.is_empty()).then(|| logic.custom_vars.to_json()))
        .build()
}

/// `LogicMapper.MapLogicXml`.
pub fn map_logic_xml(logic: Node) -> AssetLogicData {
    let mut output = AssetLogicData::default();

    if let Some(model) = element(logic, "model") {
        output.model = Some(AssetLogicModel {
            dimensions: element(model, "dimensions").map(map_dimensions),
            directions: map_directions(element(model, "directions")),
        });
    }

    if let Some(action) = element(logic, "action") {
        output.action = Some(ActionData {
            link: attr(action, "link").map(str::to_string),
            start_state: try_parse_int(attr(action, "startState")),
        });
    }

    output.mask_type = element(logic, "mask").and_then(|m| attr(m, "type")).map(str::to_string);
    output.credits = element(logic, "credits").and_then(|c| attr(c, "value")).map(str::to_string);

    if let Some(sample) = element(logic, "sound").and_then(|s| element(s, "sample")) {
        output.sound_sample = Some(SoundSample {
            id: try_parse_int(attr(sample, "id")).unwrap_or(0),
            no_pitch: attr(sample, "nopitch") == Some("true"),
        });
    }

    if let Some(planets) = element(logic, "planetsystem") {
        output.planet_systems = map_planet_systems(elements(planets, "object"));
    }

    if let Some(particles) = element(logic, "particlesystems") {
        output.particle_systems = map_particle_systems(elements(particles, "particlesystem"));
    }

    if let Some(custom) = element(logic, "customvars") {
        let mut vars = CustomVars::default();
        for variable in elements(custom, "variable") {
            if let Some(name) = attr(variable, "name").filter(|n| !n.is_empty()) {
                vars.variables.push(name.to_string());
            }
        }
        output.custom_vars = vars;
    }

    output
}

fn parse_float_or_null(value: Option<&str>) -> Option<f32> {
    try_parse_f32(value).filter(|v| !v.is_nan())
}

fn map_dimensions(dimensions: Node) -> AssetDimension {
    AssetDimension {
        x: parse_float_or_null(attr(dimensions, "x")).unwrap_or(0.0),
        y: parse_float_or_null(attr(dimensions, "y")).unwrap_or(0.0),
        z: Some(parse_float_or_null(attr(dimensions, "z")).unwrap_or(0.000001)),
        center_z: parse_float_or_null(attr(dimensions, "centerZ")),
    }
}

fn map_directions(directions: Option<Node>) -> Vec<i32> {
    let Some(directions) = directions else { return Vec::new() };
    let mut list: Vec<i32> = elements(directions, "direction").filter_map(|d| try_parse_int(attr(d, "id"))).collect();
    if list.is_empty() {
        list.push(0);
    }
    list
}

/// `MapPlanetSystem.MapPlanetSystems`.
pub fn map_planet_systems<'a, 'i: 'a>(planets: impl Iterator<Item = Node<'a, 'i>>) -> Vec<AssetLogicPlanetSystem> {
    planets
        .map(|p| AssetLogicPlanetSystem {
            id: try_parse_int(attr(p, "id")),
            name: attr(p, "name").map(str::to_string),
            parent: attr(p, "parent").map(str::to_string),
            radius: try_parse_f32(attr(p, "radius")),
            arc_speed: try_parse_f32(attr(p, "arcspeed")),
            arc_offset: try_parse_f32(attr(p, "arcoffset")),
            blend: try_parse_int(attr(p, "blend")),
            height: try_parse_f32(attr(p, "height")),
        })
        .collect()
}

/// `MapParticleSystem.MapParticleSystems`.
pub fn map_particle_systems<'a, 'i: 'a>(systems: impl Iterator<Item = Node<'a, 'i>>) -> Vec<ParticleSystem> {
    systems
        .map(|p| {
            let emitters = map_emitters(p);
            ParticleSystem {
                size: try_parse_int(attr(p, "size")),
                canvas_id: try_parse_int(attr(p, "canvas_id")),
                offset_y: try_parse_f32(attr(p, "offset_y")),
                blend: try_parse_f32(attr(p, "blend")),
                bg_color: attr(p, "bg_color").map(str::to_string),
                emitters: (!emitters.is_empty()).then_some(emitters),
            }
        })
        .filter(|ps| ps.emitters.is_some() || ps.size.is_some())
        .collect()
}

fn map_emitters(system: Node) -> Vec<ParticleSystemEmitter> {
    elements(system, "emitter")
        .map(|e| ParticleSystemEmitter {
            id: try_parse_int(attr(e, "id")),
            name: attr(e, "name").map(str::to_string),
            sprite_id: try_parse_int(attr(e, "sprite_id")),
            max_num_particles: try_parse_int(attr(e, "max_num_particles")),
            particles_per_frame: try_parse_int(attr(e, "particles_per_frame")),
            burst_pulse: try_parse_int(attr(e, "burst_pulse")).unwrap_or(1),
            fuse_time: try_parse_int(attr(e, "fuse_time")),
            simulation: element(e, "simulation").map(|s| ParticleSystemSimulation {
                force: try_parse_f32(attr(s, "force")),
                direction: try_parse_f32(attr(s, "direction")),
                gravity: try_parse_f32(attr(s, "gravity")),
                air_friction: try_parse_f32(attr(s, "airfriction")),
                shape: attr(s, "shape").map(str::to_string),
                energy: try_parse_f32(attr(s, "energy")),
            }),
            particles: element(e, "particles").map(|particles| {
                elements(particles, "particle")
                    .map(|p| ParticleSystemParticle {
                        is_emitter: attr(p, "is_emitter") == Some("true"),
                        life_time: try_parse_int(attr(p, "lifetime")),
                        fade: attr(p, "fade") == Some("true"),
                        frames: elements(p, "frame")
                            .filter_map(|f| attr(f, "name"))
                            .filter(|n| !n.is_empty())
                            .map(str::to_string)
                            .collect(),
                    })
                    .collect()
            }),
        })
        .collect()
}

/// Parses `*_logic.bin` text (`XElement.Parse` + `LogicMapper.MapLogicXml`).
pub fn parse_logic_text(text: &str) -> anyhow::Result<AssetLogicData> {
    let doc = crate::xml::parse(text)?;
    Ok(map_logic_xml(doc.root_element()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nl(s: &str) -> String {
        s.replace('\n', habbo_core::paths::NEWLINE)
    }

    #[test]
    fn maps_logic_xml_to_json() {
        let xml = r#"<objectData type="chair">
  <model>
    <dimensions x="1" y="2" z="0.5" centerZ="1.25"/>
    <directions><direction id="2"/><direction id="4"/></directions>
  </model>
  <action link="http://x" startState="1"/>
  <mask type="door"/>
  <credits value="me"/>
  <sound><sample id="12" nopitch="true"/></sound>
  <planetsystem><object id="1" name="sun" radius="0.5" arcspeed="2" blend="1"/></planetsystem>
  <particlesystems>
    <particlesystem size="64" canvas_id="1" offset_y="0.25">
      <emitter id="0" name="e" sprite_id="3" burst_pulse="2">
        <simulation force="1.5" airfriction="0.02" shape="cone"/>
        <particles><particle is_emitter="false" lifetime="10" fade="true"><frame name="a"/><frame name=""/></particle></particles>
      </emitter>
    </particlesystem>
    <particlesystem/>
  </particlesystems>
  <customvars><variable name="v1"/><variable/></customvars>
</objectData>"#;
        let logic = parse_logic_text(xml).unwrap();
        let json = logic_object_json(Some(&logic), FloatStyle::Fixed).to_string_indented().unwrap();
        let expected = r#"{
  "model": {
    "dimensions": {
      "x": 1,
      "y": 2,
      "z": 0.5,
      "centerZ": 1.25
    },
    "directions": [
      2,
      4
    ]
  },
  "action": {
    "link": "http://x",
    "startState": 1
  },
  "maskType": "door",
  "credits": "me",
  "soundSample": {
    "id": 12,
    "noPitch": true
  },
  "planetSystems": [
    {
      "id": 1,
      "name": "sun",
      "radius": 0.5,
      "arcSpeed": 2,
      "blend": 1
    }
  ],
  "particleSystems": [
    {
      "size": 64,
      "canvasId": 1,
      "offsetY": 0.25,
      "emitters": [
        {
          "id": 0,
          "name": "e",
          "spriteId": 3,
          "burstPulse": 2,
          "simulation": {
            "force": 1.5,
            "airFriction": 0.02,
            "shape": "cone"
          },
          "particles": [
            {
              "isEmitter": false,
              "lifeTime": 10,
              "fade": true,
              "frames": [
                "a"
              ]
            }
          ]
        }
      ]
    }
  ],
  "customVars": {
    "variables": [
      "v1"
    ]
  }
}"#;
        assert_eq!(json, nl(expected));
    }

    #[test]
    fn missing_values_use_csharp_defaults() {
        let logic = parse_logic_text(r#"<objectData><model><dimensions x="NaN"/><directions/></model></objectData>"#).unwrap();
        let json = logic_object_json(Some(&logic), FloatStyle::Fixed).to_string_indented().unwrap();
        assert_eq!(
            json,
            nl("{\n  \"model\": {\n    \"dimensions\": {\n      \"x\": 0,\n      \"y\": 0,\n      \"z\": 0.000001\n    },\n    \"directions\": [\n      0\n    ]\n  }\n}")
        );
        assert_eq!(logic_object_json(None, FloatStyle::Fixed).to_string_indented().unwrap(), nl("{\n  \"model\": {}\n}"));
    }
}
