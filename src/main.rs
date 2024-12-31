use std::time::Duration;

use avian2d::prelude::*;
use bevy::{
    prelude::*,
    reflect::{DynamicTyped, TypeData},
    time::Stopwatch,
    utils::{HashMap, HashSet},
};
use bevy_ecs_ldtk::prelude::*;
use itertools::Itertools;

const HORIZONTAL_SPEED_PER_SEC: f32 = 80.;
const JUMP_HEIGHT: f32 = TILE * 3.0;
const JUMP_HORIZONTAL_DISTANCE: f32 = TILE * 5.;
const JUMP_BUFFER_SECS: f32 = 0.2;
// Bevy runs at 64 hz for fixed timestep.
const HORIZONTAL_SPEED_PER_TICK: f32 = HORIZONTAL_SPEED_PER_SEC / 64.;
const TILE: f32 = 16.0;
const HALF: f32 = 1. / 2.;
const JUMP_INITIAL_VELOCITY: f32 =
    (2. * JUMP_HEIGHT * HORIZONTAL_SPEED_PER_SEC) / (JUMP_HORIZONTAL_DISTANCE * HALF);
const GRAVITY: f32 = (2. * JUMP_HEIGHT * HORIZONTAL_SPEED_PER_SEC * HORIZONTAL_SPEED_PER_SEC)
    / (HALF * HALF * JUMP_HORIZONTAL_DISTANCE * JUMP_HORIZONTAL_DISTANCE);
const JUMP_DURATION_TO_PEAK_IF_HELD: f32 = (-JUMP_INITIAL_VELOCITY) / -GRAVITY;
const FAST_FALL_GRAVITY: f32 = GRAVITY * 2.;

const PLAYER_COLLIDER_WIDTH: f32 = 7.;
const PLAYER_COLLIDER_HEIGHT: f32 = 14.;
const PLAYER_COLLIDER_ROUNDING: f32 = 2.;

fn main() {
    App::new()
        .add_plugins(PhysicsDebugPlugin::default())
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        // .insert_resource(Gravity(Vec2::NEG_Y * GRAVITY))
        .insert_resource(Gravity(Vec2::ZERO))
        .add_plugins(LdtkPlugin)
        .insert_resource(LevelSelection::index(2))
        .add_systems(Startup, setup)
        .add_systems(Update, setup_wall_physics)
        .add_systems(Update, rescale_colliders)
        .add_systems(
            FixedUpdate,
            (
                (player_proximity_cast, update_action_buffer),
                jump,
                apply_player_gravity,
            )
                .chain(),
        )
        // .add_systems(Update, setup_physics_object_collider)
        .register_ldtk_entity::<PlayerBundle>("Player")
        .register_ldtk_entity::<GoalBundle>("Goal")
        .register_ldtk_int_cell::<WallBundle>(1)
        // .add_systems(Update, draw_tile_gizmo)
        .add_systems(Update, move_player_from_input)
        // .add_systems(
        //     Update,
        //     (translate_grid_coords_entities, move_player_from_input),
        // )
        // .add_systems(Update, test) // .add_systems(Update, print_sprite)
        // .add_systems(Update, gizmos)
        // .add_systems(Update, update_gizmo)
        // .insert_resource(GizmoOrigin(Vec2::ZERO))
        .run();
}

// fn print_sprite(mut player: Query<&Sprite, With<Player>>) {
//     let Ok(player) = player.get_single() else {
//         return;
//     };
//     dbg!(player);
//     panic!();
// }

// fn test(
//     mut players: Query<&Transform, (With<Player>, Without<Wall>)>,
//     walls: Query<&Transform, (With<Wall>, Without<Player>)>,
// ) {
//     if players.is_empty() || walls.is_empty() {
//         return;
//     }
//     let player = players.single();
//     let mut hashset = HashSet::new();
//     for wall in walls.iter() {
//         hashset.insert(wall.translation.z as i32);
//     }
//     dbg!(player);
//     dbg!(hashset);
// }

