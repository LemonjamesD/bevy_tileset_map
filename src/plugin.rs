use bevy::prelude::*;
use bevy_ecs_tilemap::TilemapStage;

#[derive(Debug, Clone, PartialEq, Eq, Hash, StageSet)]
pub struct TilesetMapStage;

#[derive(SystemLabel, Clone, Debug, Hash, Eq, PartialEq)]
pub enum TilesetMapLabel {
	/// Labels the system that handles auto tile updates
	UpdateAutoTiles,
	/// Labels the system that handles auto tile removals
	RemoveAutoTiles,
}

/// Plugin for setting up tilesets
#[derive(Default)]
pub struct TilesetMapPlugin;

impl Plugin for TilesetMapPlugin {
	fn build(&self, app: &mut App) {
		app.add_stage(TilesetMapStage.in_schedule(TilesetMapStage));

		#[cfg(feature = "auto-tile")]
		app.add_event::<crate::auto::RemoveAutoTileEvent>()
			.add_system(
				TilesetMapStage,
				SystemSet::new().with_system(
					crate::auto::on_remove_auto_tile.label(TilesetMapLabel::RemoveAutoTiles),
				),
			)
			.add_system(
				TilemapStage,
				crate::auto::on_change_auto_tile
					.label(TilesetMapLabel::UpdateAutoTiles)
					.before(bevy_ecs_tilemap::TilemapLabel::UpdateChunkVisibility),
			);
	}
}
