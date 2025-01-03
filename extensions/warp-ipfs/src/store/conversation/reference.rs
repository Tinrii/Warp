use crate::store::conversation::MessageDocument;
use futures::stream::BoxStream;
use futures::{stream, StreamExt};
use indexmap::IndexMap;
use ipld_core::cid::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;
use warp::error::Error;

//TODO: Implement a defragmentation for the references
const REFERENCE_LENGTH: usize = 500;

#[derive(Default, Debug, Serialize, Deserialize, Copy, Clone)]
pub struct MessageReferenceList {
    pub messages: Option<Cid>, // resolves to IndexMap<String, Option<Cid>>
    pub next: Option<Cid>,     // resolves to MessageReferenceList
}

impl MessageReferenceList {
    #[async_recursion::async_recursion]
    pub async fn insert(&mut self, ipfs: &Ipfs, message: &MessageDocument) -> Result<Cid, Error> {
        let mut list_refs = match self.messages {
            Some(cid) => {
                ipfs.get_dag(cid)
                    .timeout(Duration::from_secs(10))
                    .deserialized::<IndexMap<String, Option<Cid>>>()
                    .await?
            }
            None => IndexMap::new(),
        };

        //TODO: Might be worth to replace if it exist?
        if list_refs.contains_key(&message.id.to_string()) {
            return Err(Error::MessageFound);
        }

        if list_refs.len() > REFERENCE_LENGTH {
            let mut next_ref = match self.next {
                Some(cid) => {
                    ipfs.get_dag(cid)
                        .timeout(Duration::from_secs(10))
                        .deserialized()
                        .await?
                }
                None => MessageReferenceList::default(),
            };

            let cid = next_ref.insert(ipfs, message).await?;
            let next_cid = ipfs.put_dag(next_ref).await?;
            self.next.replace(next_cid);
            return Ok(cid);
        }

        let id = message.id.to_string();

        let cid = ipfs.put_dag(message).await?;
        list_refs.insert(id, Some(cid));

        let ref_cid = ipfs.put_dag(list_refs).await?;
        self.messages.replace(ref_cid);

        Ok(cid)
    }

    #[async_recursion::async_recursion]
    pub async fn update(&mut self, ipfs: &Ipfs, message: &MessageDocument) -> Result<Cid, Error> {
        let mut list_refs = match self.messages {
            Some(cid) => {
                ipfs.get_dag(cid)
                    .timeout(Duration::from_secs(10))
                    .deserialized::<IndexMap<String, Option<Cid>>>()
                    .await?
            }
            None => IndexMap::new(),
        };

        let id = message.id.to_string();

        if !list_refs.contains_key(&id) {
            let mut next_ref = match self.next {
                Some(cid) => {
                    ipfs.get_dag(cid)
                        .timeout(Duration::from_secs(10))
                        .deserialized::<MessageReferenceList>()
                        .await?
                }
                None => return Err(Error::MessageNotFound),
            };

            let cid = next_ref.update(ipfs, message).await?;
            let next_cid = ipfs.put_dag(next_ref).await?;
            self.next.replace(next_cid);
            return Ok(cid);
        }

        let msg_ref = list_refs.get_mut(&id).expect("entry exist");

        if msg_ref.is_none() {
            return Err(Error::MessageNotFound);
        }

        let cid = ipfs.put_dag(message).await?;
        msg_ref.replace(cid);

        let ref_cid = ipfs.put_dag(list_refs).await?;
        self.messages.replace(ref_cid);

        Ok(cid)
    }

    pub fn list(&self, ipfs: &Ipfs) -> BoxStream<'_, MessageDocument> {
        let cid = match self.messages {
            Some(cid) => cid,
            None => return stream::empty().boxed(),
        };

        let ipfs = ipfs.clone();

        let stream = async_stream::stream! {
            let list = match ipfs
                .get_dag(cid)
                .timeout(Duration::from_secs(10))
                .deserialized::<IndexMap<String, Option<Cid>>>()
                .await
            {
                Ok(list) => list,
                Err(_) => return
            };

            for message_cid in list.values() {
                let Some(cid) = message_cid else {
                    continue;
                };

                if let Ok(message_document) = ipfs.get_dag(*cid).deserialized::<MessageDocument>().await {
                    yield message_document;
                }
            }

            let Some(next) = self.next else {
                return;
            };

            let Ok(refs) = ipfs.get_dag(next)
                .timeout(Duration::from_secs(10))
                .deserialized::<MessageReferenceList>()
                .await else {
                    return;
                };

            let stream = refs.list(&ipfs);

            for await item in stream {
                yield item;
            }
        };

