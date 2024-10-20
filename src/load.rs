use crate::build::SpawnMeshEvent;
use crate::{components::*, MapAssetLoaderError};
use crate::{MapAsset, PostBuildMapEvent};
use bevy::asset::io::Reader;
use bevy::asset::LoadContext;
use bevy::asset::LoadedAsset;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::texture::ImageAddressMode;
use bevy::render::texture::ImageSampler;
use bevy::render::texture::ImageSamplerDescriptor;
use bevy::render::texture::ImageType;
use bevy::render::texture::{CompressedImageFormats, ImageFilterMode};
use std::collections::BTreeMap;

pub(crate) fn extensions() -> &'static [&'static str] {
    &["map"]
}

pub(crate) async fn load<'a>(
    reader: &'a mut dyn Reader,
    load_context: &'a mut LoadContext<'_>,
    headless: bool,
) -> Result<MapAsset, MapAssetLoaderError> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    if let Ok(map) = std::str::from_utf8(&bytes)
        .expect("invalid utf8")
        .parse::<shalrath::repr::Map>()
    {
        let geomap = Some(shambler::GeoMap::new(map.clone()));
        let mut map = MapAsset {
            geomap: geomap,
            texture_sizes: BTreeMap::new(),
            material_handles: BTreeMap::new(),
        };

        if !headless {
            load_map_textures(&mut map, load_context).await?;
        }
        return Ok(map);
    }
    Err(MapAssetLoaderError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "invalid map",
    )))
}

pub(crate) fn handle_loaded_map_system(
    map_units: Res<MapUnits>,
    mut commands: Commands,
    mut map_assets: ResMut<Assets<MapAsset>>,
    mut ev_asset: EventReader<AssetEvent<MapAsset>>,
    mut q_maps: Query<Entity, With<Map>>,
    mut post_build_event: EventWriter<PostBuildMapEvent>,
    mut spawn_mesh_event: EventWriter<SpawnMeshEvent>,
) {
    for ev in ev_asset.read() {
        match ev {
            AssetEvent::LoadedWithDependencies { id } => {
                for map_entity in q_maps.iter_mut() {
                    commands.entity(map_entity).despawn_descendants();
                    let map_asset = map_assets.get_mut(*id).unwrap();
                    crate::build::build_map(
                        &map_units,
                        map_entity,
                        map_asset,
                        &mut commands,
                        &mut spawn_mesh_event,
                        &mut post_build_event,
                    );
                }
            }
            _ => {}
        }
    }
}

