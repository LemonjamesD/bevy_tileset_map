//! Tools for placing and removing tiles

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use bevy_tileset::prelude::*;
use thiserror::Error;
use std::hash::Hash;
use bevy::asset::Error;

/// Errors related to the placement of tiles
#[derive(Error, Debug)]
pub enum TilePlacementError {
	/// A tile already exists at the given coordinate
	///
	/// The meaning of this changes a bit depending on the context. For example, it could
	/// mean that an unexpected tile was found or that _any_ tile was found.
	#[error("Attempted to place tile {new:?} but found existing tile {existing:?} (@ {pos:?})")]
	TileAlreadyExists {
		/// The ID of the new tile to be placed
		new: TileId,
		/// The ID of the existing tile
		existing: Option<TileId>,
		/// The desired/occupied tile coordinate
		pos: TilePos,
	},
	/// The tileset does not exist or is invalid
	///
	/// Contains the ID of the tileset in question
	#[error("Invalid tileset {0:?}")]
	InvalidTileset(TilesetId),
	/// The tile does not exist or is invalid
	///
	/// This can happen when a given [`TileId`] doesn't exist within a tileset
	///
	/// Contains the ID of the tile in question
	#[error("Invalid tile {0:?}")]
	InvalidTile(TileId),
	/// A catch-all for errors generated by `bevy_ecs_tilemap`
	///
	/// Contains the generated error
	#[error("Tilemap error: {0:?}")]
	MapError(Error),
}

/// An enum denoting how a tile was placed or removed
///
/// This allows you to respond to the results the placement, such as handling cleanup
/// or performing a secondary action.
///
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PlacedTile {
	/// A tile was added
	Added {
		/// The replaced tile
		old_tile: Option<(Entity, Option<TileId>)>,
		/// The placed tile
		new_tile: (Entity, TileId),
	},
	/// A tile was removed
	Removed {
		/// The removed tile
		old_tile: Option<(Entity, Option<TileId>)>,
	},
}

/// A result type alias for tile placement
pub type TilePlacementResult = Result<PlacedTile, TilePlacementError>;

// Ripped straight out of bevy_ecs_tilemap code
pub trait MapId: Clone + Copy + PartialEq + Eq + Hash + Into<u16> {}

impl MapId for u16 {}

/// A helper system param used to place tiles
///
/// All methods automatically account for the tile's [`TileType`] and respects Auto Tiles,
/// allowing for a much simpler user experience.
///
/// Additionally, tilesets are automatically derived from the given [`TileId`]s. This works for
/// any [`Tileset`] registered in `Assets<Tileset>`.
///
/// # Examples
///
/// ```
/// # use bevy::prelude::Res;
/// # use bevy_ecs_tilemap::TilePos;
/// # use bevy_tileset_map::prelude::{TileId, TilePlacer};
/// struct CurrentTile(TileId);
///
/// fn tile_placement_system(mut placer: TilePlacer, tile: Res<CurrentTile>) {
///   placer.place(
///     tile.0,
///     TilePos(0, 0),
///     0u16,
///     0u16,
///   ).unwrap();
/// }
/// ```
#[derive(SystemParam)]
pub struct TilePlacer<'w, 's> {
	map_query: Query<'w, 's, &'static TileStorage>,
	tilesets: Tilesets<'w, 's>,
	commands: Commands<'w, 's>,
	/// Query used to get info about a tile
	#[cfg(not(feature = "auto-tile"))]
	#[allow(dead_code)]
	query: Query<'w, 's, (&'static TileTextureIndex, Option<&'static AnimatedTile>)>,
	/// Query used to get info about a tile
	#[cfg(feature = "auto-tile")]
	#[allow(dead_code)]
	query: Query<
		'w,
		's,
		(
			&'static TileTextureIndex,
			Option<&'static AnimatedTile>,
			Option<&'static bevy_tileset::auto::AutoTileId>,
		),
	>,
	/// Query used to get and send data for the [`RemoveAutoTileEvent`] event
	#[cfg(feature = "auto-tile")]
	#[allow(dead_code)]
	auto_query: Query<
		'w,
		's,
		(
			&'static TilePos,
			&'static TileParent,
			&'static bevy_tileset::auto::AutoTileId,
		),
		With<Tile>,
	>,
	#[cfg(feature = "auto-tile")]
	#[allow(dead_code)]
	event_writer: EventWriter<'w, 's, crate::auto::RemoveAutoTileEvent>,
}

impl<'w, 's> TilePlacer<'w, 's> {
	pub fn place<Id: Into<TileId>, Pos: Into<TilePos> + Clone, MId: MapId>(
		&mut self,
		tile_id: Id,
		pos: Pos,
		map_id: MId,
		layer_id: u16,
	) -> TilePlacementResult {
		self.place_unchecked(tile_id, pos, map_id, layer_id)
	}

	pub fn try_place<Id: Into<TileId>, Pos: Into<TilePos> + Clone, MId: MapId>(
		&mut self,
		tile_id: Id,
		pos: Pos,
		map_id: MId,
		layer_id: u16,
	) -> TilePlacementResult {
		let id = tile_id.into();
		let pos = pos.into();

		if let Some(existing) = self.get_existing(id, pos, map_id, layer_id) {
			// Tile already exists -> don't place
			return Err(TilePlacementError::TileAlreadyExists {
				new: id,
				existing: existing.id,
				pos,
			});
		}

		self.place_unchecked(id, pos, map_id, layer_id)
	}

