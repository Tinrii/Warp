#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use futures::{SinkExt, StreamExt, TryFutureExt};
use ipfs::{Ipfs, Keypair, PeerId, Protocol, Types, IpfsPath};

use libipld::{ipld, Cid, Ipld};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use warp::crypto::signature::Ed25519PublicKey;
use warp::crypto::{PublicKey, signature::Ed25519Keypair};
use warp::error::Error;
use warp::multipass::identity::{FriendRequest, FriendRequestStatus, Identity};
use warp::multipass::MultiPass;
use warp::sync::{Arc, RwLock, Mutex};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};
use warp::tesseract::Tesseract;

use super::FRIENDS_BROADCAST;
use super::identity::{IdentityStore, LookupBy};

#[derive(Clone)]
pub struct FriendsStore {
    ipfs: Ipfs<Types>,

    // In the event we are not connected to a node, this would become helpful in reboadcasting request
    rebroadcast_request: Arc<AtomicBool>,

    // Interval to rebroadcast requests
    rebroadcast_interval: Arc<AtomicUsize>,

    // Would be used to stop the look in the tokio task
    end_event: Arc<AtomicBool>,

    // Request meant for the user
    incoming_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Request meant for others
    outgoing_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Reject that been rejected by other users
    rejected_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Tesseract
    tesseract: Tesseract,

    // Sender to thread
    task: Sender<Request>,
}

impl Drop for FriendsStore {
    fn drop(&mut self) {
        self.end_event.store(true, Ordering::SeqCst);
    }
}