#[derive(Debug, Default, strum::EnumIs)]
enum JumpingStatus {
    #[default]
    NotJumping,
    Jumping {
        jump_duration: Stopwatch,
        initial_jump_location: Vec2,
    },
    JumpingEarlyRelease {
        initial_jump_location: Vec2,
    },
    // Falling {
    //     initial_jump_location: Vec2,
    // },
}

#[derive(Component, Debug, Default)]
struct PlayerPhysicsStatus {
    grounded: bool,
    left_wall: bool,
    right_wall: bool,
    jumping_status: JumpingStatus,
}

#[derive(Debug, Default)]
enum ActionBuffer {
    Buffered(Stopwatch),
    #[default]
    None,
}

impl ActionBuffer {
    fn reset(&mut self) {
        *self = ActionBuffer::None;
    }
    fn start(&mut self) {
        *self = ActionBuffer::Buffered(Stopwatch::new());
    }
    fn tick(&mut self, dt: Duration) {
        if let ActionBuffer::Buffered(buffer) = self {
            buffer.tick(dt);
        }
    }
    fn within_time(&self, secs: f32) -> bool {
        if let ActionBuffer::Buffered(buffer) = self {
            return buffer.elapsed_secs() <= secs;
        }
        false
    }
}

#[derive(Component, Debug, Default)]
struct ActionBufferStorage {
    jump: ActionBuffer,
}

fn apply_player_gravity(
    time: Res<Time>,
    mut q: Query<(&mut LinearVelocity, &PlayerPhysicsStatus), With<Player>>,
) {
    let Ok((mut player_velocity, status)) = q.get_single_mut() else {
        return;
    };
    match status.jumping_status {
        JumpingStatus::NotJumping => player_velocity.y -= GRAVITY * time.delta_secs(),
        JumpingStatus::Jumping { .. } => player_velocity.y -= GRAVITY * time.delta_secs(),
        JumpingStatus::JumpingEarlyRelease { .. } => {
            player_velocity.y -= FAST_FALL_GRAVITY * time.delta_secs()
        }
    }
}

fn update_action_buffer(
    time: Res<Time>,
    mut q: Query<&mut ActionBufferStorage>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let Ok(mut storage) = q.get_single_mut() else {
        return;
    };
    storage.jump.tick(time.delta());
    if input.just_pressed(KeyCode::Space) {
        storage.jump.start();
    }
}

fn player_proximity_cast(
    mut q: Query<(&Transform, &mut PlayerPhysicsStatus)>,
    spatial_query: SpatialQuery,
) {
    let Ok((transform, mut status)) = q.get_single_mut() else {
        return;
    };
    let config = ShapeCastConfig {
        max_distance: 0.,
        target_distance: 0.,
        compute_contact_on_penetration: false,
        ignore_origin_penetration: false,
    };
    let filter = SpatialQueryFilter {
        ..Default::default()
    };
    let down_cast = spatial_query.cast_shape(
        &Collider::rectangle(PLAYER_COLLIDER_WIDTH - 0.01, 0.),
        transform.translation.xy() - Vec2::new(0., PLAYER_COLLIDER_HEIGHT / 2. + 0.1),
        0.,
        Dir2::NEG_Y,
        &config,
        &filter,
    );
    let left_cast = spatial_query.cast_shape(
        &Collider::rectangle(0., PLAYER_COLLIDER_HEIGHT - 0.01),
        transform.translation.xy() - Vec2::new(PLAYER_COLLIDER_WIDTH / 2. + 0.1, 0.),
        0.,
        Dir2::X,
        &config,
        &filter,
    );
    let right_cast = spatial_query.cast_shape(
        &Collider::rectangle(0., PLAYER_COLLIDER_HEIGHT - 0.01),
        transform.translation.xy() + Vec2::new(PLAYER_COLLIDER_WIDTH / 2. + 0.1, 0.),
        0.,
        Dir2::NEG_X,
        &config,
        &filter,
    );
    status.grounded = down_cast.is_some();
    status.left_wall = left_cast.is_some();
    status.right_wall = right_cast.is_some();
}

