//! Project-level determinism opt-in. A project picks a
//! [`DeterminismMode`] in `project.json`; `cook` copies it into the cooked
//! scene, and loading that scene inserts it as a resource. Every simulation
//! part (a solver, a gameplay system, a compute shader) states the strongest
//! mode it supports, and [`check_determinism`] rejects a run whose parts fall
//! short, naming each one.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use bevy_ecs::prelude::{Resource, World};
use serde::{Deserialize, Serialize};

use super::{PhysicsBody, PhysicsSolver, SimulationClass};

/// How strictly the simulation must reproduce itself. Ordered from weakest
/// to strongest, so a part that supports `CrossPlatform` also supports
/// `Local`.
#[derive(
    Resource,
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
pub enum DeterminismMode {
    /// No guarantee; nothing pays for fixed-order work.
    #[default]
    Off,
    /// Identical results on one machine and build.
    Local,
    /// Identical results on every supported device.
    CrossPlatform,
}

impl DeterminismMode {
    /// Parses the name used in `project.json` and custom shader pragmas,
    /// ignoring case.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "local" => Some(Self::Local),
            "crossplatform" | "cross_platform" | "cross-platform" => {
                Some(Self::CrossPlatform)
            }
            _ => None,
        }
    }
}

/// Built-in CPU solver name in [`DeterminismSupport`].
pub const CPU_PHYSICS_PART: &str = "cpu_physics";

/// Strongest mode each simulation part supports, keyed by part name. Engine
/// plugins declare their parts; games declare their own simulation systems
/// with [`DeterminismSupport::declare`]. Parts used by the scene's bodies
/// are added by [`check_determinism`].
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct DeterminismSupport {
    pub parts: BTreeMap<String, DeterminismMode>,
}

impl DeterminismSupport {
    /// Records that `part` supports up to `mode`. A second declaration of
    /// the same part keeps the weaker mode.
    pub fn declare(&mut self, part: impl Into<String>, mode: DeterminismMode) {
        self.parts
            .entry(part.into())
            .and_modify(|existing| *existing = (*existing).min(mode))
            .or_insert(mode);
    }
}

/// One part that does not support the selected mode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeterminismOffender {
    pub part: String,
    pub supports: DeterminismMode,
}

/// The selected mode and every part that falls short of it, sorted by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeterminismError {
    pub mode: DeterminismMode,
    pub offenders: Vec<DeterminismOffender>,
}

impl fmt::Display for DeterminismError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "determinism mode {:?} is not supported by",
            self.mode
        )?;
        for (index, offender) in self.offenders.iter().enumerate() {
            let separator = if index == 0 { " " } else { ", " };
            write!(
                formatter,
                "{separator}{} (supports {:?})",
                offender.part, offender.supports
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for DeterminismError {}

/// Part name and supported mode for a body's physics route, or `None` for
/// bodies that do not simulate.
///
/// Built-in solvers support `Local`: they iterate in body order and use no
/// order-dependent atomics, but their float math is not pinned across
/// devices yet. A custom shader states its mode on a line of its own,
/// `// rusting: determinism = local`; without one it supports `Off`.
#[must_use]
pub fn body_part(
    body: &PhysicsBody,
    project_root: Option<&Path>,
) -> Option<(String, DeterminismMode)> {
    let local = DeterminismMode::Local;
    match body.simulation {
        SimulationClass::None | SimulationClass::Static => None,
        SimulationClass::Cpu => Some((CPU_PHYSICS_PART.into(), local)),
        SimulationClass::Gpu => {
            Some(match (body.solver, &body.custom_shader) {
                (PhysicsSolver::Custom, Some(path)) => {
                    let source = project_root
                        .map_or_else(|| path.into(), |root| root.join(path));
                    let mode = std::fs::read_to_string(source)
                        .ok()
                        .and_then(|text| shader_determinism(&text))
                        .unwrap_or_default();
                    (format!("gpu_shader:{path}"), mode)
                }
                (solver, _) => (format!("gpu_solver:{solver:?}"), local),
            })
        }
    }
}

/// Reads the `// rusting: determinism = <mode>` pragma from shader source.
#[must_use]
pub fn shader_determinism(source: &str) -> Option<DeterminismMode> {
    source.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("//")?.trim();
        let rest = rest.strip_prefix("rusting:")?.trim();
        let value = rest.strip_prefix("determinism")?.trim();
        DeterminismMode::parse(value.strip_prefix('=')?.trim())
    })
}

/// Fails when a declared part or a part used by a scene body supports less
/// than `mode`. `Off` always passes.
pub fn check_parts(
    mode: DeterminismMode,
    parts: &BTreeMap<String, DeterminismMode>,
) -> Result<(), DeterminismError> {
    let offenders: Vec<_> = parts
        .iter()
        .filter(|(_, supports)| **supports < mode)
        .map(|(part, supports)| DeterminismOffender {
            part: part.clone(),
            supports: *supports,
        })
        .collect();
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(DeterminismError { mode, offenders })
    }
}

/// Startup check for a loaded world: the [`DeterminismMode`] resource
/// against [`DeterminismSupport`] plus every simulating body's part.
/// Relative custom shader paths resolve against the working directory.
pub fn check_determinism(world: &mut World) -> Result<(), DeterminismError> {
    let mode = world
        .get_resource::<DeterminismMode>()
        .copied()
        .unwrap_or_default();
    if mode == DeterminismMode::Off {
        return Ok(());
    }
    let mut support = world
        .get_resource::<DeterminismSupport>()
        .cloned()
        .unwrap_or_default();
    let mut bodies = world.query::<&PhysicsBody>();
    for body in bodies.iter(world) {
        if let Some((part, supports)) = body_part(body, None) {
            support.declare(part, supports);
        }
    }
    check_parts(mode, &support.parts)
}
