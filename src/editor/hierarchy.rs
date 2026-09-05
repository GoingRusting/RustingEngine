//! Parent-first ordering used by the editor Hierarchy area.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::World;

use crate::runtime::{Camera, Collider, PhysicsBody, RigidBody};
use crate::runtime::{Name, Parent};
use crate::Transform;

use super::{gui_elements, EditorState, EntityRequest};

#[derive(Clone, Copy)]
struct HierarchyDrag(Entity);

/// One visible row in the expanded Hierarchy tree.
pub(super) struct HierarchyItem {
    pub(super) entity: Entity,
    pub(super) name: String,
    pub(super) depth: usize,
    pub(super) transform: Option<Transform>,
}

struct RawHierarchyItem {
    entity: Entity,
    name: String,
    parent: Option<Entity>,
    transform: Option<Transform>,
}

/// Builds a parent-first tree where every child directly follows its parent.
pub(super) fn collect_entities(world: &mut World) -> Vec<HierarchyItem> {
    let mut query = world.query::<(
        Entity,
        Option<&Name>,
        Option<&Parent>,
        Option<&Transform>,
    )>();
    let mut raw = query
        .iter(world)
        .filter(|(_, name, parent, transform)| {
            name.is_some() || parent.is_some() || transform.is_some()
        })
        .map(|(entity, name, parent, transform)| RawHierarchyItem {
            entity,
            name: name.map_or_else(
                || format!("Entity {entity:?}"),
                |name| name.0.clone(),
            ),
            parent: parent.map(|parent| parent.0),
            transform: transform.copied(),
        })
        .collect::<Vec<_>>();
    // Sorting once gives every group of siblings a stable readable order.
    raw.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.entity.cmp(&right.entity))
    });

    let known = raw
        .iter()
        .map(|item| item.entity)
        .collect::<std::collections::HashSet<_>>();
    let mut children = std::collections::HashMap::<Entity, Vec<usize>>::new();
    let mut roots = Vec::new();
    for (index, item) in raw.iter().enumerate() {
        if let Some(parent) =
            item.parent.filter(|parent| known.contains(parent))
        {
            children.entry(parent).or_default().push(index);
        } else {
            roots.push(index);
        }
    }

    let mut visited = std::collections::HashSet::new();
    let mut ordered = Vec::with_capacity(raw.len());
    for root in roots {
        append_branch(&raw, &children, root, 0, &mut visited, &mut ordered);
    }
    // Runtime hierarchy validation rejects cycles, but this fallback keeps a
    // damaged external scene visible instead of silently dropping its rows.
    for index in 0..raw.len() {
        if !visited.contains(&raw[index].entity) {
            append_branch(
                &raw,
                &children,
                index,
                0,
                &mut visited,
                &mut ordered,
            );
        }
    }
    ordered
}

fn append_branch(
    raw: &[RawHierarchyItem],
    children: &std::collections::HashMap<Entity, Vec<usize>>,
    index: usize,
    depth: usize,
    visited: &mut std::collections::HashSet<Entity>,
    ordered: &mut Vec<HierarchyItem>,
) {
    let item = &raw[index];
    if !visited.insert(item.entity) {
        return;
    }
    ordered.push(HierarchyItem {
        entity: item.entity,
        name: item.name.clone(),
        depth,
        transform: item.transform,
    });
    if let Some(child_indexes) = children.get(&item.entity) {
        for child in child_indexes {
            append_branch(raw, children, *child, depth + 1, visited, ordered);
        }
    }
}

