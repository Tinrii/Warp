use super::Result;
use chrono::{DateTime, Utc};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use uuid::Uuid;

/// `FileType` describes all supported file types.
/// This will be useful for applying icons to the tree later on
/// if we don't have a supported file type, we can just default to generic.
///
/// TODO: Use mime to define the filetype
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq, Display)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    #[display(fmt = "generic")]
    Generic,
    #[display(fmt = "image/png")]
    ImagePng,
    #[display(fmt = "archive")]
    Archive,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Display)]
#[serde(rename_all = "lowercase")]
pub enum FileHookType {
    #[display(fmt = "create")]
    Create,
    #[display(fmt = "delete")]
    Delete,
    #[display(fmt = "rename")]
    Rename,
    #[display(fmt = "move")]
    Move,
}

/// `File` represents the files uploaded to the FileSystem (`Constellation`).
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct File {
    /// ID of the `File`
    id: Uuid,

    /// Name of the `File`
    name: String,

    /// Size of the `File`.
    size: i64,

    /// Description of the `File`. TODO: Make this optional
    description: String,

    /// Timestamp of the creation of the `File`
    #[serde(with = "chrono::serde::ts_seconds")]
    creation: DateTime<Utc>,

    /// Timestamp of the `File` when it is modified
    #[serde(with = "chrono::serde::ts_seconds")]
    modified: DateTime<Utc>,

    /// Type of the `File`.
    file_type: FileType,

    /// Hash of the `File`
    hash: Hash,

    /// External reference
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
}

impl Default for File {
    fn default() -> Self {
        let timestamp = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: String::from("un-named file"),
            description: String::new(),
            size: 0,
            creation: timestamp,
            modified: timestamp,
            file_type: FileType::Generic,
            hash: Hash::default(),
            reference: None,
        }
    }
}

impl File {
    /// Create a new `File` instance
    ///
    /// # Examples
    ///
    /// ```
    /// use warp::constellation::file::File;
    ///
    /// let file = File::new("test.txt");
    ///
    /// assert_eq!(file.name(), String::from("test.txt"));
    /// ```
    pub fn new(name: &str) -> File {
        let mut file = File::default();
        let name = name.trim();
        if !name.is_empty() {
            file.name = name.to_string();
        }
        file
    }

    pub fn id(&self) -> Uuid {
        self.id.clone()
    }

    pub fn name(&self) -> String {
        self.name.to_owned()
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string()
    }

    pub fn description(&self) -> String {
        self.description.to_owned()
    }

    /// Set the description of the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp::constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_description("test file");
    ///
    /// assert_eq!(file.description().as_str(), "test file");
    /// ```
    pub fn set_description(&mut self, desc: &str) {
        self.description = desc.to_string();
        self.modified = Utc::now()
    }

    /// Set the reference of the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp::constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_ref("test_file.txt");
    ///
    /// assert_eq!(file.reference().is_some(), true);
    /// assert_eq!(file.reference().unwrap().as_str(), "test_file.txt");
    /// ```
    pub fn set_ref(&mut self, reference: &str) {
        self.reference = Some(reference.to_string());
        self.modified = Utc::now();
    }

    pub fn reference(&self) -> Option<String> {
        self.reference.clone()
    }

    pub fn size(&self) -> i64 {
        self.size
    }

    /// Set the size the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp::constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_size(100000);
    ///
    /// assert_eq!(Item::from(file).size(), 100000);
    /// ```
    pub fn set_size(&mut self, size: i64) {
        self.size = size;
        self.modified = Utc::now();
    }

    pub fn creation(&self) -> DateTime<Utc> {
        self.creation
    }

    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    pub fn set_modified(&mut self) {
        self.modified = Utc::now()
    }

    pub fn hash(&self) -> Hash {
        self.hash.clone()
    }

    pub fn hash_mut(&mut self) -> &mut Hash {
        &mut self.hash
    }
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Hash {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blake2: Option<String>,
}

impl Hash {
    /// Use to generate a hash from file
    pub fn hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        self.sha1hash_from_file(&path)?;
        self.sha256hash_from_file(&path)?;
        Ok(())
    }

    /// Used to generate a hash from reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp::constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha1, Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")))    ;///
    /// assert_eq!(hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")));
    /// ```
    pub fn hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<()> {
        self.sha1hash_from_reader(reader)?;
        self.sha256hash_from_reader(reader)?;
        Ok(())
    }

    /// Used to generate a hash from slice
    ///
    /// # Example
    ///
    /// ```
    /// use warp::constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.hash_from_slice(b"Hello, World!");
    ///
    /// assert_eq!(hash.sha1, Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")));
    /// assert_eq!(hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")));
    /// ```
    pub fn hash_from_slice(&mut self, data: &[u8]) {
        self.sha1hash_from_slice(data);
        self.sha256hash_from_slice(data);
    }

    /// Use to generate a sha1 hash of a file
    pub fn sha1hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        let mut file = std::fs::File::open(path)?;
        self.sha1hash_from_reader(&mut file)
    }

    /// Use to generate a sha1 hash from a reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp::constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.sha1hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha1, Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")))
    /// ```
    pub fn sha1hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<()> {
        let res = crate::crypto::hash::sha1_hash_stream(reader, None)?;
        reader.seek(SeekFrom::Start(0))?;
        self.sha1 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }

    /// Use to generate a sha1 hash from a reader
    ///
    /// # Example
    /// ```
    /// use warp::constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.sha1hash_from_slice(b"Hello, World!");
    ///
    /// assert_eq!(hash.sha1, Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")))
    /// ```
    pub fn sha1hash_from_slice(&mut self, slice: &[u8]) {
        let res = crate::crypto::hash::sha1_hash(slice, None);
        self.sha1 = Some(hex::encode(res).to_uppercase());
    }

    /// Use to generate a sha256 hash of a file
    pub fn sha256hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        let mut file = std::fs::File::open(path)?;
        self.sha256hash_from_reader(&mut file)
    }

    /// Use to generate a sha256 hash from a reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp::constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.sha256hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")))
    /// ```
    pub fn sha256hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<()> {
        let res = crate::crypto::hash::sha256_hash_stream(reader, None)?;
        reader.seek(SeekFrom::Start(0))?;
        self.sha256 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }

    /// Use to generate a sha256 hash from a reader
    ///
    /// # Example
    /// ```
    /// use warp::constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.sha256hash_from_slice(b"Hello, World!");
    ///
    /// assert_eq!(hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")))
    /// ```
    pub fn sha256hash_from_slice(&mut self, slice: &[u8]) {
        let res = crate::crypto::hash::sha256_hash(slice, None);
        self.sha256 = Some(hex::encode(res).to_uppercase());
    }
}

pub mod ffi {
    use crate::constellation::file::File;
    use std::ffi::CStr;
    #[allow(unused)]
    use std::ffi::{c_void, CString};
    #[allow(unused)]
    use std::os::raw::{c_char, c_int};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn file_new(name: *const c_char) -> *mut File {
        let name = match name.is_null() {
            true => "unused".to_string(),
            false => CStr::from_ptr(name).to_string_lossy().to_string(),
        };
        let file = Box::new(File::new(name.as_str()));
        Box::into_raw(file) as *mut File
    }
}