fn jump(
    mut player: Query<
        (
            &Transform,
            &mut LinearVelocity,
            &mut PlayerPhysicsStatus,
            &mut ActionBufferStorage,
        ),
        With<Player>,
    >,
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    let Ok((transform, mut vel, mut status, mut storage)) = player.get_single_mut() else {
        return;
    };
    if status.grounded {
        status.jumping_status = JumpingStatus::NotJumping;
    }
    match status.jumping_status {
        JumpingStatus::Jumping {
            ref mut jump_duration,
            initial_jump_location,
        } => {
            jump_duration.tick(time.delta());
            if jump_duration.elapsed_secs() >= JUMP_DURATION_TO_PEAK_IF_HELD {
                status.jumping_status = JumpingStatus::NotJumping;
            }
            if input.just_released(KeyCode::Space) {
                status.jumping_status = JumpingStatus::JumpingEarlyRelease {
                    initial_jump_location,
                };
                vel.y = 0.;
            }
        }
        JumpingStatus::JumpingEarlyRelease {
            initial_jump_location,
        } => {
            if initial_jump_location.y >= transform.translation.y {
                status.jumping_status = JumpingStatus::NotJumping;
            }
        }
        JumpingStatus::NotJumping => {}
    }
    if storage.jump.within_time(JUMP_BUFFER_SECS) && status.grounded {
        storage.jump.reset();
        vel.y = JUMP_INITIAL_VELOCITY;
        status.grounded = false;
        status.jumping_status = JumpingStatus::Jumping {
            initial_jump_location: transform.translation.xy(),
            jump_duration: Stopwatch::new(),
        };
    }
}

fn move_player_from_input(
    mut player: Query<(&mut LinearVelocity, &mut Sprite), With<Player>>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let Ok((mut velocity, mut sprite)) = player.get_single_mut() else {
        return;
    };

    let mut left_right = 0f32;
    let mut flip = false;
    if input.pressed(KeyCode::KeyA) {
        left_right -= HORIZONTAL_SPEED_PER_SEC;
        flip = true;
    }
    if input.pressed(KeyCode::KeyD) {
        left_right += HORIZONTAL_SPEED_PER_SEC;
        flip = false;
    }
    if left_right == 0. {
        // if input.any_just_released([KeyCode::KeyA, KeyCode::KeyD]) {
        velocity.x = 0.;
        // }
    } else {
        sprite.flip_x = flip;
        velocity.x = left_right;
    }
}

// fn setup_physics_object_collider(
//     mut commands: Commands,
//     mut query: Query<(Entity, &mut Transform, &Parent), Added<PhysicsObject>>,
// ) {
//     for (entity, transform, parent) in query.iter_mut() {
//         let mut cmds = commands.spawn((
//             RigidBody::Dynamic,
//             Collider::rectangle(16., 16.),
//             *transform,
//         ));
//         cmds.set_parent(parent.get());
//         cmds.insert_children(1, &[entity]);
//         // The division by transform.scale.x makes sure that the collider size is
//         // actually 16.0 and not scaled relative to pixel art scaling
//         // commands.entity(entity).insert((
//         //     RigidBody::Dynamic,
//         //     Collider::rectangle(16.0 / transform.scale.x, 16.0 / transform.scale.y),
//         //     transform.with_translation(transform.translation.with_z(10.)),
//         // ));
//     }
// }

fn rescale_colliders(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Collider), With<PendingRescale>>,
) {
    for (entity, mut collider) in query.iter_mut() {
        // collider.set_scale(transform.scale.recip().xy(), 1);
        collider.set_scale(Vec2::splat(1.0), 1);
        commands.entity(entity).remove::<PendingRescale>();
    }
}

#[derive(Default, Component)]
struct PendingRescale;

