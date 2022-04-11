use anchor_client::{
    solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair},
    Client, Cluster, Program,
};
use warp_common::anyhow;

use crate::manager::SolanaManager;
use anchor_client::solana_sdk::pubkey::Pubkey;
use std::rc::Rc;
use users::User;
use warp_common::anyhow::anyhow;

pub struct UserHelper {
    pub client: Client,
    pub program: Program,
    kp: Keypair,
}

impl UserHelper {
    pub fn new(manager: SolanaManager) -> anyhow::Result<Self> {
        //"chea[" way of copying keypair since it does not support copy or clone
        let kp_str = manager.get_payer_account()?.to_base58_string();
        let kp = Keypair::from_base58_string(&kp_str);
        let client = Client::new_with_options(
            Cluster::Devnet,
            Rc::new(Keypair::from_base58_string(&kp_str)),
            CommitmentConfig::confirmed(),
        );

        let program = client.program(users::id());
        Ok(Self {
            client,
            program,
            kp,
        })
    }

    pub fn create(&self, name: &str, photo: &str, status: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(&payer)?;

        self.program
            .request()
            .signer(&self.kp)
            .accounts(users::accounts::Create {
                user,
                signer: payer,
                payer,
                system_program: self.program.id(),
            })
            .args(users::instruction::Create {
                name: name.to_string(),
                photo_hash: photo.to_string(),
                status: status.to_string(),
            })
            .send()?;
        Ok(())
    }

    pub fn get_user(&self, addr: &Pubkey) -> anyhow::Result<User> {
        let key = self.program_key(addr)?;
        let account = self.program.account(key)?;
        Ok(account)
    }
    //
    pub fn get_current_user(&self) -> anyhow::Result<User> {
        self.get_user(&self.program.payer())
    }

    pub fn set_name(&mut self, name: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(&payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetName {
                name: name.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    pub fn set_photo(&mut self, hash: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(&payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetPhotoHash {
                photo_hash: hash.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    pub fn set_status(&mut self, status: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(&payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetStatus {
                status: status.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    fn program_key(&self, addr: &Pubkey) -> anyhow::Result<Pubkey> {
        let (key, _) =
            Pubkey::try_find_program_address(&[&addr.to_bytes(), &b"user"[..]], &self.program.id())
                .ok_or_else(|| anyhow!("Error finding program"))?;
        Ok(key)
    }
}
