pub mod common;

#[cfg(test)]
mod test {

    use std::time::Duration;

    use crate::common::{self, create_account, create_accounts};
    use futures::StreamExt;
    use warp::constellation::file::FileType;
    use warp::multipass::identity::{IdentityStatus, IdentityUpdate, Platform};
    use warp::tesseract::Tesseract;
    use warp_ipfs::WarpIpfsBuilder;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;
    use warp::multipass::{IdentityInformation, LocalIdentity, MultiPass};

    #[async_test]
    async fn create_identity() -> anyhow::Result<()> {
        let (_, did, _) = create_account(
            None,
            Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            None,
        )
        .await?;

        assert_eq!(
            did.to_string(),
            "did:key:z6MksiU5wFcZHHSp4VvtQePW4zwUDNmGADqxfQi4TdcEvmjz"
        );

        Ok(())
    }

    #[async_test]
    async fn get_own_identity() -> anyhow::Result<()> {
        let (_, _, identity) = create_account(
            Some("JohnDoe"),
            Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            None,
        )
        .await?;

        assert_eq!(identity.username(), "JohnDoe");
        assert_eq!(
            identity.did_key().to_string(),
            "did:key:z6MksiU5wFcZHHSp4VvtQePW4zwUDNmGADqxfQi4TdcEvmjz"
        );

        Ok(())
    }

