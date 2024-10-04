use avian3d::prelude::Collider;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::primitives::Aabb;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::utils::Entry;
use bevy::utils::HashMap;
#[cfg(feature = "rapier")]
#[cfg(not(feature = "avian"))]
use bevy_rapier3d::geometry::ActiveCollisionTypes;
use std::collections::BTreeMap;
use std::time::Duration;

use crate::components::*;
use crate::conversions::*;

use crate::{MapAsset, PostBuildMapEvent};

#[derive(Event)]
pub struct SpawnMeshEvent {
    map: Entity,
    brush: Entity,
    mesh: Mesh,
    collider: Option<Entity>,
    material: Handle<StandardMaterial>,
    texture_name: String,
    texture_size: (u32, u32),
}

pub fn build_map(
    map_units: &MapUnits,
    map_entity: Entity,
    map_asset: &mut MapAsset,
    commands: &mut Commands,
    spawn_mesh_event: &mut EventWriter<SpawnMeshEvent>,
    post_build_map_event: &mut EventWriter<PostBuildMapEvent>,
) {
    let geomap = map_asset.geomap.as_ref().unwrap();

    let face_trangle_planes = &geomap.face_planes;
    let face_planes = shambler::face::face_planes(&face_trangle_planes);
    let brush_hulls = shambler::brush::brush_hulls(&geomap.brush_faces, &face_planes);
    let (face_vertices, _face_vertex_planes) =
        shambler::face::face_vertices(&geomap.brush_faces, &face_planes, &brush_hulls);
    let face_centers = shambler::face::face_centers(&face_vertices);
    let face_indices = shambler::face::face_indices(
        &geomap.face_planes,
        &face_planes,
        &face_vertices,
        &face_centers,
        shambler::face::FaceWinding::Clockwise,
    );
    let face_triangle_indices = shambler::face::face_triangle_indices(&face_indices);
    let face_normals = shambler::face::normals_flat(&face_vertices, &face_planes);

    let face_uvs = shambler::face::new(
        &geomap.faces,
        &geomap.textures,
        &geomap.face_textures,
        &face_vertices,
        &face_planes,
        &geomap.face_offsets,
        &geomap.face_angles,
        &geomap.face_scales,
        &shambler::texture::texture_sizes(
            &geomap.textures,
            map_asset.get_texture_names_with_size(),
        ),
    );

    // spawn entities (@PointClass)
    geomap
        .entity_properties
        .iter()
        .for_each(|(entity_id, props)| {
            // if it's an entity brush we process it later
            if geomap.entity_brushes.get(entity_id).is_some() {
                return;
            }

            // map properties into btree
            // just easier to access props
            let mut props = props
                .iter()
                .map(|p| (p.key.as_str(), p.value.as_str()))
                .collect::<BTreeMap<_, _>>();

            let classname = props.get(&"classname").unwrap_or(&"").to_string();
            let translation = props.get(&"origin").unwrap_or(&"0 0 0").to_string();
            let rotation = props.get(&"angles").unwrap_or(&"0 0 0").to_string();

            let translation = translation.split(" ").collect::<Vec<&str>>();
            let translation = if translation.len() == 3 {
                to_bevy_position(
                    &Vec3::new(
                        translation[0].parse::<f32>().unwrap(),
                        translation[1].parse::<f32>().unwrap(),
                        translation[2].parse::<f32>().unwrap(),
                    ),
                    &map_units,
                )
            } else {
                Vec3::ZERO
            };

            let rotation = rotation.split(" ").collect::<Vec<&str>>();
            let rotation = if rotation.len() == 3 {
                to_bevy_rotation(&Vec3::new(
                    rotation[0].parse::<f32>().unwrap(),
                    rotation[1].parse::<f32>().unwrap(),
                    rotation[2].parse::<f32>().unwrap(),
                ))
            } else {
                Quat::IDENTITY
            };

            commands.entity(map_entity).with_children(|children| {
                let entity = children.spawn((MapEntityProperties {
                    classname: classname.to_string(),
                    transform: Transform::from_translation(translation)
                        * Transform::from_rotation(rotation),
                    properties: props
                        .iter_mut()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                },));

                if let Some(target_name) = props.get("targetname") {
                    entity.insert(TriggerTarget {
                        target_name: target_name.to_string(),
                    });
                }
            });
        });

    // spawn brush entities (@SolidClass)
    for (entity_id, brushes) in geomap.entity_brushes.iter() {
        let entity_properties = geomap.entity_properties.get(&entity_id);

        if let None = entity_properties {
            panic!("brush entity {} has no properties!", entity_id);
        }

        // map properties into btree
        // just easier to access props
        let mut props = entity_properties
            .unwrap()
            .iter()
            .map(|p| (p.key.as_str(), p.value.as_str()))
            .collect::<BTreeMap<_, _>>();
        let classname = props.get(&"classname").unwrap_or(&"").to_string();
        let brush_entity = (
            BrushEntity {},
            MapEntityProperties {
                classname: classname.to_string(),
                properties: props
                    .iter_mut()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                ..default()
            },
            SpatialBundle::default(),
        );

        commands.entity(map_entity).with_children(|children| {
            let mut entity = children.spawn(brush_entity);
            let brush_entity = entity.id();
            entity.with_children(|gchildren| {
                for brush_id in brushes.iter() {
                    let brush_faces = geomap.brush_faces.get(brush_id).unwrap();
                    let mut brush_vertices: Vec<Vec3> = Vec::new();
                    let mut meshes_to_spawn = HashMap::<String, Mesh>::new();
                    let mut has_foliage = false;

                    for face_id in brush_faces.iter() {
                        let texture_id = geomap.face_textures.get(face_id).unwrap();
                        let texture_name = geomap.textures.get(texture_id).unwrap();

                        if !face_triangle_indices.contains_key(&face_id) {
                            println!("face {} has no indices", face_id);
                            continue;
                        }

                        let indices =
                            to_bevy_indecies(&face_triangle_indices.get(&face_id).unwrap());
                        let vertices =
                            to_bevy_vertices(&face_vertices.get(&face_id).unwrap(), &map_units);
                        let mut normals = to_bevy_vec3s(&face_normals.get(&face_id).unwrap());
                        let uvs = uvs_to_bevy_vec2s(&face_uvs.get(&face_id).unwrap());
                        brush_vertices.extend(vertices.clone());

                        // we don't render anything for these textures
                        if texture_name == "trigger"
                            || texture_name == "clip"
                            || texture_name == "common/trigger"
                            || texture_name == "common/clip"
                        {
                            continue;
                        }

                        // For foliage, we make all the normals point up, since we want the
                        // texture to be lit "evenly" from above, to avoid the "paper cutout" look
                        if texture_name.contains("-f") {
                            normals = normals.iter().map(|_| Vec3::new(0.0, 1.0, 0.0)).collect();
                            has_foliage = true;
                        }

                        let mut mesh = Mesh::new(
                            PrimitiveTopology::TriangleList,
                            RenderAssetUsages::RENDER_WORLD,
                        );
                        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
                        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
                        mesh.insert_indices(Indices::U32(indices));

                        if uvs.len() > 0 {
                            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
                            if let Err(e) = mesh.generate_tangents() {
                                println!("error generating tangents: {:?}", e);
                            }
                        }

                        match meshes_to_spawn.entry(texture_name.clone()) {
                            Entry::Occupied(mut entry) => {
                                let mut existing_mesh = entry.get_mut();
                                existing_mesh.merge(&mesh);
                            }
                            Entry::Vacant(entry) => {
                                entry.insert(mesh);
                            }
                        }
                    }

                    // spawn it's collider
                    #[cfg(feature = "avian")]
                    {
                        if let Some(convex_hull) =
                            avian3d::prelude::Collider::convex_hull(brush_vertices)
                        {
                            let mut collider = gchildren.spawn((
                                convex_hull,
                                TransformBundle::default(),
                                VisibilityBundle::default(),
                            ));
                            if classname == "trigger_multiple" {
                                collider = collider.insert((
                                    TriggerMultiple {
                                        target: props.get("target").unwrap().to_string(),
                                    },
                                    avian3d::prelude::RigidBody::Dynamic,
                                    avian3d::prelude::Sensor,
                                ));
                            } else if classname == "trigger_once" {
                                collider = collider.insert((
                                    TriggerOnce {
                                        target: props.get("target").unwrap().to_string(),
                                    },
                                    avian3d::prelude::RigidBody::Dynamic,
                                    avian3d::prelude::Sensor,
                                ));
                            } else if has_foliage {
                                // Don't collide with foliage
                                collider = collider.remove::<Collider>();
                            } else {
                                collider = collider.insert((avian3d::prelude::RigidBody::Static,));
                            }

                            for (texture_name, mesh) in meshes_to_spawn {
                                if map_asset.material_handles.contains_key(&texture_name) {
                                    spawn_mesh_event.send(SpawnMeshEvent {
                                        map: map_entity,
                                        brush: brush_entity,
                                        mesh: mesh,
                                        collider: Some(collider.id()),
                                        material: map_asset
                                            .material_handles
                                            .get(&texture_name)
                                            .unwrap()
                                            .clone(),
                                        texture_size: map_asset
                                            .texture_sizes
                                            .get(&texture_name)
                                            .unwrap()
                                            .clone(),
                                        texture_name: texture_name.to_string(),
                                    });
                                }
                            }
                        }
                    }

                    #[cfg(feature = "rapier")]
                    #[cfg(not(feature = "avian"))]
                    {
                        if let Some(convex_hull) =
                            bevy_rapier3d::prelude::Collider::convex_hull(&brush_vertices)
                        {
                            let mut collider = gchildren.spawn((
                                convex_hull,
                                TransformBundle::default(),
                                VisibilityBundle::default(),
                            ));
                            if classname == "trigger_multiple" {
                                collider.insert((
                                    TriggerMultiple {
                                        target: props.get("target").unwrap().to_string(),
                                    },
                                    bevy_rapier3d::prelude::RigidBody::KinematicPositionBased,
                                    bevy_rapier3d::prelude::Sensor,
                                    ActiveCollisionTypes::default()
                                        | ActiveCollisionTypes::KINEMATIC_KINEMATIC,
                                ));
                            } else if classname == "trigger_once" {
                                collider.insert((
                                    TriggerOnce {
                                        target: props.get("target").unwrap().to_string(),
                                    },
                                    bevy_rapier3d::prelude::RigidBody::KinematicPositionBased,
                                    bevy_rapier3d::prelude::Sensor,
                                    ActiveCollisionTypes::default()
                                        | ActiveCollisionTypes::KINEMATIC_KINEMATIC,
                                ));
                            } else if only_foliage {
                                // Don't collide with foliage
                            } else {
                                collider.insert((bevy_rapier3d::prelude::RigidBody::Fixed,));
                            }

                            for (mesh, texture_name) in meshes_to_spawn {
                                if map_asset.material_handles.contains_key(texture_name) {
                                    spawn_mesh_event.send(SpawnMeshEvent {
                                        map: map_entity,
                                        mesh: mesh,
                                        collider: Some(collider.id()),
                                        material: map_asset
                                            .material_handles
                                            .get(texture_name)
                                            .unwrap()
                                            .clone(),
                                        texture_size: map_asset
                                            .texture_sizes
                                            .get(texture_name)
                                            .unwrap()
                                            .clone(),
                                        texture_name: texture_name.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            });

            if let Some(target_name) = props.get("targetname") {
                entity.insert(TriggerTarget {
                    target_name: target_name.to_string(),
                });
            }
        });
    }

    post_build_map_event.send(PostBuildMapEvent { map: map_entity });
}

pub fn mesh_spawn_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    // mut materials: ResMut<Assets<StandardMaterial>>,
    mut spawn_mesh_event: EventReader<SpawnMeshEvent>,
    transforms: Query<&Transform>,
) {
    let mut consolidated_meshes: HashMap<
        (Entity, Handle<StandardMaterial>, (i32, i32, i32)),
        (Option<Entity>, Entity, Mesh, (u32, u32), String),
    > = HashMap::default();

    // let mut i = 0;
    for ev in spawn_mesh_event.read() {
        // i += 1;
        let transform = ev
            .collider
            .map(|c| transforms.get(c).unwrap_or(&Transform::IDENTITY))
            .unwrap_or_else(|| transforms.get(ev.map).unwrap_or(&Transform::IDENTITY));
        let aabb = ev
            .mesh
            .compute_aabb()
            .unwrap_or(Aabb::from_min_max(Vec3::ZERO, Vec3::ZERO));
        let bucket = (
            ((transform.translation.x + aabb.center.x) / 50.0).floor() as i32,
            ((transform.translation.y + aabb.center.y) / 50.0).floor() as i32,
            ((transform.translation.z + aabb.center.z) / 50.0).floor() as i32,
        );
        match consolidated_meshes.entry((ev.brush, ev.material.clone(), bucket)) {
            Entry::Occupied(mut entry) => {
                let (other_collider, map, mesh, _, _) = entry.get_mut();
                let other_transform: &Transform = other_collider
                    .map(|c| transforms.get(c).unwrap_or(&Transform::IDENTITY))
                    .unwrap_or_else(|| transforms.get(*map).unwrap_or(&Transform::IDENTITY));

                let final_transform = Transform::from_matrix(
                    transform.compute_matrix().inverse() * other_transform.compute_matrix(),
                );

                mesh.merge(&ev.mesh.clone().transformed_by(final_transform));

                if let Some(collider) = ev.collider {
                    commands.entity(collider).with_children(|children| {
                        children.spawn((
                            Brush {
                                texture_size: ev.texture_size,
                                texture_name: ev.texture_name.to_owned(),
                            },
                            SpatialBundle {
                                transform: *transform,
                                ..default()
                            },
                        ));
                    });
                } else {
                    commands.entity(ev.map).with_children(|children| {
                        children.spawn((
                            Brush {
                                texture_size: ev.texture_size,
                                texture_name: ev.texture_name.to_owned(),
                            },
                            SpatialBundle {
                                transform: *transform,
                                ..default()
                            },
                        ));
                    });
                }
            }
            Entry::Vacant(entry) => {
                entry.insert((
                    ev.collider,
                    ev.map,
                    ev.mesh.to_owned(),
                    ev.texture_size,
                    ev.texture_name.to_owned(),
                ));
            }
        }
    }

    // if i > 0 {
    //     println!("Original meshes: {}", i);
    //     println!("Consolidated meshes: {}", consolidated_meshes.len());
    // }

    // let mut a = 0.0;
    for ((_, material, _), (collider, map, mesh, texture_size, texture_name)) in consolidated_meshes
    {
        // if this mesh has a collider, make it a child of the collider
        if let Some(collider) = collider {
            commands.entity(collider).with_children(|children| {
                children.spawn((
                    Brush {
                        texture_size: texture_size,
                        texture_name: texture_name.to_owned(),
                    },
                    PbrBundle {
                        mesh: meshes.add(mesh.to_owned()),
                        material: material.to_owned(),
                        // material: materials.add(StandardMaterial {
                        //     base_color: Color::WHITE,
                        //     emissive: Color::hsv(a, 1.0, 1.0).into(),
                        //     ..default()
                        // }),
                        ..default()
                    },
                ));
            });
        // otherwise, it's a child of the map
        } else {
            commands.entity(map).with_children(|children| {
                children.spawn((
                    Brush {
                        texture_size: texture_size,
                        texture_name: texture_name.to_owned(),
                    },
                    PbrBundle {
                        mesh: meshes.add(mesh.to_owned()),
                        material: material.to_owned(),
                        // material: materials.add(StandardMaterial {
                        //     base_color: Color::WHITE,
                        //     emissive: Color::hsv(a, 1.0, 1.0).into(),
                        //     ..default()
                        // }),
                        ..default()
                    },
                ));
            });
        }
        // a += 40.0;
    }
}

pub fn post_build_map_system(
    map_units: Res<MapUnits>,
    mut commands: Commands,
    mut event_reader: EventReader<crate::PostBuildMapEvent>,
    mut map_entities: Query<(Entity, &crate::components::MapEntityProperties)>,
) {
    for _ in event_reader.read() {
        // to set these up, see the .fgd file in the TrenchBroom
        // game folder for Qevy Example also see the readme
        for (entity, props) in map_entities.iter_mut() {
            match props.classname.as_str() {
                "light" => {
                    commands.entity(entity).insert(PointLightBundle {
                        transform: props.transform,
                        point_light: PointLight {
                            color: props.get_property_as_color("color", Color::WHITE),
                            radius: props.get_property_as_f32("radius", 0.0),
                            range: props.get_property_as_f32("range", 10.0),
                            intensity: props.get_property_as_f32("intensity", 800.0),
                            shadows_enabled: props.get_property_as_bool("shadows_enabled", false),
                            ..default()
                        },
                        ..default()
                    });
                }
                "directional_light" => {
                    commands.entity(entity).insert(DirectionalLightBundle {
                        transform: props.transform,
                        directional_light: DirectionalLight {
                            color: props.get_property_as_color("color", Color::WHITE),
                            illuminance: props.get_property_as_f32("illuminance", 10000.0),
                            shadows_enabled: props.get_property_as_bool("shadows_enabled", false),
                            ..default()
                        },
                        ..default()
                    });
                }
                "mover" => {
                    let mover_entity = commands.entity(entity);
                    let mover_entity = mover_entity.insert((
                        Mover {
                            moving_time: Duration::from_secs_f32(
                                props.get_property_as_f32("moving_time", 1.0),
                            ),
                            destination_time: Duration::from_secs_f32(
                                props.get_property_as_f32("destination_time", 2.0),
                            ),
                            destination_offset: {
                                to_bevy_position(
                                    &props.get_property_as_vec3("destination_offset", Vec3::ZERO),
                                    &map_units,
                                )
                            },
                            state: MoverState::default(),
                        },
                        TransformBundle {
                            local: Transform::from_xyz(0.0, 0.0, 0.0),
                            ..default()
                        },
                    ));

                    if let Some(mover_kind) =
                        props.get_property_as_string("mover_kind", Some(&"linear".into()))
                    {
                        match mover_kind.as_str() {
                            "door" => {
                                mover_entity.insert(Door {
                                    key: props.get_property_as_string("key", None).into(),
                                    open_once: props.get_property_as_bool("open_once", false),
                                });
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
