use crate::{constellation::file::FileType, crypto::DID, error::Error};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use derive_more::Display;
use futures::stream::BoxStream;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::hash::Hasher;
use std::{
    fmt::{Debug, Display},
    vec,
};

pub const SHORT_ID_SIZE: usize = 8;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Display)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
pub enum IdentityStatus {
    #[display(fmt = "online")]
    Online,
    #[display(fmt = "away")]
    Away,
    #[display(fmt = "busy")]
    Busy,
    #[display(fmt = "offline")]
    Offline,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq, Display)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
pub enum Platform {
    #[display(fmt = "desktop")]
    Desktop,
    #[display(fmt = "mobile")]
    Mobile,
    #[display(fmt = "web")]
    Web,
    #[display(fmt = "unknown")]
    #[default]
    Unknown,
}

/// Profile containing the newly created `Identity` and a passphrase, if applicable.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct IdentityProfile {
    identity: Identity,
    passphrase: Option<zeroize::Zeroizing<String>>,
}

impl Drop for IdentityProfile {
    fn drop(&mut self) {
        if let Some(passphrase) = self.passphrase.as_mut() {
            zeroize::Zeroize::zeroize(passphrase);
        }
    }
}

impl IdentityProfile {
    pub fn new(identity: Identity, passphrase: Option<String>) -> Self {
        Self {
            identity,
            passphrase: passphrase.map(zeroize::Zeroizing::new),
        }
    }
    /// Reference to `Identity`
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    /// Supplied passphrase, if applicable.
    pub fn passphrase(&self) -> Option<&str> {
        self.passphrase.as_ref().map(|phrase| phrase.as_str())
    }
}

#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct IdentityImage {
    data: Bytes,
    image_type: FileType,
}

impl IdentityImage {
    pub fn set_data(&mut self, data: impl Into<Bytes>) {
        self.data = data.into()
    }

    pub fn set_image_type(&mut self, image_type: FileType) {
        self.image_type = image_type
    }
}

impl IdentityImage {
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn image_type(&self) -> &FileType {
        &self.image_type
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relationship {
    friends: bool,
    received_friend_request: bool,
    sent_friend_request: bool,
    blocked: bool,
    blocked_by: bool,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FriendRequest {
    identity: DID,
    date: DateTime<Utc>,
}

impl FriendRequest {
    pub fn new(identity: DID, date: Option<DateTime<Utc>>) -> Self {
        Self {
            identity,
            date: date.unwrap_or_else(Utc::now),
        }
    }
}

impl FriendRequest {
    pub fn date(&self) -> DateTime<Utc> {
        self.date
    }

    pub fn identity(&self) -> &DID {
        &self.identity
    }
}

impl Relationship {
    pub fn set_friends(&mut self, val: bool) {
        self.friends = val;
    }

    pub fn set_received_friend_request(&mut self, val: bool) {
        self.received_friend_request = val;
    }

    pub fn set_sent_friend_request(&mut self, val: bool) {
        self.sent_friend_request = val;
    }

    pub fn set_blocked(&mut self, val: bool) {
        self.blocked = val;
    }

    pub fn set_blocked_by(&mut self, val: bool) {
        self.blocked_by = val;
    }
}

impl Relationship {
    pub fn friends(&self) -> bool {
        self.friends
    }

    pub fn received_friend_request(&self) -> bool {
        self.received_friend_request
    }

    pub fn sent_friend_request(&self) -> bool {
        self.sent_friend_request
    }

    pub fn blocked(&self) -> bool {
        self.blocked
    }

    pub fn blocked_by(&self) -> bool {
        self.blocked_by
    }
}

#[derive(
    Default, Hash, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Serialize, Deserialize, Ord,
)]
pub struct ShortId([u8; SHORT_ID_SIZE]);

impl TryFrom<String> for ShortId {
    type Error = Error;
    fn try_from(short_id: String) -> Result<Self, Self::Error> {
        let bytes = short_id.as_bytes();
        let short_id: [u8; SHORT_ID_SIZE] = bytes[bytes.len() - SHORT_ID_SIZE..]
            .try_into()
            .map_err(|_| Error::InvalidPublicKeyLength)?;
        Ok(ShortId::from(short_id))
    }
}

impl core::ops::Deref for ShortId {
    type Target = [u8; SHORT_ID_SIZE];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<[u8; SHORT_ID_SIZE]> for ShortId {
    fn from(id: [u8; SHORT_ID_SIZE]) -> Self {
        ShortId(id)
    }
}

impl Display for ShortId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Username of the identity
    username: String,

    /// Short id derived from the DID to be used along side `Identity::username` (eg `Username#0000`)
    short_id: ShortId,

    /// Public key for the identity
    did_key: DID,

    /// Timestamp when the identity was created
    created: DateTime<Utc>,

    /// Timestamp when the identity was last modified or updated
    modified: DateTime<Utc>,

