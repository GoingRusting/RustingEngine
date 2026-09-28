use std::path::PathBuf;
use std::time::Duration;

use super::*;

#[derive(
    Component, Clone, Debug, Default, Serialize, Deserialize, PartialEq,
)]
struct Health {
    points: i32,
}

rusting_engine::reflect! {
    struct Health {
        points: i32,
    }
}

/// Imports and data every test script shares. Offsets: 0 "transform",
/// 16 "/position/0", 32 "5.0", 48 "Target", 64 "test.health",
/// 80 "/points", 96 "-3", 112 "no.such", 128 scratch buffer.
const PRELUDE: &str = r#"
  (import "rusting" "entity" (func $entity (result i64)))
  (import "rusting" "find" (func $find (param i32 i32) (result i64)))
  (import "rusting" "get"
    (func $get (param i64 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "rusting" "set"
    (func $set (param i64 i32 i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "transform")
  (data (i32.const 16) "/position/0")
  (data (i32.const 32) "5.0")
  (data (i32.const 48) "Target")
  (data (i32.const 64) "test.health")
  (data (i32.const 80) "/points")
  (data (i32.const 96) "-3")
  (data (i32.const 112) "no.such")
"#;

struct Game {
    app: App,
    folder: PathBuf,
}

impl Game {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugin(ScriptPlugin).unwrap();
        app.register_scene_component::<Health>("test.health")
            .unwrap();
        let folder = std::env::temp_dir()
            .join(format!("rusting-script-{}", uuid_like()));
        std::fs::create_dir_all(&folder).unwrap();
        Self { app, folder }
    }

    /// Writes a text module with [`PRELUDE`] and `body`.
    fn module(&self, file: &str, body: &str) -> PathBuf {
        let path = self.folder.join(file);
        std::fs::write(&path, format!("(module {PRELUDE} {body})")).unwrap();
        path
    }

    fn spawn(
        &mut self,
        name: &str,
        points: i32,
        module: Option<PathBuf>,
    ) -> Entity {
        let entity = self.app.spawn((
            Name(name.into()),
            Transform::default(),
            Health { points },
        ));
        if let Some(module) = module {
            self.app
                .world_mut()
                .entity_mut(entity)
                .insert(Script { module });
        }
        entity
    }

    fn frame(&mut self) {
        self.app.update(Duration::from_millis(16)).unwrap();
    }

    fn points(&self, entity: Entity) -> i32 {
        self.app.world().get::<Health>(entity).unwrap().points
    }

    fn x(&self, entity: Entity) -> f32 {
        self.app.world().get::<Transform>(entity).unwrap().position[0]
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[test]
fn scripts_read_and_write_reflected_fields() {
    let mut game = Game::new();
    // `start` moves its own object; `update` copies the target's points
    // into its own through JSON, as a designer script would.
    let module = game.module(
        "copy.wat",
        r#"
        (func (export "start")
          (drop (call $set (call $entity)
            (i32.const 0) (i32.const 9) (i32.const 16) (i32.const 11)
            (i32.const 32) (i32.const 3))))
        (func (export "update") (param $dt f32) (local $len i32)
          (local.set $len (call $get (call $find (i32.const 48) (i32.const 6))
            (i32.const 64) (i32.const 11) (i32.const 80) (i32.const 7)
            (i32.const 128) (i32.const 64)))
          (drop (call $set (call $entity)
            (i32.const 64) (i32.const 11) (i32.const 80) (i32.const 7)
            (i32.const 128) (local.get $len))))
        "#,
    );
    game.spawn("Target", 42, None);
    let scripted = game.spawn("Scripted", 0, Some(module));
    game.frame();
    assert_eq!(game.x(scripted), 5.0);
    assert_eq!(game.points(scripted), 42);
}

#[test]
fn bad_requests_return_a_status_instead_of_stopping_the_script() {
    let mut game = Game::new();
    // An unknown component and a string into an integer field both return
    // INVALID, which this script records as -3 points.
    let module = game.module(
        "bad.wat",
        r#"
        (func (export "update") (param $dt f32)
          (if (i32.eq (i32.const -3) (call $set (call $entity)
                (i32.const 112) (i32.const 7) (i32.const 80) (i32.const 7)
                (i32.const 96) (i32.const 2)))
            (then (if (i32.eq (i32.const -3) (call $set (call $entity)
                    (i32.const 64) (i32.const 11) (i32.const 80) (i32.const 7)
                    (i32.const 48) (i32.const 6)))
              (then (drop (call $set (call $entity)
                (i32.const 64) (i32.const 11) (i32.const 80) (i32.const 7)
                (i32.const 96) (i32.const 2))))))))
        "#,
    );
    let scripted = game.spawn("Scripted", 0, Some(module));
    game.frame();
    assert_eq!(game.points(scripted), -3);
    // A missing entity is its own status.
    let missing = game.module(
        "missing.wat",
        r#"
        (func (export "update") (param $dt f32)
          (if (i32.eq (i32.const -1) (call $get (call $find (i32.const 48) (i32.const 6))
                (i32.const 64) (i32.const 11) (i32.const 80) (i32.const 7)
                (i32.const 128) (i32.const 64)))
            (then (drop (call $set (call $entity)
              (i32.const 0) (i32.const 9) (i32.const 16) (i32.const 11)
              (i32.const 32) (i32.const 3))))))
        "#,
    );
    game.app
        .world_mut()
        .get_mut::<Script>(scripted)
        .unwrap()
        .module = missing;
    game.frame();
    assert_eq!(game.x(scripted), 5.0, "the new module ran");
}

#[test]
fn sandbox_limits_stop_only_the_offending_script() {
    let mut game = Game::new();
    let endless = game.module(
        "endless.wat",
        r#"(func (export "update") (param $dt f32) (loop $spin (br $spin)))"#,
    );
    // 1024 pages is 64 MiB, over the 16 MiB limit, so it never starts.
    let greedy = game.folder.join("greedy.wat");
    std::fs::write(
        &greedy,
        r#"(module
          (import "rusting" "entity" (func $entity (result i64)))
          (memory (export "memory") 1024)
          (func (export "start") (drop (call $entity))))"#,
    )
    .unwrap();
    let working = game.module(
        "working.wat",
        r#"
        (func (export "update") (param $dt f32)
          (drop (call $set (call $entity)
            (i32.const 0) (i32.const 9) (i32.const 16) (i32.const 11)
            (i32.const 32) (i32.const 3))))
        "#,
    );
    let broken = game.folder.join("broken.wasm");
    std::fs::write(&broken, b"not wasm").unwrap();
    game.spawn("Endless", 0, Some(endless));
    game.spawn("Greedy", 0, Some(greedy));
    game.spawn("Broken", 0, Some(broken));
    game.spawn("Unreadable", 0, Some(game.folder.join("absent.wasm")));
    let fine = game.spawn("Fine", 0, Some(working));
    game.frame();
    game.frame();
    assert_eq!(game.x(fine), 5.0);
    let host = game.app.world().resource::<ScriptHost>();
    let mut failed: Vec<_> = host
        .scripts
        .values()
        .map(|script| {
            (script.module.file_name().unwrap().to_owned(), script.failed)
        })
        .collect();
    failed.sort();
    assert_eq!(
        failed,
        [
            ("absent.wasm".into(), true),
            ("broken.wasm".into(), true),
            ("endless.wat".into(), true),
            ("greedy.wat".into(), true),
            ("working.wat".into(), false),
        ]
    );
}

#[test]
fn removed_scripts_are_dropped() {
    let mut game = Game::new();
    let module = game.module("empty.wat", "");
    let scripted = game.spawn("Scripted", 0, Some(module));
    game.frame();
    assert_eq!(game.app.world().resource::<ScriptHost>().scripts.len(), 1);
    game.app.world_mut().entity_mut(scripted).remove::<Script>();
    game.frame();
    assert!(game.app.world().resource::<ScriptHost>().scripts.is_empty());
}