/// Draws object creation, selection actions, parenting, and the tree rows.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_hierarchy_area(
    ui: &mut egui::Ui,
    world: &World,
    entities: &[HierarchyItem],
    state: &mut EditorState,
    entity_request: &mut Option<EntityRequest>,
    edited_transform: &mut Option<Transform>,
    edited_camera: &mut Option<Camera>,
    edited_physics: &mut Option<PhysicsBody>,
    edited_rigid_body: &mut Option<RigidBody>,
    edited_collider: &mut Option<Collider>,
) {
    if state.rename_target.is_some_and(|target| {
        !entities.iter().any(|item| item.entity == target)
    }) {
        state.rename_target = None;
    }
    if gui_elements::EditorTheme::toolbar_icon_button(
        ui,
        "Add Object",
        super::EditorIcon::AddObject,
        120.0,
        true,
    )
    .on_hover_text("Open the object catalog")
    .clicked()
    {
        state.add_object_parent = None;
        state.add_object_modal_open = true;
    }
    ui.separator();
    egui::ScrollArea::both()
        .id_salt("hierarchy_panel_scroll")
        .auto_shrink([false, false])
        .scroll_bar_visibility(
            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
        )
        .show(ui, |ui| {
            for item in entities {
                if state.rename_target == Some(item.entity) {
                    draw_inline_rename(ui, item, state, entity_request);
                    continue;
                }
                let response = gui_elements::EditorTheme::tree_row(
                    ui,
                    &item.name,
                    item.depth,
                    state.selected == Some(item.entity),
                );
                response.dnd_set_drag_payload(HierarchyDrag(item.entity));
                if let Some(payload) =
                    response.dnd_hover_payload::<HierarchyDrag>()
                {
                    let valid = can_reparent(world, payload.0, item.entity);
                    ui.painter().rect_stroke(
                        response.rect,
                        5.0,
                        egui::Stroke::new(
                            2.0_f32,
                            if valid {
                                gui_elements::EditorTheme::ACCENT_HOVER
                            } else {
                                egui::Color32::LIGHT_RED
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(payload) =
                    response.dnd_release_payload::<HierarchyDrag>()
                {
                    if can_reparent(world, payload.0, item.entity) {
                        *entity_request = Some(EntityRequest::Reparent(
                            payload.0,
                            Some(item.entity),
                        ));
                    }
                }
                if response.clicked() || response.secondary_clicked() {
                    select_item(
                        world,
                        item,
                        state,
                        edited_transform,
                        edited_camera,
                        edited_physics,
                        edited_rigid_body,
                        edited_collider,
                    );
                }
                response.context_menu(|ui| {
                    ui.set_min_width(220.0);
                    gui_elements::EditorTheme::menu_section(ui, "OBJECT");
                    if gui_elements::EditorTheme::menu_action(
                        ui,
                        "Add Child Object...",
                        true,
                    )
                    .clicked()
                    {
                        state.add_object_parent = Some(item.entity);
                        state.add_object_modal_open = true;
                    }
                    if gui_elements::EditorTheme::menu_action(
                        ui, "Rename", true,
                    )
                    .clicked()
                    {
                        state.selected = Some(item.entity);
                        state.rename_draft = item.name.clone();
                        state.rename_target = Some(item.entity);
                    }
                    if gui_elements::EditorTheme::menu_action(
                        ui,
                        "Duplicate",
                        true,
                    )
                    .clicked()
                    {
                        *entity_request =
                            Some(EntityRequest::Duplicate(item.entity));
                    }
                    if world.get::<Parent>(item.entity).is_some()
                        && gui_elements::EditorTheme::menu_action(
                            ui,
                            "Move to Scene Root",
                            true,
                        )
                        .clicked()
                    {
                        *entity_request =
                            Some(EntityRequest::Reparent(item.entity, None));
                    }
                    gui_elements::EditorTheme::menu_section(ui, "DANGER");
                    if gui_elements::EditorTheme::menu_action(
                        ui, "Delete", true,
                    )
                    .clicked()
                    {
                        *entity_request =
                            Some(EntityRequest::Delete(item.entity));
                    }
                });
            }
        });
}

fn can_reparent(world: &World, child: Entity, parent: Entity) -> bool {
    if child == parent
        || world.get_entity(child).is_err()
        || world.get_entity(parent).is_err()
    {
        return false;
    }
    let mut ancestor = Some(parent);
    let mut visited = std::collections::HashSet::new();
    while let Some(entity) = ancestor {
        if entity == child || !visited.insert(entity) {
            return false;
        }
        ancestor = world.get::<Parent>(entity).map(|parent| parent.0);
    }
    true
}

/// Copies one clicked tree row into editor selection and Inspector drafts.
#[allow(clippy::too_many_arguments)]
fn select_item(
    world: &World,
    item: &HierarchyItem,
    state: &mut EditorState,
    edited_transform: &mut Option<Transform>,
    edited_camera: &mut Option<Camera>,
    edited_physics: &mut Option<PhysicsBody>,
    edited_rigid_body: &mut Option<RigidBody>,
    edited_collider: &mut Option<Collider>,
) {
    state.selected = Some(item.entity);
    state.rename_draft = item.name.clone();
    state.rename_target = None;
    *edited_transform = item.transform;
    *edited_camera = world.get::<Camera>(item.entity).copied();
    *edited_physics = world.get::<PhysicsBody>(item.entity).cloned();
    *edited_rigid_body = world.get::<RigidBody>(item.entity).copied();
    *edited_collider = world.get::<Collider>(item.entity).copied();
}

/// Replaces one tree row with a focused text field until Apply or Cancel.
fn draw_inline_rename(
    ui: &mut egui::Ui,
    item: &HierarchyItem,
    state: &mut EditorState,
    entity_request: &mut Option<EntityRequest>,
) {
    const INDENT: f32 = 18.0;
    let mut apply = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.add_space(item.depth as f32 * INDENT);
        let button_width = 52.0;
        let gap = ui.spacing().item_spacing.x;
        let text_width =
            (ui.available_width() - button_width * 2.0 - gap * 2.0).max(50.0);
        let response = ui.add_sized(
            [text_width, 28.0],
            egui::TextEdit::singleline(&mut state.rename_draft),
        );
        response.request_focus();
        apply =
            gui_elements::EditorTheme::toolbar_button(ui, "Apply", false, true)
                .clicked()
                || (response.has_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        cancel = gui_elements::EditorTheme::toolbar_button(
            ui, "Cancel", false, true,
        )
        .clicked()
            || ui.input(|input| input.key_pressed(egui::Key::Escape));
    });
    if apply && !state.rename_draft.trim().is_empty() {
        *entity_request = Some(EntityRequest::Rename(
            item.entity,
            state.rename_draft.trim().into(),
        ));
        state.rename_target = None;
    } else if cancel {
        state.rename_draft = item.name.clone();
        state.rename_target = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::gui_elements::EditorTheme;

    #[test]
    fn children_are_drawn_immediately_after_their_parent() {
        let mut world = World::new();
        let second_root = world.spawn(Name("Z Root".into())).id();
        let first_root = world.spawn(Name("A Root".into())).id();
        let child =
            world.spawn((Name("Child".into()), Parent(first_root))).id();
        let grandchild =
            world.spawn((Name("Grandchild".into()), Parent(child))).id();

        let items = collect_entities(&mut world);
        let order = items
            .iter()
            .map(|item| (item.entity, item.depth))
            .collect::<Vec<_>>();

        assert_eq!(
            order,
            vec![
                (first_root, 0),
                (child, 1),
                (grandchild, 2),
                (second_root, 0)
            ]
        );
    }

    #[test]
    fn drag_parenting_rejects_self_and_descendant_cycles() {
        let mut world = World::new();
        let root = world.spawn(Name("Root".into())).id();
        let child = world.spawn((Name("Child".into()), Parent(root))).id();
        let grandchild =
            world.spawn((Name("Grandchild".into()), Parent(child))).id();

        assert!(!can_reparent(&world, root, grandchild));
        assert!(!can_reparent(&world, child, child));
        assert!(can_reparent(&world, grandchild, root));
    }

    #[test]
    fn child_rows_are_indented_one_step_from_parent_rows() {
        let context = egui::Context::default();
        let mut rects = None;

        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let parent = EditorTheme::tree_row(ui, "Parent", 0, false);
                let child = EditorTheme::tree_row(ui, "Child", 1, false);
                rects = Some((parent.rect, child.rect));
            });
        });

        let (parent, child) = rects.unwrap();
        assert_eq!(child.left() - parent.left(), 18.0);
        assert!(child.top() >= parent.bottom());
    }
}