#[derive(Default, Component)]
struct Player;
#[derive(Default, Bundle, LdtkEntity)]
struct PlayerBundle {
    #[sprite_sheet]
    sprite_sheet: Sprite,
    // #[grid_coords]
    // grid_coords: GridCoords,
    player: Player,
    pending_rescale: PendingRescale,
    #[with(|_| RigidBody::Dynamic)]
    rigid_body: RigidBody,
    #[with(|_| Collider::round_rectangle(
        PLAYER_COLLIDER_WIDTH - PLAYER_COLLIDER_ROUNDING * 2.,
        PLAYER_COLLIDER_HEIGHT - PLAYER_COLLIDER_ROUNDING * 2.,
        PLAYER_COLLIDER_ROUNDING
    ))]
    collider: Collider,
    #[with(|_| Friction::new(0.).with_combine_rule(CoefficientCombine::Min))]
    combine: Friction,
    #[with(|_| LockedAxes::ROTATION_LOCKED)]
    locked: LockedAxes,
    proximity: PlayerPhysicsStatus,
    action_buffers: ActionBufferStorage,
}

#[derive(Default, Bundle, LdtkEntity)]
struct GoalBundle {
    #[sprite_sheet]
    sprite_sheet: Sprite,
}

#[derive(Default, Component)]
struct Wall;

#[derive(Default, Bundle, LdtkIntCell)]
struct WallBundle {
    wall: Wall,
}

// fn translate_grid_coords_entities(
//     mut grid_coords_entities: Query<(&mut Transform, &GridCoords), Changed<GridCoords>>,
// ) {
//     for (mut transform, grid_coords) in grid_coords_entities.iter_mut() {
//         transform.translation =
//             bevy_ecs_ldtk::utils::grid_coords_to_translation(*grid_coords, IVec2::splat(16))
//                 .extend(transform.translation.z);
//     }
// }

