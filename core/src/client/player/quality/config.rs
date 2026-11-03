//! Recovered M/247 configuration center policy for the playback quality menu.
use crate::{playback::quality::QualityExperiments, protocol::context::RequestContext};
use serde_json::Value;

const MUSIC_PACKAGE_HIRES: &str = "preload#musicPackageCanUseHiresAndLoseless";

fn music_package_hires(data: Option<&Value>) -> Option<bool> {
    match data.and_then(|data| data.get(MUSIC_PACKAGE_HIRES)) {
        // M/247 merges verified remote settings over its built-in PC defaults.
        None => Some(true),
        Some(value) => value.as_bool(),
    }
}

pub(super) fn experiments(context: Option<&RequestContext>) -> QualityExperiments {
    let snapshot = context.and_then(|context| context.pc_snapshot().ok().flatten());
    QualityExperiments {
        music_package_hires: snapshot
            .as_ref()
            .and_then(|snapshot| music_package_hires(snapshot.data.as_ref())),
        // The separate StandardAudio_VIPLogo_PCV2 experiment is not config data.
        standard_trial_requires_purchase: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        membership::Quality,
        playback::quality::{QualityAccess, QualityPolicy, QualityPrivilege, QualitySubscriber},
        protocol::{
            api::ApiFuture,
            context::TokenProvider,
            mconfig::{APPLICATION_KEY, ConfigBackend, ConfigUrl, PcConfig, Update},
        },
        storage::StateStore,
    };
    use serde_json::json;
    use std::{collections::BTreeMap, sync::Arc};

    struct NoToken;
    impl TokenProvider for NoToken {
        fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
            panic!("reading a configuration snapshot must not fetch a token");
        }
    }

    #[test]
    fn music_package_default_and_verified_override_do_not_coerce_malformed_values() {
        assert_eq!(music_package_hires(None), Some(true));
        assert_eq!(music_package_hires(Some(&json!({}))), Some(true));
        for flag in [true, false] {
            assert_eq!(
                music_package_hires(Some(&json!({(MUSIC_PACKAGE_HIRES):flag}))),
                Some(flag)
            );
        }
        for invalid in [json!(null), json!("false"), json!(0), json!({})] {
            assert_eq!(
                music_package_hires(Some(&json!({(MUSIC_PACKAGE_HIRES):invalid}))),
                None
            );
        }
        let fixed = RequestContext::new(
            json!({(MUSIC_PACKAGE_HIRES):true}),
            "pc.music.163.com".to_owned(),
            Arc::new(NoToken),
        );
        assert_eq!(experiments(None), QualityExperiments::default());
        assert_eq!(experiments(Some(&fixed)), QualityExperiments::default());
    }
    struct DisableMusicPackage;
    impl ConfigBackend for DisableMusicPackage {
        fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
            Box::pin(async { Ok(json!({(MUSIC_PACKAGE_HIRES):false})) })
        }
    }

    #[tokio::test]
    async fn accepted_pc_snapshot_changes_music_package_quality_projection() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(directory.path()).unwrap());
        let config = PcConfig::new(store, Arc::new(DisableMusicPackage)).unwrap();
        let context =
            RequestContext::from_pc(config.clone(), "3.1.42.205594", Arc::new(NoToken)).unwrap();
        let privilege = QualityPrivilege::from_official(
            &json!({"id":1,"maxbr":999000,"fl":128000,"payed":0,"flag":4096}),
        )
        .unwrap();
        let rights = BTreeMap::new();
        let access = |quality| {
            QualityPolicy {
                privilege: Some(&privilege),
                subscriber: Some(QualitySubscriber {
                    vip: false,
                    svip: false,
                    music_package: true,
                }),
                use_rights: &rights,
                experiments: experiments(Some(&context)),
                trial: false,
            }
            .access(quality)
        };
        assert_eq!(access(Quality::Lossless), QualityAccess::Allowed);
        let headers = |os| {
            BTreeMap::from([("mconfig-info".into(), json!({
            (APPLICATION_KEY): {
                "version":1,"os":os,"fullUrl":"https://d5.music.126.net/obj/fixture/config.data"
            }
        }).to_string())])
        };
        assert_eq!(
            config.resolve(&headers("web")).await.unwrap(),
            Update::NonPc
        );
        assert_eq!(access(Quality::Lossless), QualityAccess::Allowed);
        assert_eq!(
            config.resolve(&headers("pc")).await.unwrap(),
            Update::Updated
        );
        assert_eq!(access(Quality::Lossless), QualityAccess::RequiresVip);
        assert_eq!(access(Quality::Standard), QualityAccess::Allowed);
    }
}
