use crate::{
    directory::Directory,
    item::Item,
};
use warp_common::chrono::{DateTime, Utc};
use warp_common::error::Error;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::Result;

/// Interface that would provide functionality around the filesystem.
pub trait Constellation {
    /// Returns the version for `Constellation`
    fn version(&self) -> &ConstellationVersion;

    /// Provides the timestamp of when the file system was modified
    fn modified(&self) -> DateTime<Utc>;

    /// Get root directory
    fn root_directory(&self) -> &Directory;

    /// Get root directory
    fn root_directory_mut(&mut self) -> &mut Directory;

    /// Get current directory
    fn current_directory(&self) -> &Directory {
        unimplemented!()
    }

    /// Get a current directory that is mutable.
    fn current_directory_mut(&mut self) -> &mut Directory {
        unimplemented!()
    }

    fn get_index(&self) -> &Directory {
        self.root_directory()
    }

    fn get_index_mut(&mut self) -> &mut Directory {
        self.root_directory_mut()
    }

    /// Add an `Item` to the current directory
    fn add_child(&mut self, item: Item) -> Result<()> {
        self.root_directory_mut().add_child(item)
    }

    /// Used to get an `Item` from the current directory
    fn get_child(&self, name: &str) -> Result<&Item> {
        self.root_directory().get_child(name)
    }

    /// Used to get a mutable `Item` from the current directory
    fn get_child_mut(&mut self, name: &str) -> Result<&mut Item> {
        self.root_directory_mut().get_child_mut(name)
    }

    /// Checks to see if the current directory has a `Item`
    fn has_child(&self, child_name: &str) -> bool {
        self.root_directory().has_child(child_name)
    }

    /// Used to remove child from within the current directory
    fn remove_child(&mut self, child_name: &str) -> Result<Item> {
        self.root_directory_mut().remove_child(child_name)
    }

    /// Used to rename a child within current directory.
    fn rename_child(&mut self, current_name: &str, new_name: &str) -> Result<()> {
        self.root_directory_mut()
            .rename_child(current_name, new_name)
    }

    /// Used to move a child from its current directory to another.
    fn move_item_to(&mut self, child: &str, dst: &str) -> Result<()> {
        self.root_directory_mut().move_item_to(child, dst)
    }

    /// Used to find and return the first found item within the filesystem
    fn find_item(&self, item_name: &str) -> Result<&Item> {
        self.root_directory().find_item(item_name)
    }

    /// Used to return a list of items across the filesystem
    fn find_all_items(&self, item_names: Vec<String>) -> Vec<&Item> {
        self.root_directory().find_all_items(item_names)
    }

    /// Returns a mutable directory from the filesystem
    fn open_directory(&mut self, path: &str) -> Result<&mut Directory> {
        match path.trim().is_empty() {
            false => self
                .root_directory_mut()
                .get_child_mut_by_path(path)
                .and_then(Item::get_directory_mut),
            true => Ok(self.root_directory_mut()),
        }
    }

    fn go_back(&self) -> Option<Directory> {
        unimplemented!()
    }

    fn go_back_to_directory(&mut self, _: &str) -> Option<Directory> {
        unimplemented!()
    }
}

pub trait ConstellationGetPut: Constellation {
    /// Use to upload file to the filesystem
    fn put<R: std::io::Read>(
        &mut self,
        name: &str,
        reader: &mut R,
    ) -> Result<()>;

    /// Use to download a file from the filesystem
    fn get<W: std::io::Write>(
        &self,
        name: &str,
        writer: &mut W,
    ) -> Result<()>;
}

pub enum ConstellationInOutType {
    Json,
    Yaml,
    Toml
}

pub trait ConstellationImportExport: Constellation {
    fn export(&self, r#type: ConstellationInOutType) -> Result<String>{
        match r#type {
            ConstellationInOutType::Json => warp_common::serde_json::to_string(self.root_directory()).map_err(Error::from),
            ConstellationInOutType::Yaml => warp_common::serde_yaml::to_string(self.root_directory()).map_err(Error::from),
            ConstellationInOutType::Toml => warp_common::toml::to_string(self.root_directory()).map_err(Error::from)
        }
    }

    fn import(&mut self, r#type: ConstellationInOutType, data: String) -> Result<()> {
        let directory: Directory = match r#type {
            ConstellationInOutType::Json => warp_common::serde_json::from_str(&data.as_str())?,
            ConstellationInOutType::Yaml => warp_common::serde_yaml::from_str(data.as_str())?,
            ConstellationInOutType::Toml => warp_common::toml::from_str(data.as_str())?
        };
        //TODO: create a function to override directory children.
        self.open_directory("")?.children = directory.children;

        Ok(())
    }
}

pub trait ConstellationImpl: Constellation + ConstellationImportExport + ConstellationGetPut {}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct ConstellationVersion(String);

impl From<i16> for ConstellationVersion {
    fn from(version: i16) -> Self {
        ConstellationVersion(format!("{version}"))
    }
}

impl From<(i16, i16)> for ConstellationVersion {
    fn from((major, minor): (i16, i16)) -> Self {
        ConstellationVersion(format!("{major}.{minor}"))
    }
}

impl From<(i16, i16, i16)> for ConstellationVersion {
    fn from((major, minor, patch): (i16, i16, i16)) -> Self {
        ConstellationVersion(format!("{major}.{minor}.{patch}"))
    }
}

impl ConstellationVersion {
    pub fn major(&self) -> i16 {
        match self.0.contains('.') {
            true => self
                .0
                .split('.')
                .filter_map(|v| v.parse().ok())
                .collect::<Vec<_>>()
                .get(0)
                .copied()
                .unwrap_or_default(),
            false => self.0.parse().unwrap_or_default(),
        }
    }

    pub fn minor(&self) -> i16 {
        self.0
            .split('.')
            .filter_map(|v| v.parse().ok())
            .collect::<Vec<_>>()
            .get(1)
            .copied()
            .unwrap_or_default()
    }

    pub fn patch(&self) -> i16 {
        self.0
            .split('.')
            .filter_map(|v| v.parse().ok())
            .collect::<Vec<_>>()
            .get(2)
            .copied()
            .unwrap_or_default()
    }
}