fn setup_wall_physics(
    mut commands: Commands,
    // Gridcoords for "greedy meshing" of colliders
    // Parent so we can get grandparent (level entity)
    walls: Query<(&GridCoords, &Parent), Added<Wall>>,
    // We look through this query using the entity it from the "walls" query
    // therefore we can get "grandparent" entity.
    parents: Query<&Parent, Without<Wall>>,
    mut gizmos: Gizmos,
) {
    let mut map: HashMap<Entity, HashSet<GridCoords>> = HashMap::new();
    for (grid_coords, parent) in walls.iter() {
        let grandparent = parents.get(parent.get()).expect("level should exist").get();
        map.entry(grandparent).or_default().insert(*grid_coords);
    }
    for (level, coords) in map {
        let grouped = coords
            .into_iter()
            .into_group_map_by(|coords| (coords.x.div_euclid(64), coords.y.div_euclid(64)));
        let mut buffer = Vec::new();
        for ((xmul, ymul), coords_list) in grouped {
            let mut data = [0u64; 64];
            for coord in coords_list {
                let x = coord.x.rem_euclid(64);
                let y = coord.y.rem_euclid(64);
                data[x as usize] |= 1 << y;
            }
            binary_greedy_meshing(data, &mut buffer);
            let mut final_meshes: Vec<(GridCoords, GridCoords)> = Vec::new();
            for MeshedRect { x1, y1, x2, y2 } in buffer.iter().copied() {
                final_meshes.push((
                    GridCoords::new(xmul * 64 + x1, ymul * 64 + y1),
                    GridCoords::new(xmul * 64 + x2, ymul * 64 + y2),
                ))
            }
            buffer.clear();
            commands.entity(level).with_children(|builder| {
                for (gc1, gc2) in final_meshes {
                    let t1 =
                        bevy_ecs_ldtk::utils::grid_coords_to_translation(gc1, IVec2::splat(16));
                    let t2 =
                        bevy_ecs_ldtk::utils::grid_coords_to_translation(gc2, IVec2::splat(16));
                    gizmos.circle_2d(t1, 1., Color::srgb(1., 0., 0.));
                    gizmos.circle_2d(t2, 1., Color::srgb(1., 0., 0.));
                    let avg = t1.midpoint(t2) - Vec2::splat(8.0);
                    let w = (t1.x - t2.x).abs();
                    let h = (t1.y - t2.y).abs();
                    builder.spawn((
                        Transform::from_translation(avg.extend(0.)),
                        Collider::rectangle(w, h),
                        RigidBody::Static,
                    ));
                }
            });
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MeshedRect {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

fn binary_greedy_meshing(mut data: [u64; 64], buffer: &mut Vec<MeshedRect>) {
    'outer: for line in 0..data.len() {
        loop {
            // Get the index of the first unmeshed collider in this line
            // or 64, if there are none
            let first_index = data[line].trailing_zeros();
            if first_index == 64 {
                // No more colliders to mesh in this line, go to next line
                continue 'outer;
            }
            // Find number of colliders in line that can be combined with this one.
            let count = (data[line] >> first_index).trailing_ones();
            // Get a mask that represents the meshable bits in the row.
            let mask = 1u64.checked_shl(count).map_or(!0, |n| n - 1) << first_index;
            // Zero out current line
            data[line] &= !mask;
            let mut width = 1;
            // Continue to the right and try to greedily mesh the next lines if available.
            // off by 1 proof: at the last index in array,
            // - line is 63
            // - width is 1
            // line + width is 64, and we should stop.
            // so condition should be < 64
            while line + width < 64 {
                let masked_next = data[line + width] & mask;
                if masked_next != mask {
                    // We can't expand into the next line anymore
                    break;
                }
                // zero out the bytes we just meshed
                data[line + width] &= !mask;
                width += 1;
            }
            buffer.push(MeshedRect {
                x1: line as i32,
                y1: first_index as i32,
                x2: (line + width) as i32,
                y2: (first_index + count) as i32,
            })
        }
    }
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Camera2d,
        OrthographicProjection {
            scale: 0.5,
            ..OrthographicProjection::default_2d()
        },
        Transform::from_xyz(1280.0 / 4.0, 720.0 / 4.0, 0.0),
    ));

    commands.spawn(LdtkWorldBundle {
        ldtk_handle: asset_server.load("tile-based-game.ldtk").into(),
        ..Default::default()
    });
}

// fn draw_tile_gizmo(mut gizmos: Gizmos, q: Query<&GridCoords, With<Wall>>) {
//     for grid_coords in q.iter() {
//         let translation =
//             bevy_ecs_ldtk::utils::grid_coords_to_translation(*grid_coords, IVec2::splat(16));
//         gizmos.rect_2d(
//             Isometry2d {
//                 rotation: Rot2::radians(0.),
//                 translation,
//             },
//             Vec2::splat(16.),
//             Color::srgb(0., 1., 0.),
//         );
//     }
// }

// #[derive(Resource)]
// struct GizmoOrigin(Vec2);

// fn update_gizmo(
//     kbd: Res<ButtonInput<KeyCode>>,
//     mut origin: ResMut<GizmoOrigin>,
//     mut q: Query<&mut Text2d>,
// ) {
//     let mut dir = Vec2::ZERO;
//     if kbd.pressed(KeyCode::ArrowUp) {
//         dir += Vec2::Y;
//     }
//     if kbd.pressed(KeyCode::ArrowDown) {
//         dir -= Vec2::Y;
//     }
//     if kbd.pressed(KeyCode::ArrowLeft) {
//         dir -= Vec2::X;
//     }
//     if kbd.pressed(KeyCode::ArrowRight) {
//         dir += Vec2::X;
//     }
//     origin.0 += dir;
//     q.single_mut().0 = format!("x: {}, y: {}", origin.0.x, origin.0.y);
// }

// fn gizmos(mut gizmos: Gizmos, origin: Res<GizmoOrigin>) {
//     gizmos.rect_2d(
//         Isometry2d {
//             rotation: Rot2::degrees(0.),
//             translation: origin.0,
//         },
//         Vec2::splat(16.0),
//         Color::srgb(1.0, 0., 0.),
//     );
// }