pub enum Request {
    SendRequest(PublicKey, OneshotSender<Result<(), Error>>),
    AcceptRequest(PublicKey, OneshotSender<Result<(), Error>>),
    RejectRequest(PublicKey, OneshotSender<Result<(), Error>>),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum InternalRequest {
    SendRequest(PublicKey, PublicKey),
}

impl FriendsStore {
    pub async fn new(ipfs: Ipfs<Types>, tesseract: Tesseract) -> anyhow::Result<Self> {
        let rebroadcast_request = Arc::new(AtomicBool::new(false));
        let end_event = Arc::new(AtomicBool::new(false));
        let rebroadcast_interval = Arc::new(AtomicUsize::new(1));
        let incoming_request = Arc::new(Default::default());
        let outgoing_request = Arc::new(Default::default());
        let rejected_request = Arc::new(Default::default());


        //TODO: Broadcast topic over DHT to find other peers that would be subscribed and connect to them
        let (task, mut rx) = tokio::sync::mpsc::channel(1);

        let store = Self {
            ipfs,
            rebroadcast_request,
            rebroadcast_interval,
            end_event,
            incoming_request,
            outgoing_request,
            rejected_request,
            tesseract,
            task,
        };

        //TODO:

        // for tokio task
        let store_inner = store.clone();

        let stream = store
            .ipfs
            .pubsub_subscribe(FRIENDS_BROADCAST.into())
            .await?;

        // let topic_cid = store
        //     .ipfs
        //     .put_dag(ipld!(format!("gossipsub:{}", FRIENDS_BROADCAST)))
        //     .await?;

        let ipfs_clone = store.ipfs.clone();

        //TODO: Maybe move this into the main task when there are no events being received?

        let (local_ipfs_public_key, local_peer_id) = store.ipfs.identity().await.map(|(p, _)| (p.clone(), p.to_peer_id()))?;

        tokio::spawn(async move {
            let mut store = store_inner;
            //Using this for "peer discovery" when providing the cid over DHT
            
            futures::pin_mut!(stream);
            let mut broadcast_interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                if store.end_event.load(Ordering::SeqCst) {
                    break
                }
                tokio::select! {
                    events = rx.recv() => {
                        //Here we receive events to send off to either a peer or to a node to relay the request
                        //TODO:
                        //* Use (custom?) DHT to provide the request to peer over libp2p-kad.
                        //* Sign and encrypt request using private key and the peer public key to ensure they only get the request
                        if let Some(events) = events {
                            match events {
                                Request::SendRequest(pkey, ret) => {

                                    let local_public_key = match libp2p_pub_to_pub(&local_ipfs_public_key) {
                                        Ok(pk) => pk,
                                        Err(e) => {
                                            let _ = ret.send(Err(Error::Any(e)));
                                            continue
                                        }
                                    };

                                    if local_public_key == pkey {
                                        let _ = ret.send(Err(Error::CannotSendSelfFriendRequest));
                                        continue
                                    }

                                    if store.is_friend(pkey.clone()).await.is_ok() {
                                        let _ = ret.send(Err(Error::FriendExist));
                                        continue
                                    }

                                    let peer: PeerId = match pub_to_libp2p_pub(&pkey) {
                                        Ok(pk) => pk.into(),
                                        Err(e) => {
                                            let _ = ret.send(Err(Error::Any(e)));
                                            continue
                                        }
                                    };
                                    
                                    let mut found = false;
                                    for request in store.outgoing_request.read().iter() {
                                        // checking the from and status is just a precaution and not required
                                        if request.from() == local_public_key && request.to() == pkey && request.status() == FriendRequestStatus::Pending {
                                            // since the request has already been sent, we should not be sending it again
                                            found = true;
                                            break;
                                        }
                                    }
                                    
                                    if found {
                                        let _ = ret.send(Err(Error::CannotSendFriendRequest));
                                        continue;
                                    }
                                    
                                    let mut request = FriendRequest::default();
                                    request.set_from(local_public_key);
                                    request.set_to(pkey);
                                    request.set_status(FriendRequestStatus::Pending);
                                    let signature = match sign_serde(&store.tesseract, &request) {
                                        Ok(sig) => sig,
                                        Err(e) => {
                                            let _ = ret.send(Err(Error::Any(e)));
                                            continue
                                        }
                                    };
                                    request.set_signature(signature);

                                    store.outgoing_request.write().push(request);
                                    //TODO: create dag of request
                                    
                                    let _ = ret.send(Ok(()));
                                }
                                Request::AcceptRequest(pkey, ret) => {
                                    let local_public_key = match libp2p_pub_to_pub(&local_ipfs_public_key) {
                                        Ok(pk) => pk,
                                        Err(e) => {
                                            let _ = ret.send(Err(Error::Any(e)));
                                            continue
                                        }
                                    };

                                    if local_public_key == pkey {
                                        let _ = ret.send(Err(Error::CannotAcceptSelfAsFriend));
                                        continue
                                    }
                                    {
                                        let incoming_request = store.incoming_request.read();
                                        let mut found = false;
                                        for request in incoming_request.iter() {
                                            // checking the from is just a precaution and not required
                                            if request.from() == pkey && request.to() == local_public_key  {
                                                // since the request has already been sent, we should not be sending it again
                                                found = true;
                                                break;
                                            }
                                        }

                                        if !found {
                                            let _ = ret.send(Err(Error::CannotFindFriendRequest));
                                            continue;
                                        }
                                    }

                                    let mut request = FriendRequest::default();
                                    request.set_from(local_public_key);
                                    request.set_to(pkey.clone());
                                    request.set_status(FriendRequestStatus::Accepted);

                                    let signature = match sign_serde(&store.tesseract, &request) {
                                        Ok(sig) => sig,
                                        Err(e) => {
                                            let _ = ret.send(Err(Error::Any(e)));
                                            continue
                                        }
                                    };
                                    request.set_signature(signature);

                                    if let Err(e) = store.add_friend(pkey).await {
                                        let _ = ret.send(Err(e));
                                        continue
                                    }

                                    store.outgoing_request.write().push(request);

                                    let _ = ret.send(Ok(()));
                                }
                                Request::RejectRequest(pkey, ret) => {
                                    let _ = ret.send(Err(Error::Unimplemented));
                                }
                            }
                        }
                    },
                    message = stream.next() => {
                        if let Some(message) = message {
                            if let Ok(data) = serde_json::from_slice::<FriendRequest>(&message.data) {
                                if store.outgoing_request.read().contains(&data) {
                                    continue;
                                }

                                if store.incoming_request.read().contains(&data) {
                                    continue;
                                }

                                if store.rejected_request.read().contains(&data) {
                                    continue;
                                }

                                //first verify the request before processing it
                                let pk = match Ed25519PublicKey::try_from(data.from().into_bytes()) {
                                    Ok(pk) => pk,
                                    Err(_e) => {
                                        //TODO: Log
                                        continue
                                    }
                                };

                                let mut request = FriendRequest::default();
                                request.set_from(data.from());
                                request.set_to(data.to());
                                request.set_status(data.status());
                                request.set_date(data.date());

                                let signature = match data.signature() {
                                    Some(s) => s,
                                    None => continue
                                };

                                if let Err(_) = verify_serde_sig(pk, &request, &signature) {
                                    //Signature is not valid
                                    continue
                                }
                                
                                match data.status() {
                                    FriendRequestStatus::Accepted => {
                                        let index = match store.outgoing_request.read().iter().position(|request| request.from() == data.to() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        let _ = store.outgoing_request.write().remove(index);

                                        if let Err(_) = store.add_friend(request.from()).await {
                                            //TODO: Log
                                            continue
                                        }
                                    }
                                    FriendRequestStatus::Pending => store.incoming_request.write().push(data),
                                    FriendRequestStatus::Denied => store.rejected_request.write().push(data),
                                    _ => {}
                                };

                                
                            
                            }
                        }
                    }
                    _ = broadcast_interval.tick() => {
                        //TODO: Add check to determine if peers are subscribed to topic before publishing
                        //TODO: Provide a signed and/or encrypted payload
                        let outgoing_request = store.outgoing_request.read().clone();
                        for request in outgoing_request.iter() {
                            if let Ok(bytes) = serde_json::to_vec(&request) {
                                if let Err(_) = store.ipfs.pubsub_publish(FRIENDS_BROADCAST.into(), bytes).await {
                                    continue
                                }
                            }
                        }
                    }
                }
            }
        });
        Ok(store)
    }
}

fn pub_to_libp2p_pub(public_key: &PublicKey) -> anyhow::Result<libp2p::identity::PublicKey> {
    let pk = libp2p::identity::PublicKey::Ed25519(libp2p::identity::ed25519::PublicKey::decode(&public_key.into_bytes())?);
    Ok(pk)
}

fn libp2p_pub_to_pub(public_key: &libp2p::identity::PublicKey) -> anyhow::Result<PublicKey> {
    let pk = match public_key {
        libp2p::identity::PublicKey::Ed25519(pk) => PublicKey::from_bytes(&pk.encode()),
        _ => anyhow::bail!(Error::PublicKeyInvalid)
    };
    Ok(pk)
}

fn sign_serde<D: Serialize>(tesseract: &Tesseract, data: &D) -> anyhow::Result<Vec<u8>> {
    let kp = tesseract.retrieve("ipfs_keypair")?;
    let kp = bs58::decode(kp).into_vec()?;
    let keypair = Ed25519Keypair::from_bytes(&kp)?;
    let bytes = serde_json::to_vec(data)?;
    Ok(keypair.sign(&bytes))
}

fn verify_serde_sig<D: Serialize>(pk: Ed25519PublicKey, data: &D, signature: &[u8]) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(data)?;
    pk.verify(&bytes, signature)?;
    Ok(())
}