    #[async_test]
    async fn get_identity() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (Some("JohnDoe"), None, Some("test::get_identity".into())),
            (Some("JaneDoe"), None, Some("test::get_identity".into())),
        ])
        .await?;

        let (account_a, _, _) = accounts.first().expect("Account exist");

        let (_, did_b, _) = accounts.last().expect("Account exist");

        //used to wait for the nodes to discover eachother and provide their identity to each other
        let identity_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(id) = account_a.get_identity(did_b).await {
                    break id;
                }
            }
        })
        .await?;

        assert_eq!(identity_b.username(), "JaneDoe");

        Ok(())
    }

    #[async_test]
    async fn get_identity_by_username() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                Some("JohnDoe"),
                None,
                Some("test::get_identity_by_username".into()),
            ),
            (
                Some("JaneDoe"),
                None,
                Some("test::get_identity_by_username".into()),
            ),
        ])
        .await?;

        let (account_a, _, _) = accounts.first().unwrap();

        let (_account_b, _, _) = accounts.last().unwrap();

        //used to wait for the nodes to discover eachother and provide their identity to each other

        let identity_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(id) = account_a.get_identity(String::from("JaneDoe")).await {
                    break id;
                }
            }
        })
        .await?;

        assert_eq!(identity_b.username(), "JaneDoe");
        Ok(())
    }

    #[async_test]
    async fn update_identity_username() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        tesseract.unlock(b"internal pass").unwrap();

        let mut account = WarpIpfsBuilder::default().set_tesseract(tesseract).await;

        account
            .create_identity(
                Some("JohnDoe"),
                Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            )
            .await?;

        let old_identity = account.identity().await?;

        account
            .update_identity(IdentityUpdate::Username("JohnDoe2.0".into()))
            .await?;

        let updated_identity = account.identity().await?;

        assert_ne!(old_identity.username(), updated_identity.username());

        Ok(())
    }

    #[async_test]
    async fn update_identity_status_message() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        tesseract.unlock(b"internal pass").unwrap();

        let mut account = WarpIpfsBuilder::default().set_tesseract(tesseract).await;

        account
            .create_identity(
                Some("JohnDoe"),
                Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            )
            .await?;

        let old_identity = account.identity().await?;

        account
            .update_identity(IdentityUpdate::StatusMessage(Some("Blast off".into())))
            .await?;

        let updated_identity = account.identity().await?;

        assert_eq!(old_identity.status_message(), None);

        assert_eq!(updated_identity.status_message(), Some("Blast off"));

        Ok(())
    }

    #[async_test]
    async fn identity_status() -> anyhow::Result<()> {
        let (account, did, _) =
            create_account(Some("JohnDoe"), None, Some("test::identity_status".into())).await?;
        let status = account.identity_status(&did).await?;
        assert_eq!(status, IdentityStatus::Online);
        Ok(())
    }

    #[async_test]
    async fn update_identity_status() -> anyhow::Result<()> {
        let (mut account, did, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::update_identity_status".into()),
        )
        .await?;
        let status = account.identity_status(&did).await?;
        assert_eq!(status, IdentityStatus::Online);

        account.set_identity_status(IdentityStatus::Away).await?;

        let status = account.identity_status(&did).await?;
        assert_eq!(status, IdentityStatus::Away);

        Ok(())
    }

    #[async_test]
    async fn get_identity_status() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                Some("JohnDoe"),
                None,
                Some("test::get_identity_status".into()),
            ),
            (
                Some("JaneDoe"),
                None,
                Some("test::get_identity_status".into()),
            ),
        ])
        .await?;

        let (account_a, _, _) = accounts.first().unwrap();

        let (mut account_b, did_b, _) = accounts.last().cloned().unwrap();

        let status_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(status) = account_a.identity_status(&did_b).await {
                    break status;
                }
            }
        })
        .await?;

        assert_eq!(status_b, IdentityStatus::Online);

        account_b.set_identity_status(IdentityStatus::Away).await?;

        let status = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(status) = account_a.identity_status(&did_b).await {
                    if status != status_b {
                        break status;
                    }
                }
            }
        })
        .await?;

        assert_eq!(status, IdentityStatus::Away);

        Ok(())
    }

    #[async_test]
    async fn identity_platform() -> anyhow::Result<()> {
        let (account, did, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::identity_platform".into()),
        )
        .await?;
        let platform = account.identity_platform(&did).await?;
        assert_eq!(platform, Platform::Desktop);
        Ok(())
    }

    #[async_test]
    async fn identity_real_profile_picture() -> anyhow::Result<()> {
        let (mut account, did, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::identity_real_profile_picture".into()),
        )
        .await?;

        account
            .update_identity(IdentityUpdate::Picture(common::PROFILE_IMAGE.into()))
            .await?;

        let image = account.identity_picture(&did).await?;

        assert_eq!(image.data(), common::PROFILE_IMAGE);
        assert!(image
            .image_type()
            .eq(&FileType::Mime("image/png".parse().unwrap())));
        Ok(())
    }

    #[async_test]
    async fn identity_real_profile_picture_stream() -> anyhow::Result<()> {
        let (mut account, did, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::identity_real_profile_picture_stream".into()),
        )
        .await?;

        let st = futures::stream::iter(vec![Ok(common::PROFILE_IMAGE.into())]).boxed();

        account
            .update_identity(IdentityUpdate::PictureStream(st))
            .await?;

        let image = account.identity_picture(&did).await?;

        assert_eq!(image.data(), common::PROFILE_IMAGE);
        assert!(image
            .image_type()
            .eq(&FileType::Mime("image/png".parse().unwrap())));
        Ok(())
    }

    #[async_test]
    async fn identity_profile_picture() -> anyhow::Result<()> {
        let (mut account, did, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::identity_profile_picture".into()),
        )
        .await?;

        account
            .update_identity(IdentityUpdate::Picture("picture".into()))
            .await?;

        let image = account.identity_picture(&did).await?;

        assert_eq!(image.data(), b"picture");
        assert!(image
            .image_type()
            .eq(&FileType::Mime("application/octet-stream".parse().unwrap())));
        Ok(())
    }

    #[async_test]
    async fn identity_profile_banner() -> anyhow::Result<()> {
        let (mut account, did, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::identity_profile_banner".into()),
        )
        .await?;

        account
            .update_identity(IdentityUpdate::Banner("banner".into()))
            .await?;

        let image = account.identity_banner(&did).await?;

        assert_eq!(image.data(), b"banner");
        assert!(image
            .image_type()
            .eq(&FileType::Mime("application/octet-stream".parse().unwrap())));
        Ok(())
    }

    #[async_test]
    async fn get_identity_platform() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                Some("JohnDoe"),
                None,
                Some("test::get_identity_platform".into()),
            ),
            (
                Some("JaneDoe"),
                None,
                Some("test::get_identity_platform".into()),
            ),
        ])
        .await?;

        let (account_a, _, _) = accounts.first().unwrap();

        let (_account_b, did_b, _) = accounts.last().unwrap();

        let platform_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(platform) = account_a.identity_platform(did_b).await {
                    break platform;
                }
            }
        })
        .await?;

        assert_eq!(platform_b, Platform::Desktop);
        Ok(())
    }
}
