//! Debug session: drives a loaded game one command at a time over lines of
//! JSON, so an agent can pause, step, inspect, change and capture a running
//! game without restarting it.
//!
//! One request per line: `{"id": 1, "cmd": "step", "ticks": 10}`. One reply
//! per line: `{"id": 1, "ok": true, "tick": 10, "result": ...}` or
//! `{"id": 1, "ok": false, "tick": 10, "error": "..."}`. The game is paused
//! between commands; time passes only inside `step`.
//!
//! Commands: `step` (`ticks`, default 1), `get` (`entity`, `path`, default the
//! whole entity), `set` (`entity`, `path`, `value`), `press` / `release`
//! (`action`), `capture` (`path`, optional `size` `[w, h]`), `tick`, `quit`.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

use crate::rendering::capture::HeadlessCapture;
use crate::runtime::{FrameTime, RuntimeInput};
use crate::scenario::{assign, press, reflected, tick_delta, Assignment};
use crate::App;

/// Environment variable that makes a game build run a debug session on
/// standard input and output instead of opening a window.
pub const DEBUG_SESSION_ENV: &str = "RUSTING_DEBUG_SESSION";

struct Session<'app> {
    app: &'app mut App,
    /// Calls to `update` so far; the first is tick 0 with no time passing.
    updates: u32,
    capture: Option<HeadlessCapture>,
}

impl Session<'_> {
    fn tick(&self) -> u64 {
        self.app.world().resource::<FrameTime>().fixed_tick
    }

    fn step(&mut self, ticks: u32) -> Result<Value, String> {
        for _ in 0..ticks {
            let delta = tick_delta(self.app, self.updates);
            match self.capture.as_mut() {
                Some(capture) => capture.frame(self.app, delta),
                None => self
                    .app
                    .update(delta)
                    .map(drop)
                    .map_err(|error| error.to_string()),
            }?;
            self.updates += 1;
            self.app
                .world_mut()
                .resource_mut::<RuntimeInput>()
                .clear_frame_edges();
        }
        Ok(Value::Null)
    }

    fn capture(
        &mut self,
        path: PathBuf,
        size: [u32; 2],
    ) -> Result<Value, String> {
        if self.capture.is_none() {
            self.capture = Some(HeadlessCapture::new(size)?);
            // Render the paused state once; no time passes.
            let capture = self.capture.as_mut().expect("just set");
            capture.frame(self.app, Duration::ZERO)?;
        }
        let capture = self.capture.as_ref().expect("set above");
        capture.save(&path)?;
        Ok(json!({"path": path, "size": size}))
    }

    fn run(&mut self, request: &Value) -> Result<Value, String> {
        let text = |key: &str| {
            request[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("`{key}` is required"))
        };
        match request["cmd"].as_str().unwrap_or_default() {
            "tick" => Ok(Value::Null),
            "step" => {
                let ticks = request["ticks"].as_u64().unwrap_or(1);
                self.step(u32::try_from(ticks).map_err(|_| "too many ticks")?)
            }
            "get" => {
                let state = reflected(self.app.world_mut(), &text("entity")?)?;
                let path = request["path"].as_str().unwrap_or_default();
                state
                    .pointer(path)
                    .cloned()
                    .ok_or_else(|| format!("`{path}` does not exist"))
            }
            "set" => {
                let set = Assignment {
                    entity: text("entity")?,
                    counter: None,
                    path: text("path")?,
                    value: request["value"].clone(),
                };
                assign(self.app.world_mut(), &set).map(Value::String)
            }
            cmd @ ("press" | "release") => press(
                self.app.world_mut(),
                &text("action")?,
                cmd == "press",
                None,
            )
            .map(Value::String),
            "capture" => {
                let size = request["size"]
                    .as_array()
                    .and_then(|size| {
                        Some([size.first()?.as_u64()?, size.get(1)?.as_u64()?])
                    })
                    .map_or(Some([1280, 720]), |size| {
                        Some([
                            u32::try_from(size[0]).ok()?,
                            u32::try_from(size[1]).ok()?,
                        ])
                    })
                    .filter(|size| !size.contains(&0))
                    .ok_or("`size` must be [width, height] of at least 1")?;
                self.capture(PathBuf::from(text("path")?), size)
            }
            other => Err(format!("unknown command `{other}`")),
        }
    }
}