pub(crate) async fn load_map_textures<'a>(
    map_asset: &mut MapAsset,
    load_context: &mut LoadContext<'a>,
) -> Result<(), MapAssetLoaderError> {
    let geomap = map_asset.geomap.as_mut().unwrap();

    // for each texture, load it into the asset server
    for texture_info in geomap.textures.iter() {
        let texture_name = texture_info.1;

        let base_color_texture = match load_texture(
            format!("textures/{}.png", texture_name),
            true,
            load_context,
        )
        .await
        {
            Ok(texture) => Some(texture),
            Err(MapAssetLoaderError::ReadAssetBytes(_)) => None,
            Err(err) => {
                return Err(err);
            }
        };

        if base_color_texture.is_some() {
            let (base_color_texture, texture_size) = base_color_texture.unwrap();

            let metallic_roughness_texture = match load_texture(
                format!("textures/{}.metallic_roughness.png", texture_name),
                false,
                load_context,
            )
            .await
            {
                Ok(texture) => Some(texture),
                Err(MapAssetLoaderError::ReadAssetBytes(_)) => None,
                Err(err) => {
                    return Err(err);
                }
            };

            let normal_map_texture = match load_texture(
                format!("textures/{}.normal_map.png", texture_name),
                false,
                load_context,
            )
            .await
            {
                Ok(texture) => Some(texture),
                Err(MapAssetLoaderError::ReadAssetBytes(_)) => None,
                Err(err) => {
                    return Err(err);
                }
            };

            let depth_map_texture = match load_texture(
                format!("textures/{}.depth_map.png", texture_name),
                false,
                load_context,
            )
            .await
            {
                Ok(texture) => Some(texture),
                Err(MapAssetLoaderError::ReadAssetBytes(_)) => None,
                Err(err) => {
                    return Err(err);
                }
            };

            let occlusion_texture = match load_texture(
                format!("textures/{}.occlusion.png", texture_name),
                false,
                load_context,
            )
            .await
            {
                Ok(texture) => Some(texture),
                Err(MapAssetLoaderError::ReadAssetBytes(_)) => None,
                Err(err) => {
                    return Err(err);
                }
            };

            let emissive_texture = match load_texture(
                format!("textures/{}.emissive.png", texture_name),
                false,
                load_context,
            )
            .await
            {
                Ok(texture) => Some(texture),
                Err(MapAssetLoaderError::ReadAssetBytes(_)) => None,
                Err(err) => {
                    return Err(err);
                }
            };

            let specular_transmission_texture = match load_texture(
                format!("textures/{}.specular_transmission.png", texture_name),
                false,
                load_context,
            )
            .await
            {
                Ok(texture) => Some(texture),
                Err(MapAssetLoaderError::ReadAssetBytes(_)) => None,
                Err(err) => {
                    return Err(err);
                }
            };

            let diffuse_transmission_texture = match load_texture(
                format!("textures/{}.diffuse_transmission.png", texture_name),
                false,
                load_context,
            )
            .await
            {
                Ok(texture) => Some(texture),
                Err(MapAssetLoaderError::ReadAssetBytes(_)) => None,
                Err(err) => {
                    return Err(err);
                }
            };

            let (perceptual_roughness, metallic, reflectance) =
                if metallic_roughness_texture.is_some() {
                    (1.0, 1.0, 0.5)
                } else {
                    (0.55, 0.0, 0.0)
                };

            let alpha_mode = if texture_name.ends_with("-m") || texture_name.ends_with("-f") {
                AlphaMode::Mask(0.5)
            } else {
                AlphaMode::Opaque
            };

            let (specular_transmission, thickness) = if specular_transmission_texture.is_some() {
                (1.0, 0.1)
            } else {
                (0.0, 0.0)
            };

            let diffuse_transmission = if diffuse_transmission_texture.is_some() {
                1.0
            } else if texture_name.contains("-f") {
                0.5
            } else {
                0.0
            };

            let emissive = if emissive_texture.is_some() {
                LinearRgba::new(30.0, 30.0, 30.0, 1.0)
            } else {
                LinearRgba::BLACK
            };

            let mat = StandardMaterial {
                perceptual_roughness,
                metallic,
                reflectance,
                base_color_texture: Some(base_color_texture),
                metallic_roughness_texture: metallic_roughness_texture.map(|(t, _)| t),
                normal_map_texture: normal_map_texture.map(|(t, _)| t),
                depth_map: depth_map_texture.map(|(t, _)| t),
                occlusion_texture: occlusion_texture.map(|(t, _)| t),
                parallax_mapping_method: ParallaxMappingMethod::Relief { max_steps: 20 },
                specular_transmission,
                diffuse_transmission,
                thickness,
                specular_transmission_texture: specular_transmission_texture.map(|(t, _)| t),
                diffuse_transmission_texture: diffuse_transmission_texture.map(|(t, _)| t),
                emissive_texture: emissive_texture.map(|(t, _)| t),
                emissive,
                parallax_depth_scale: 0.04,
                alpha_mode,
                ..default()
            };

            let mat_handle = load_context.add_loaded_labeled_asset::<StandardMaterial>(
                format!("materials/{}", texture_name),
                LoadedAsset::from(mat),
            );
            map_asset
                .material_handles
                .insert(texture_name.clone(), mat_handle);
            map_asset
                .texture_sizes
                .insert(texture_name.clone(), texture_size);
        }
    }

    Ok(())
}

async fn load_texture<'a>(
    file: String,
    is_srgb: bool,
    load_context: &mut LoadContext<'a>,
) -> Result<(Handle<Image>, (u32, u32)), MapAssetLoaderError> {
    let bytes = load_context.read_asset_bytes(&file).await?;

    let filter = if file.contains("-m") || file.contains("-f") || file.contains(".normal_map") {
        // avoid getting the edges of masked shapes and weird artifacts in normal map lighting
        ImageFilterMode::Nearest
    } else {
        ImageFilterMode::Linear
    };

    let image = Image::from_buffer(
        &bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::all(),
        is_srgb,
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: filter,
            min_filter: filter,
            mipmap_filter: filter,
            ..default()
        }),
        RenderAssetUsages::RENDER_WORLD,
    )?;

    let handle = load_context.add_loaded_labeled_asset(file, LoadedAsset::from(image.clone()));

    Ok((handle, (image.width(), image.height())))
}
