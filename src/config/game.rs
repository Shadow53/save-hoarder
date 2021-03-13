use super::{Config, Error as ConfigError};
use crate::games::{GameType, Games};
use log::{debug, info, warn};
use std::io::{self, Write};
use std::{
    fmt,
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to add {0}: already exists at {1}")]
    GameOfTypeExists(AddGame, PathBuf),
    #[error("failed to save games list: {0}")]
    Save(io::Error),
    #[error("failed to serialize games data: {0}")]
    Serialize(toml::ser::Error),
    #[error("failed to read games list from file: {0}")]
    ReadGames(ConfigError),
}

fn save_games_file(games_path: &Path, games: &Games) -> Result<(), Error> {
    info!(
        "Saving games configuration to {}",
        games_path.to_string_lossy()
    );
    let output = toml::to_string_pretty(games).map_err(Error::Serialize)?;

    let mut file = std::fs::File::create(games_path).map_err(Error::Save)?;

    file.write_all(output.as_bytes()).map_err(Error::Save)
}

#[derive(Clone, PartialEq, Debug, StructOpt)]
pub struct AddGame {
    pub game: String,
    pub ty: GameType,
    pub path: PathBuf,
    #[structopt(short, long)]
    pub force: bool,
}

impl fmt::Display for AddGame {
    /// This implementation on AddGame is for the purpose of error reporting.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) at {}",
            self.game,
            self.ty,
            self.path.to_string_lossy()
        )
    }
}

impl AddGame {
    pub fn add_game(&self, config: &Config) -> Result<(), Error> {
        let mut games = config.get_games().map_err(Error::ReadGames)?;
        // Remove game for modification
        let mut game = games.remove(&self.game).unwrap_or_default();

        // Overwriting is not enabled and item exists
        if !self.force {
            debug!("Not allowed to overwrite entries");
            if let Some(item) = game.get(&self.ty) {
                debug!("Existing item found. Returning error.");
                return Err(Error::GameOfTypeExists(self.clone(), item.to_owned()));
            }
        }

        // Insert. Log old version if present.
        if let Some(old_path) = game.insert(self.ty, self.path.clone()) {
            warn!("replaced old path {}", old_path.to_string_lossy());
        }

        // Re-insert game into collection
        games.insert(self.game.clone(), game);

        // Save to file
        let games_path = config.get_games_file_path();
        save_games_file(&games_path, &games)?;

        Ok(())
    }
}

#[derive(Clone, PartialEq, Debug, StructOpt)]
pub struct RemoveGame {
    pub game: String,
    pub ty: Option<GameType>,
}

impl RemoveGame {
    pub fn remove_game(&self, config: &Config) -> Result<(), Error> {
        let mut games = config.get_games().map_err(Error::ReadGames)?;
        // Remove game for modification
        let mut game = games.remove(&self.game).unwrap_or_default();

        // If type specified, remove just the type entry and re-add game
        // Otherwise, remove all game items
        match self.ty {
            Some(ty) => {
                match game.remove(&ty) {
                    Some(old_path) => info!(
                        "Removed saves path for {} {}: {}",
                        ty,
                        self.game,
                        old_path.to_string_lossy()
                    ),
                    None => warn!("No saves path found for {} {}", ty, self.game),
                }

                // Re-insert game into collection
                games.insert(self.game.clone(), game);
            }
            None => info!("Removed all entries for {}", self.game),
        }

        // Save to file
        let games_path = config.get_games_file_path();
        save_games_file(&games_path, &games)?;

        Ok(())
    }
}

#[derive(Clone, PartialEq, Debug, StructOpt)]
pub enum Command {
    Add(AddGame),
    Remove(RemoveGame),
}

impl Command {
    pub fn run(&self, config: &Config) -> Result<(), Error> {
        match self {
            Self::Add(adder) => adder.add_game(config),
            Self::Remove(remover) => remover.remove_game(config),
        }
    }
}