impl FriendsStore {
    pub async fn send_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task
            .send(Request::SendRequest(pubkey, tx))
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn accept_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task
            .send(Request::AcceptRequest(pubkey, tx))
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn reject_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task
            .send(Request::RejectRequest(pubkey, tx))
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await.map_err(anyhow::Error::from)?
    }
}

impl FriendsStore {

    pub async fn raw_block_list(&self) -> Result<(Cid, Vec<PublicKey>), Error> {
        match self.tesseract.retrieve("block_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid.clone());
                match self.ipfs.get_dag(path).await {
                    Ok(Ipld::Bytes(bytes)) => {
                        Ok((cid, serde_json::from_slice::<Vec<PublicKey>>(&bytes)?))
                    }
                    _ => return Err(Error::Other), //Note: It should not hit here unless the repo is corrupted
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn block_list(&self) -> Result<Vec<PublicKey>, Error> {
        self.raw_block_list().await.map(|(_, list)| list)
    }

    pub async fn block_cid(&self) -> Result<Cid, Error> {
        self.raw_block_list().await.map(|(cid, _)| cid)
    }

    pub async fn block(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (block_cid, mut block_list) = self.raw_block_list().await?;
        
        if block_list.contains(&pubkey) {
            //TODO: Proper error related to blocking
            return Err(Error::FriendExist);
        }

        block_list.push(pubkey);

        self.ipfs.remove_pin(&block_cid, false).await?;

        let block_list_bytes = serde_json::to_vec(&block_list)?;

        let cid = self.ipfs.put_dag(ipld!(block_list_bytes)).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("block_cid", &cid.to_string())?;
        Ok(())
    }

    pub async fn unblock(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (block_cid, mut block_list) = self.raw_block_list().await?;
        
        if !block_list.contains(&pubkey) {
            //TODO: Proper error related to blocking
            return Err(Error::FriendDoesntExist);
        }

        let index = block_list
            .iter()
            .position(|pk| *pk == pubkey)
            .ok_or(Error::ArrayPositionNotFound)?;

        block_list.remove(index);

        self.ipfs.remove_pin(&block_cid, false).await?;

        let block_list_bytes = serde_json::to_vec(&block_list)?;

        let cid = self.ipfs.put_dag(ipld!(block_list_bytes)).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("block_cid", &cid.to_string())?;
        Ok(())
    }
}

impl FriendsStore {
    pub async fn raw_friends_list(&self) -> Result<(Cid, Vec<PublicKey>), Error> {
        match self.tesseract.retrieve("friends_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid.clone());
                match self.ipfs.get_dag(path).await {
                    Ok(Ipld::Bytes(bytes)) => {
                        let list = serde_json::from_slice::<Vec<PublicKey>>(&bytes).unwrap_or_default();
                        Ok((cid, list))
                    }
                    Err(e) => Err(Error::Any(anyhow::anyhow!("Unable to get dag: {}", e))),
                    _ => Err(Error::Other),
                }
            }
            Err(e) => return Err(e),
        }
    }

    pub async fn friends_list(&self) -> Result<Vec<PublicKey>, Error> {
        self.raw_friends_list().await.map(|(_, list)| list)
    }

    pub async fn friends_cid(&self) -> Result<Cid, Error> {
        self.raw_friends_list().await.map(|(cid, _)| cid)
    }

    // Should not be called directly but only after a request is accepted
    pub async fn add_friend(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (friend_cid, mut friend_list) = self.raw_friends_list().await?;
            
        if friend_list.contains(&pubkey) {
            return Err(Error::FriendExist);
        }
    
        friend_list.push(pubkey);
    
        self.ipfs.remove_pin(&friend_cid, false).await?;
    
        let friend_list_bytes = serde_json::to_vec(&friend_list)?;
    
        let cid = self.ipfs.put_dag(ipld!(friend_list_bytes)).await?;
    
        self.ipfs.insert_pin(&cid, false).await?;
    
        self.tesseract.set("friends_cid", &cid.to_string())?;
        Ok(())
        
    }

    pub async fn remove_friend(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (friend_cid, mut friend_list) = self.raw_block_list().await?;

        if !friend_list.contains(&pubkey) {
            return Err(Error::FriendDoesntExist);
        }

        let friend_index = friend_list
            .iter()
            .position(|pk| *pk == pubkey)
            .ok_or(Error::ArrayPositionNotFound)?;

        let pk = friend_list.remove(friend_index);

        self.ipfs.remove_pin(&friend_cid, false).await?;

        let friend_list_bytes = serde_json::to_vec(&friend_list)?;

        let cid = self.ipfs.put_dag(ipld!(friend_list_bytes)).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("friends_cid", &cid.to_string())?;

        Ok(())
    }

    // pub async fn friends_list_with_identity(&self) -> Result<Vec<Identity>, Error> {
    //     let mut identity_list = vec![];

    //     let list = self.friends_list().await?;

    //     for pk in list {
    //         let mut identity = Identity::default();
    //         if let Ok(id) = self.identity_store.lookup(LookupBy::PublicKey(pk.clone())) {
    //             identity = id;
    //         } else {
    //             //Since we are not able to resolve this lookup, we would just have the public key apart of the identity for the time being
    //             identity.set_public_key(pk);
    //         }
    //         identity_list.push(identity);
    //     }
    //     Ok(identity_list)
    // }

    pub async fn is_friend(&self, pubkey: PublicKey) -> Result<(), Error> {
        let list = self.friends_list().await?;
        for pk in list {
            if pk == pubkey {
                return Ok(());
            }
        }
        Err(Error::FriendDoesntExist)
    }
}

impl FriendsStore {
    pub fn list_all_request(&self) -> Vec<FriendRequest> {
        let mut requests = vec![];
        requests.extend(self.list_incoming_request());
        requests.extend(self.list_outgoing_request());
        requests
    }

    pub fn list_incoming_request(&self) -> Vec<FriendRequest> {
        self.incoming_request
            .read()
            .iter()
            .filter(|request| request.status() == FriendRequestStatus::Pending)
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn list_outgoing_request(&self) -> Vec<FriendRequest> {
        self.outgoing_request
            .read()
            .iter()
            .filter(|request| request.status() == FriendRequestStatus::Pending)
            .cloned()
            .collect::<Vec<_>>()
    }
}
