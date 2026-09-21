//! Reading the game's savegame files and composing new ones out of
//! several backups.

pub mod catalogue;
pub mod compose;
/// Not public: a merge is only ever reached through `compose`, which is
/// what verifies the backups it reads and writes the result safely.
pub(crate) mod merge;
pub mod ssf1;
pub mod summary;