	pub fn replace<Id: Into<TileId>, Pos: Into<TilePos> + Clone, MId: MapId>(
		&mut self,
		tile_id: Id,
		pos: Pos,
		map_id: MId,
		layer_id: u16,
	) -> TilePlacementResult {
		let id = tile_id.into();
		let pos = pos.into();

		if let Some(existing) = self.get_existing(id, pos, map_id, layer_id) {
			// Check that the existing tile is of a different type
			if let Some(existing_id) = existing.id {
				if existing_id.eq_tile_group(&id) {
					return Err(TilePlacementError::TileAlreadyExists {
						new: id,
						existing: existing.id,
						pos,
					});
				}
			}
		}

		self.place_unchecked(id, pos, map_id, layer_id)
	}

	pub fn toggle_matching<Id: Into<TileId>, Pos: Into<TilePos> + Clone, MId: MapId>(
		&mut self,
		tile_id: Id,
		pos: Pos,
		map_id: MId,
		layer_id: u16,
	) -> TilePlacementResult {
		let id = tile_id.into();
		let pos = pos.into();

		if let Some(existing) = self.get_existing(id, pos, map_id, layer_id) {
			// Remove the existing tile if it matches
			if let Some(existing_id) = existing.id {
				if existing_id.eq_tile_group(&id) {
					self.remove(pos, map_id, layer_id)?;
					return Ok(PlacedTile::Removed {
						old_tile: Some((existing.entity, Some(existing_id))),
					});
				}
			}

			// Tile exists but did not match -> don't remove
			return Err(TilePlacementError::TileAlreadyExists {
				new: id,
				existing: existing.id,
				pos,
			});
		}

		self.place_unchecked(id, pos, map_id, layer_id)
	}

	pub fn toggle<Id: Into<TileId>, Pos: Into<TilePos> + Clone, MId: MapId>(
		&mut self,
		tile_id: Id,
		pos: Pos,
		map_id: MId,
		layer_id: u16,
	) -> TilePlacementResult {
		let id = tile_id.into();
		let pos = pos.into();

		if let Some(existing) = self.get_existing(id, pos, map_id, layer_id) {
			self.remove(pos, map_id, layer_id)?;
			return Ok(PlacedTile::Removed {
				old_tile: Some((existing.entity, existing.id)),
			});
		}

		self.place_unchecked(id, pos, map_id, layer_id)
	}

	pub fn remove<Pos: Into<TilePos>, MId: MapId>(
		&mut self,
		pos: Pos,
		map_id: MId,
		layer_id: u16,
	) -> Result<(), TilePlacementError> {
		let pos = pos.into();

		#[cfg(feature = "auto-tile")]
		{
			// Get the current tile entity
			let entity = self
				.map_query
				.get_tile_entity(pos, map_id, layer_id)
				.map_err(|err| TilePlacementError::MapError(err))?;

			// Attempt to remove the auto tile
			self.try_remove_auto_tile(entity);
		}

		// Despawn the tile and notify the chunk
		self.map_query
			.remove(pos);
		Ok(())
	}

	pub fn add_to_layer<TId: Into<TileId>, Pos: Into<TilePos>>(
		&mut self,
		tile_id: TId,
		pos: Pos,
		layer_builder: &mut LayerBuilder<TileBundle>,
	) -> TilePlacementResult {
	}

	pub fn update<TId: Into<TileId>>(
		&mut self,
		tile_id: TId,
		entity: Entity,
	) -> Result<(), TilePlacementError> {
	}

	fn place_unchecked<Id: Into<TileId>, Pos: Into<TilePos>, MId: MapId>(
		&mut self,
		tile_id: Id,
		pos: Pos,
		map_id: MId,
		layer_id: u16,
	) -> TilePlacementResult {
	}

	#[cfg(feature = "auto-tile")]
	fn apply_auto_tile(&mut self, id: &TileId, tileset_id: &TilesetId, entity: Entity) {
		let id = id.into();
		let is_auto = self
			.get_tile_data(id)
			.ok()
			.and_then(|data| Some(data.is_auto()))
			.unwrap_or_default();

		let mut cmds = self.commands.entity(entity);
		if is_auto {
			cmds.insert(bevy_tileset::auto::AutoTileId {
				group_id: id.group_id,
				tileset_id: *tileset_id,
			});
		} else {
			cmds.remove::<bevy_tileset::auto::AutoTileId>();
			self.try_remove_auto_tile(entity);
		}
	}

	#[cfg(feature = "auto-tile")]
	fn try_remove_auto_tile(&mut self, entity: Entity) -> bool {
		// Create the remove event
		let event = if let Ok((pos, parent, auto)) = self.auto_query.get(entity) {
			Some(crate::auto::RemoveAutoTileEvent {
				entity,
				pos: *pos,
				parent: *parent,
				auto_id: *auto,
			})
		} else {
			None
		};

		// Send the remove event (separated due to mutability rules)
		if let Some(event) = event {
			self.event_writer.send(event);
			true
		} else {
			false
		}
	}

	/// Tries to get the existing tile for a given tile coordinate
	fn get_existing<Pos: Into<TilePos>>(
		&mut self,
		pos: Pos,
	) -> Option<Entity> {
	}

	/// Get the tileset belonging to the given `TileId`
	fn get_tileset(&self, tile_id: &TileId) -> Result<&Tileset, TilePlacementError> {
	}

	/// Get the ID of the tileset belonging to the given `TileId`
	fn get_tileset_id(&self, tile_id: &TileId) -> Result<TilesetId, TilePlacementError> {
	}

	/// Get the `TileIndex` matching the given `TileId`
	fn get_tile_index(&self, tile_id: &TileId) -> Result<TileIndex, TilePlacementError> {
	}

	/// Get the `TileData` matching the given `TileId`
	fn get_tile_data(&self, tile_id: &TileId) -> Result<&TileData, TilePlacementError> {
	}
}
