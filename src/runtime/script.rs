//! Small compiled gameplay scripting layer.
//!
//! Editor source is human-readable `.rscript`. Cooking embeds this compact
//! instruction representation in the scene so release builds do not parse
//! source files in the frame loop.

use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, Resource, World};
use serde::{Deserialize, Serialize};

use crate::Transform;

use super::{App, AppError, FrameTime, Name, Plugin, ScheduleStage};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CompiledScript {
    pub bindings: Vec<ScriptBinding>,
    pub on_start: Vec<ScriptInstruction>,
    pub on_update: Vec<ScriptInstruction>,
    pub on_fixed_update: Vec<ScriptInstruction>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptBinding {
    pub variable: String,
    pub object_name: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransformField {
    PositionX,
    PositionY,
    PositionZ,
    RotationX,
    RotationY,
    RotationZ,
    ScaleX,
    ScaleY,
    ScaleZ,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScriptOperation {
    Set,
    Add,
    Subtract,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScriptValue {
    pub constant: f32,
    pub delta_factor: f32,
}

impl ScriptValue {
    fn evaluate(self, delta: f32) -> f32 {
        self.constant + self.delta_factor * delta
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScriptInstruction {
    pub binding: u16,
    pub field: TransformField,
    pub operation: ScriptOperation,
    pub value: ScriptValue,
}

#[derive(Component, Clone, Debug, PartialEq)]
pub struct ScriptComponent {
    pub source_path: String,
    pub enabled: bool,
    pub compiled: Option<CompiledScript>,
}

impl Default for ScriptComponent {
    fn default() -> Self {
        Self {
            source_path: "testGame/scripts/main.rscript".into(),
            enabled: true,
            compiled: None,
        }
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptSettings {
    pub enabled: bool,
}

impl Default for ScriptSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Resource, Default)]
pub struct ScriptRuntime {
    started: HashSet<Entity>,
    bindings: HashMap<(Entity, u16), Entity>,
    errors: HashMap<Entity, String>,
}

impl ScriptRuntime {
    pub fn reset(&mut self) {
        self.started.clear();
        self.bindings.clear();
        self.errors.clear();
    }

    #[must_use]
    pub fn errors(&self) -> &HashMap<Entity, String> {
        &self.errors
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptCompileError {
    pub line: usize,
    pub message: String,
}

impl Display for ScriptCompileError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ScriptCompileError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.insert_resource(ScriptSettings::default())
            .insert_resource(ScriptRuntime::default())
            .add_systems(ScheduleStage::FixedUpdate, run_fixed_scripts)
            .add_systems(ScheduleStage::Update, run_update_scripts);
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Lifecycle {
    Start,
    Update,
    FixedUpdate,
}

#[must_use = "compiled scripts should be stored in a scene or ScriptComponent"]
pub fn compile_script(
    source: &str,
) -> Result<CompiledScript, ScriptCompileError> {
    let normalized = source
        .replace('{', "{\n")
        .replace('}', "\n}\n")
        .replace(';', ";\n");
    let mut bindings = Vec::<ScriptBinding>::new();
    let mut on_start = Vec::new();
    let mut on_update = Vec::new();
    let mut on_fixed_update = Vec::new();
    let mut lifecycle = None;

    for (line_index, raw_line) in normalized.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line
            .split_once("//")
            .map_or(raw_line, |(code, _)| code)
            .trim();
        if line.is_empty() || line == "{" {
            continue;
        }
        if line == "}" {
            lifecycle = None;
            continue;
        }
        if line.starts_with("let ") {
            bindings.push(parse_binding(line, line_number)?);
            continue;
        }

        let function_name = line.trim_end_matches('{').trim();
        lifecycle = match function_name {
            "onSceneStart()" | "on_scene_start()" => Some(Lifecycle::Start),
            "onSceneUpdate()" | "on_scene_update()" => Some(Lifecycle::Update),
            "onFixedUpdate()" | "on_fixed_update()" => {
                Some(Lifecycle::FixedUpdate)
            }
            _ => lifecycle,
        };
        if matches!(
            function_name,
            "onSceneStart()"
                | "on_scene_start()"
                | "onSceneUpdate()"
                | "on_scene_update()"
                | "onFixedUpdate()"
                | "on_fixed_update()"
        ) {
            continue;
        }

        let lifecycle = lifecycle.ok_or_else(|| ScriptCompileError {
            line: line_number,
            message: "statement is outside a lifecycle function".into(),
        })?;
        let instruction = parse_instruction(
            line.trim_end_matches(';'),
            &bindings,
            line_number,
        )?;
        match lifecycle {
            Lifecycle::Start => on_start.push(instruction),
            Lifecycle::Update => on_update.push(instruction),
            Lifecycle::FixedUpdate => on_fixed_update.push(instruction),
        }
    }

    Ok(CompiledScript {
        bindings,
        on_start,
        on_update,
        on_fixed_update,
    })
}

fn parse_binding(
    line: &str,
    line_number: usize,
) -> Result<ScriptBinding, ScriptCompileError> {
    let (variable, expression) = line
        .trim_start_matches("let ")
        .trim_end_matches(';')
        .split_once('=')
        .ok_or_else(|| {
            compile_error(line_number, "expected `let name = ...`")
        })?;
    let variable = variable.trim();
    if variable.is_empty()
        || !variable.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_'
        })
    {
        return Err(compile_error(line_number, "invalid variable name"));
    }
    let expression = expression.trim();
    let argument = expression
        .strip_prefix("scene.get_object(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| {
            compile_error(
                line_number,
                "expected scene.get_object(\"Object Name\")",
            )
        })?
        .trim();
    let object_name = argument
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| {
            compile_error(line_number, "object name must be quoted")
        })?;
    Ok(ScriptBinding {
        variable: variable.into(),
        object_name: object_name.into(),
    })
}

fn parse_instruction(
    line: &str,
    bindings: &[ScriptBinding],
    line_number: usize,
) -> Result<ScriptInstruction, ScriptCompileError> {
    let (left, operation, right) =
        if let Some((left, right)) = line.split_once("+=") {
            (left, ScriptOperation::Add, right)
        } else if let Some((left, right)) = line.split_once("-=") {
            (left, ScriptOperation::Subtract, right)
        } else if let Some((left, right)) = line.split_once('=') {
            (left, ScriptOperation::Set, right)
        } else {
            return Err(compile_error(
                line_number,
                "expected an assignment using =, +=, or -=",
            ));
        };
    let (variable, property) =
        left.trim().split_once('.').ok_or_else(|| {
            compile_error(line_number, "expected object.property")
        })?;
    let binding = bindings
        .iter()
        .position(|binding| binding.variable == variable)
        .ok_or_else(|| compile_error(line_number, "unknown object variable"))?;
    let binding = u16::try_from(binding)
        .map_err(|_| compile_error(line_number, "too many object bindings"))?;
    let field = parse_transform_field(property.trim()).ok_or_else(|| {
        compile_error(line_number, "unknown transform property")
    })?;
    let value = parse_value(right.trim(), line_number)?;
    Ok(ScriptInstruction {
        binding,
        field,
        operation,
        value,
    })
}

fn parse_transform_field(property: &str) -> Option<TransformField> {
    match property {
        "x" | "position.x" => Some(TransformField::PositionX),
        "y" | "position.y" => Some(TransformField::PositionY),
        "z" | "position.z" => Some(TransformField::PositionZ),
        "rotation.x" => Some(TransformField::RotationX),
        "rotation.y" => Some(TransformField::RotationY),
        "rotation.z" => Some(TransformField::RotationZ),
        "scale.x" => Some(TransformField::ScaleX),
        "scale.y" => Some(TransformField::ScaleY),
        "scale.z" => Some(TransformField::ScaleZ),
        _ => None,
    }
}

fn parse_value(
    expression: &str,
    line_number: usize,
) -> Result<ScriptValue, ScriptCompileError> {
    let compact = expression.replace(' ', "");
    if compact == "delta" {
        return Ok(ScriptValue {
            constant: 0.0,
            delta_factor: 1.0,
        });
    }
    if let Some((left, right)) = compact.split_once('*') {
        let factor = if left == "delta" {
            right.parse::<f32>()
        } else if right == "delta" {
            left.parse::<f32>()
        } else {
            return Err(compile_error(
                line_number,
                "multiplication currently requires delta",
            ));
        }
        .map_err(|_| compile_error(line_number, "invalid numeric value"))?;
        return Ok(ScriptValue {
            constant: 0.0,
            delta_factor: factor,
        });
    }
    let constant = compact
        .parse::<f32>()
        .map_err(|_| compile_error(line_number, "invalid numeric value"))?;
    Ok(ScriptValue {
        constant,
        delta_factor: 0.0,
    })
}

fn compile_error(line: usize, message: &str) -> ScriptCompileError {
    ScriptCompileError {
        line,
        message: message.into(),
    }
}

fn run_update_scripts(world: &mut World) {
    let delta = world.resource::<FrameTime>().delta.as_secs_f32();
    if delta > 0.0 {
        run_scripts(world, Lifecycle::Update, delta);
    }
}

fn run_fixed_scripts(world: &mut World) {
    let delta = world.resource::<FrameTime>().fixed_delta.as_secs_f32();
    run_scripts(world, Lifecycle::FixedUpdate, delta);
}

fn run_scripts(world: &mut World, lifecycle: Lifecycle, delta: f32) {
    if !world.resource::<ScriptSettings>().enabled {
        return;
    }
    let mut runtime =
        world.remove_resource::<ScriptRuntime>().unwrap_or_default();
    let mut name_query = world.query::<(Entity, &Name)>();
    let mut objects = name_query
        .iter(world)
        .map(|(entity, name)| (name.0.clone(), entity))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(_, entity)| entity.to_bits());
    let objects = objects.into_iter().collect::<HashMap<_, _>>();

    let mut script_query = world.query::<(Entity, &ScriptComponent)>();
    let scripts = script_query
        .iter(world)
        .filter(|(_, script)| script.enabled)
        .map(|(entity, script)| (entity, script.clone()))
        .collect::<Vec<_>>();
    let live_scripts = scripts
        .iter()
        .map(|(entity, _)| *entity)
        .collect::<HashSet<_>>();
    runtime
        .started
        .retain(|entity| live_scripts.contains(entity));
    runtime
        .bindings
        .retain(|(entity, _), _| live_scripts.contains(entity));
    runtime
        .errors
        .retain(|entity, _| live_scripts.contains(entity));

    for (script_entity, mut component) in scripts {
        let compiled = match component.compiled.clone() {
            Some(compiled) => compiled,
            None => match std::fs::read_to_string(&component.source_path)
                .map_err(|error| error.to_string())
                .and_then(|source| {
                    compile_script(&source).map_err(|error| error.to_string())
                }) {
                Ok(compiled) => {
                    component.compiled = Some(compiled.clone());
                    world.entity_mut(script_entity).insert(component);
                    compiled
                }
                Err(error) => {
                    runtime.errors.insert(script_entity, error);
                    continue;
                }
            },
        };
        runtime.errors.remove(&script_entity);
        if runtime.started.insert(script_entity) {
            execute_instructions(
                world,
                script_entity,
                &compiled,
                &compiled.on_start,
                0.0,
                &objects,
                &mut runtime,
            );
        }
        let instructions = match lifecycle {
            Lifecycle::Start => &compiled.on_start,
            Lifecycle::Update => &compiled.on_update,
            Lifecycle::FixedUpdate => &compiled.on_fixed_update,
        };
        execute_instructions(
            world,
            script_entity,
            &compiled,
            instructions,
            delta,
            &objects,
            &mut runtime,
        );
    }
    world.insert_resource(runtime);
}

#[allow(clippy::too_many_arguments)]
fn execute_instructions(
    world: &mut World,
    script_entity: Entity,
    compiled: &CompiledScript,
    instructions: &[ScriptInstruction],
    delta: f32,
    objects: &HashMap<String, Entity>,
    runtime: &mut ScriptRuntime,
) {
    for instruction in instructions {
        let key = (script_entity, instruction.binding);
        let target = runtime
            .bindings
            .get(&key)
            .copied()
            .filter(|entity| world.get_entity(*entity).is_ok());
        let target = target.or_else(|| {
            let binding =
                compiled.bindings.get(usize::from(instruction.binding))?;
            objects.get(&binding.object_name).copied()
        });
        let Some(target) = target else {
            runtime.errors.insert(
                script_entity,
                format!(
                    "object `{}` was not found",
                    compiled.bindings[usize::from(instruction.binding)]
                        .object_name
                ),
            );
            continue;
        };
        runtime.bindings.insert(key, target);
        let Some(mut transform) = world.get_mut::<Transform>(target) else {
            runtime.errors.insert(
                script_entity,
                format!("target {target:?} has no Transform"),
            );
            continue;
        };
        let slot = transform_slot(&mut transform, instruction.field);
        let value = instruction.value.evaluate(delta);
        match instruction.operation {
            ScriptOperation::Set => *slot = value,
            ScriptOperation::Add => *slot += value,
            ScriptOperation::Subtract => *slot -= value,
        }
    }
}

fn transform_slot(
    transform: &mut Transform,
    field: TransformField,
) -> &mut f32 {
    match field {
        TransformField::PositionX => &mut transform.position[0],
        TransformField::PositionY => &mut transform.position[1],
        TransformField::PositionZ => &mut transform.position[2],
        TransformField::RotationX => &mut transform.rotation[0],
        TransformField::RotationY => &mut transform.rotation[1],
        TransformField::RotationZ => &mut transform.rotation[2],
        TransformField::ScaleX => &mut transform.scale[0],
        TransformField::ScaleY => &mut transform.scale[1],
        TransformField::ScaleZ => &mut transform.scale[2],
    }
}

pub fn invalidate_script(world: &mut World, path: &str) {
    let mut query = world.query::<&mut ScriptComponent>();
    for mut script in query.iter_mut(world) {
        if script.source_path == path {
            script.compiled = None;
        }
    }
    world.resource_mut::<ScriptRuntime>().reset();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const SCRIPT: &str = r#"
let orange = scene.get_object("Orange Cube");

onSceneStart() {
    orange.x = 5;
}

onSceneUpdate() {
    orange.rotation.y += 2 * delta;
}
"#;

    #[test]
    fn compiler_builds_bindings_and_lifecycle_instructions() {
        let compiled = compile_script(SCRIPT).unwrap();
        assert_eq!(compiled.bindings[0].object_name, "Orange Cube");
        assert_eq!(compiled.on_start.len(), 1);
        assert_eq!(compiled.on_update.len(), 1);
        assert_eq!(compiled.on_update[0].value.delta_factor, 2.0);
    }

    #[test]
    fn compiled_script_mutates_named_scene_object() {
        let mut app = App::new();
        app.add_plugin(ScriptPlugin).unwrap();
        let orange =
            app.spawn((Name("Orange Cube".into()), Transform::default()));
        app.spawn(ScriptComponent {
            source_path: "unused.rscript".into(),
            enabled: true,
            compiled: Some(compile_script(SCRIPT).unwrap()),
        });

        app.update(Duration::from_millis(500)).unwrap();
        let transform = app.world().get::<Transform>(orange).unwrap();
        assert_eq!(transform.position[0], 5.0);
        assert!((transform.rotation[1] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn compiler_rejects_unknown_objects_and_properties() {
        assert!(compile_script("onSceneUpdate() { missing.x = 1; }").is_err());
        assert!(compile_script(
            "let cube = scene.get_object(\"Cube\");\nonSceneUpdate() { cube.color = 1; }"
        )
        .is_err());
    }
}