    /// Status message
    status_message: Option<String>,

    /// Metadata
    metadata: IndexMap<String, String>,
}

impl core::hash::Hash for Identity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.short_id.hash(state);
        self.did_key.hash(state);
    }
}

impl Identity {
    pub fn set_username(&mut self, user: &str) {
        self.username = user.to_string()
    }

    pub fn set_status_message(&mut self, message: Option<String>) {
        self.status_message = message
    }

    pub fn set_short_id<I: Into<ShortId>>(&mut self, id: I) {
        self.short_id = id.into()
    }

    pub fn set_did_key(&mut self, pubkey: DID) {
        self.did_key = pubkey
    }
    pub fn set_created(&mut self, time: DateTime<Utc>) {
        self.created = time;
    }

    pub fn set_modified(&mut self, time: DateTime<Utc>) {
        self.modified = time;
    }

    pub fn set_metadata(&mut self, map: IndexMap<String, String>) {
        self.metadata = map;
    }
}

impl Identity {
    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    pub fn short_id(&self) -> ShortId {
        self.short_id
    }

    pub fn did_key(&self) -> &DID {
        &self.did_key
    }

    pub fn created(&self) -> DateTime<Utc> {
        self.created
    }

    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    pub fn metadata(&self) -> &IndexMap<String, String> {
        &self.metadata
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Identifier {
    DID(DID),
    DIDList(Vec<DID>),
    Username(String),
}

impl Default for Identifier {
    fn default() -> Self {
        Identifier::DIDList(vec![])
    }
}

impl Identifier {
    pub fn user_name(name: &str) -> Self {
        Self::Username(name.to_string())
    }

    pub fn did_key(key: DID) -> Self {
        Self::DID(key)
    }

    pub fn did_keys(keys: Vec<DID>) -> Self {
        Self::DIDList(keys)
    }
}

impl From<DID> for Identifier {
    fn from(did_key: DID) -> Self {
        Self::DID(did_key)
    }
}

impl From<&DID> for Identifier {
    fn from(did: &DID) -> Self {
        Self::DID(did.clone())
    }
}

impl From<String> for Identifier {
    fn from(username: String) -> Self {
        Self::Username(username)
    }
}

impl From<&str> for Identifier {
    fn from(username: &str) -> Self {
        Self::Username(username.to_string())
    }
}

impl From<Vec<DID>> for Identifier {
    fn from(list: Vec<DID>) -> Self {
        Self::DIDList(list)
    }
}

impl From<&[DID]> for Identifier {
    fn from(list: &[DID]) -> Self {
        Self::DIDList(list.to_vec())
    }
}

pub enum IdentityUpdate {
    Username(String),
    Picture(Vec<u8>),
    PicturePath(std::path::PathBuf),
    PictureStream(BoxStream<'static, Result<Vec<u8>, std::io::Error>>),
    AddMetadataKey { key: String, value: String },
    RemoveMetadataKey { key: String },
    ClearPicture,
    Banner(Vec<u8>),
    BannerPath(std::path::PathBuf),
    BannerStream(BoxStream<'static, Result<Vec<u8>, std::io::Error>>),
    ClearBanner,
    StatusMessage(Option<String>),
    ClearStatusMessage,
}

impl Debug for IdentityUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentityUpdate::Username(username) => write!(f, "IdentityUpdate::Username({username})"),
            IdentityUpdate::Picture(buffer) => {
                write!(f, "IdentityUpdate::Picture({} bytes)", buffer.len())
            }
            IdentityUpdate::PicturePath(path) => {
                write!(f, "IdentityUpdate::PicturePath({})", path.display())
            }
            IdentityUpdate::PictureStream(_) => write!(f, "IdentityUpdate::PictureStream"),
            IdentityUpdate::ClearPicture => write!(f, "IdentityUpdate::ClearPicture"),
            IdentityUpdate::Banner(buffer) => {
                write!(f, "IdentityUpdate::Banner({} bytes)", buffer.len())
            }
            IdentityUpdate::BannerPath(path) => {
                write!(f, "IdentityUpdate::BannerPath({})", path.display())
            }
            IdentityUpdate::BannerStream(_) => write!(f, "IdentityUpdate::BannerStream"),
            IdentityUpdate::ClearBanner => write!(f, "IdentityUpdate::ClearBanner"),
            IdentityUpdate::StatusMessage(status) => {
                write!(f, "IdentityUpdate::StatusMessage({status:?})")
            }
            IdentityUpdate::ClearStatusMessage => write!(f, "IdentityUpdate::ClearStatusMessage"),
            IdentityUpdate::AddMetadataKey { .. } => write!(f, "IdentityUpdate::AddMetadataKey"),
            IdentityUpdate::RemoveMetadataKey { .. } => {
                write!(f, "IdentityUpdate::RemoveMetadataKey")
            }
        }
    }
}