        stream.boxed()
    }

    #[async_recursion::async_recursion]
    pub async fn get(&self, ipfs: &Ipfs, message_id: Uuid) -> Result<MessageDocument, Error> {
        let cid = self.messages.ok_or(Error::MessageNotFound)?;

        let path = IpfsPath::from(cid).sub_path(&message_id.to_string())?;

        if let Ok(message_document) = ipfs
            .get_dag(path)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageDocument>()
            .await
        {
            //We can ignore the error
            message_document.verify()?;
            return Ok(message_document);
        }

        let cid = self.next.ok_or(Error::MessageNotFound)?;

        let refs_list = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageReferenceList>()
            .await?;

        refs_list.get(ipfs, message_id).await
    }

    #[async_recursion::async_recursion]
    pub async fn contains(&self, ipfs: &Ipfs, message_id: Uuid) -> bool {
        let Some(cid) = self.messages else {
            return false;
        };

        let Ok(list) = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<IndexMap<String, Option<Cid>>>()
            .await
        else {
            return false;
        };

        let id = message_id.to_string();

        if list.contains_key(&id) && list.get(&id).map(Option::is_some).unwrap_or_default() {
            return true;
        }

        let Ok(refs_list) = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageReferenceList>()
            .await
        else {
            return false;
        };

        refs_list.contains(ipfs, message_id).await
    }

    #[async_recursion::async_recursion]
    pub async fn count(&self, ipfs: &Ipfs) -> usize {
        let Some(cid) = self.messages else {
            return 0;
        };

        let Ok(list) = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<IndexMap<String, Option<Cid>>>()
            .await
        else {
            return 0;
        };

        // Only account messages that have not been marked None in this reference
        let count = list.values().filter(|item| item.is_some()).count();

        let Some(next) = self.next else {
            return count;
        };

        let Ok(refs_list) = ipfs
            .get_dag(next)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageReferenceList>()
            .await
        else {
            return count;
        };

        refs_list.count(ipfs).await + count
    }

    #[async_recursion::async_recursion]
    pub async fn remove(&mut self, ipfs: &Ipfs, message_id: Uuid) -> Result<(), Error> {
        let cid = self.messages.ok_or(Error::MessageNotFound)?;

        let id = &message_id.to_string();

        let mut list = ipfs
            .get_dag(cid)
            .local()
            .deserialized::<IndexMap<String, Option<Cid>>>()
            .await?;

        if let Some(item) = list.get_mut(id) {
            if item.is_none() {
                return Err(Error::MessageNotFound);
            }

            item.take();

            let cid = ipfs.put_dag(list).await?;
            self.messages.replace(cid);

            return Ok(());
        }

        let cid = self.next.ok_or(Error::MessageNotFound)?;

        let mut refs = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageReferenceList>()
            .await?;

        refs.remove(ipfs, message_id).await?;

        let cid = ipfs.put_dag(refs).await?;

        self.next.replace(cid);

        Ok(())
    }

    // Since we have `IndexMap<String, Option<Cid>>` where the value is an `Option`, it is possible that
    // that there could be some fragmentation when it comes to removing messages. This function would consume
    // the current `MessageReferenceList` and walk down the reference list via `MessageReferenceList::list`
    // and pass on messages where map value is `Option::Some` into a new list reference. Once completed, return
    // the new list
    // Note: This should be used at the root of the `MessageReferenceList` and not any nested reference
    //       to prevent possible fragmentation.
    // TODO: Use in the near future under a schedule to shrink reference list
    pub async fn shrink(self, ipfs: &Ipfs) -> Result<MessageReferenceList, Error> {
        let mut new_list = MessageReferenceList::default();
        let mut list = self.list(ipfs);
        while let Some(message) = list.next().await {
            new_list.insert(ipfs, &message).await?;
        }
        Ok(new_list)
    }
}