/// Reads requests from `input` until `quit` or end of input, writing one
/// reply per request to `output`.
pub fn run_debug_session(
    app: &mut App,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    let mut session = Session {
        app,
        updates: 0,
        capture: None,
    };
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<Value>(&line);
        let (id, outcome, quit) = match &parsed {
            Err(error) => {
                (Value::Null, Err(format!("parse error: {error}")), false)
            }
            Ok(request) if request["cmd"] == "quit" => {
                (request["id"].clone(), Ok(Value::Null), true)
            }
            Ok(request) => (request["id"].clone(), session.run(request), false),
        };
        let tick = session.tick();
        // Everything but a read or a capture moves or edits the live game.
        let game = outcome.is_ok()
            && matches!(
                parsed.as_ref().ok().and_then(|r| r["cmd"].as_str()),
                Some("tick" | "step" | "set" | "press" | "release")
            );
        let touched = json!({"disk": false, "editor": false, "game": game});
        let reply = match outcome {
            Ok(result) => {
                json!({"id": id, "ok": true, "tick": tick, "touched": touched, "result": result})
            }
            Err(error) => {
                json!({"id": id, "ok": false, "tick": tick, "touched": touched, "error": error})
            }
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
        if quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{
        Collider, ColliderShape, HybridPhysicsPlugin, Name, PhysicsBody,
        RenderExtractPlugin, RigidBody, RigidBodyKind, SceneId,
    };
    use crate::Transform;
    use uuid::Uuid;

    fn talk(requests: &[&str]) -> Vec<Value> {
        let mut app = App::new();
        app.add_plugin(crate::AssetPlugin).unwrap();
        app.add_plugin(HybridPhysicsPlugin).unwrap();
        app.add_plugin(RenderExtractPlugin).unwrap();
        app.world_mut().spawn((
            SceneId(Uuid::new_v4()),
            Name("Cube".into()),
            Transform::new([0.0, 5.0, 0.0]),
            PhysicsBody::default(),
            RigidBody {
                kind: RigidBodyKind::Dynamic,
                ..RigidBody::default()
            },
            Collider {
                shape: ColliderShape::Box {
                    half_extents: [0.5; 3],
                },
                ..Collider::default()
            },
        ));
        let mut output = Vec::new();
        run_debug_session(
            &mut app,
            requests.join("\n").as_bytes(),
            &mut output,
        )
        .unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn the_game_is_paused_between_commands_and_steps_exactly() {
        let y = r#""entity": "Cube", "path": "/transform/position/1""#;
        let replies = talk(&[
            &format!(r#"{{"id": 1, "cmd": "get", {y}}}"#),
            r#"{"id": 2, "cmd": "step", "ticks": 30}"#,
            &format!(r#"{{"id": 3, "cmd": "get", {y}}}"#),
            &format!(r#"{{"id": 4, "cmd": "get", {y}}}"#),
            r#"{"id": 5, "cmd": "set", "entity": "Cube", "path": "/transform/position", "value": [0.0, 50.0, 0.0]}"#,
            &format!(r#"{{"id": 6, "cmd": "get", {y}}}"#),
            r#"{"id": 7, "cmd": "get", "entity": "Nobody"}"#,
            r#"{"id": 8, "cmd": "dance"}"#,
            "oops",
            r#"{"id": 9, "cmd": "quit"}"#,
            r#"{"id": 10, "cmd": "tick"}"#,
        ]);
        assert_eq!(replies.len(), 10, "{replies:?}");
        assert_eq!(replies[0]["result"], 5.0);
        assert_eq!(replies[1]["tick"], 29);
        let fallen = replies[2]["result"].as_f64().unwrap();
        assert!(fallen < 5.0, "{fallen}");
        // Paused: reading again changes nothing.
        assert_eq!(replies[3]["result"], replies[2]["result"]);
        assert_eq!(replies[5]["result"], 50.0);
        for bad in [6, 7, 8] {
            assert_eq!(replies[bad]["ok"], false, "{:?}", replies[bad]);
        }
        assert_eq!(replies[9]["id"], 9);
        let game = |i: usize| replies[i]["touched"]["game"].as_bool();
        assert_eq!(
            [game(0), game(1), game(4), game(6)],
            [Some(false), Some(true), Some(true), Some(false)],
            "a read touches nothing, a step or set touches the game, an error nothing"
        );
        assert_eq!(replies[1]["touched"]["disk"], false);
    }
}
