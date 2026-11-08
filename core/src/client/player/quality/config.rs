//! Recovered M/247 configuration center policy for the playback quality menu.
use super::view::QualityMenuPresentation;
use crate::{
    playback::quality::QualityExperiments,
    protocol::{context::RequestContext, pc_experiments::PcExperimentGroups},
};
use serde_json::Value;

const MUSIC_PACKAGE_HIRES: &str = "preload#musicPackageCanUseHiresAndLoseless";
const AUDIO_POPOVER_TRACK_SIZE: &str = "preload#isShowAudioPopoverTrackSize";

pub(super) struct QualityConfiguration {
    pub(super) experiments: QualityExperiments,
    pub(super) presentation: Option<QualityMenuPresentation>,
    hide_track_size: bool,
}

impl QualityConfiguration {
    pub(super) fn display_size(&self, size: Option<u64>) -> Option<String> {
        if self.hide_track_size {
            None
        } else {
            size.map(|bytes| bytes.to_string())
        }
    }
}

fn hide_track_size(data: Option<&Value>) -> bool {
    // Both recovered new quality item components prepend size only when
    // !isShowSize. M/247 supplies true by default despite the name.
    data.and_then(|data| data.get(AUDIO_POPOVER_TRACK_SIZE))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

pub(in crate::client) fn music_package_hires(data: Option<&Value>) -> Option<bool> {
    match data.and_then(|data| data.get(MUSIC_PACKAGE_HIRES)) {
        // M/247 merges verified remote settings over its built-in PC defaults.
        None => Some(true),
        Some(value) => value.as_bool(),
    }
}

// TrackSoundQuality.tsx and M/34.m: the new layout and Vivid entry
// are independent; Vivid requires both of the official PC groups.
pub(super) fn menu_presentation(groups: &PcExperimentGroups) -> Option<QualityMenuPresentation> {
    (groups.group("PH-PC-hyyinzhi") == Some("t1")).then(|| QualityMenuPresentation::Cards {
        vivid: groups.group("PH-PC-zhenyingn") == Some("t1"),
    })
}

pub(super) fn read(context: Option<&RequestContext>) -> QualityConfiguration {
    let snapshot = context.and_then(|context| context.pc_snapshot().ok().flatten());
    let data = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.data.as_ref());
    QualityConfiguration {
        experiments: QualityExperiments {
            music_package_hires: snapshot.as_ref().and_then(|_| music_package_hires(data)),
            // An independent native assignment supplies this after the snapshot read.
            standard_trial_requires_purchase: None,
        },
        presentation: None,
        hide_track_size: hide_track_size(data),
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
        assert_eq!(read(None).experiments, QualityExperiments::default());
        assert_eq!(
            read(Some(&fixed)).experiments,
            QualityExperiments::default()
        );
    }
    #[test]
    fn track_size_polarity_defaults_to_hidden_and_rejects_malformed_or_fixed_overrides() {
        assert!(hide_track_size(None));
        assert!(hide_track_size(Some(&json!({}))));
        assert!(hide_track_size(Some(
            &json!({(AUDIO_POPOVER_TRACK_SIZE):true})
        )));
        assert!(!hide_track_size(Some(
            &json!({(AUDIO_POPOVER_TRACK_SIZE):false})
        )));
        for malformed in [json!(null), json!(0), json!("false"), json!({})] {
            assert!(hide_track_size(Some(
                &json!({(AUDIO_POPOVER_TRACK_SIZE):malformed})
            )));
        }
        let fixed = RequestContext::new(
            json!({(AUDIO_POPOVER_TRACK_SIZE):false}),
            "pc.music.163.com".to_owned(),
            Arc::new(NoToken),
        );
        assert_eq!(read(None).display_size(Some(100)), None);
        assert_eq!(read(Some(&fixed)).display_size(Some(100)), None);
    }

    struct PcQualityOverride;
    impl ConfigBackend for PcQualityOverride {
        fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
            Box::pin(async {
                Ok(json!({(MUSIC_PACKAGE_HIRES):false,(AUDIO_POPOVER_TRACK_SIZE):false}))
            })
        }
    }

    #[tokio::test]
    async fn accepted_pc_snapshot_changes_quality_access_and_size_presentation() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(directory.path()).unwrap());
        let config = PcConfig::new(store, Arc::new(PcQualityOverride)).unwrap();
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
                experiments: read(Some(&context)).experiments,
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
        assert_eq!(read(Some(&context)).display_size(Some(u64::MAX)), None);
        assert_eq!(
            config.resolve(&headers("pc")).await.unwrap(),
            Update::Updated
        );
        assert_eq!(access(Quality::Lossless), QualityAccess::RequiresVip);
        assert_eq!(access(Quality::Standard), QualityAccess::Allowed);
        let presentation = read(Some(&context));
        assert_eq!(
            presentation.display_size(Some(u64::MAX)).as_deref(),
            Some("18446744073709551615")
        );
        assert_eq!(presentation.display_size(None), None);
    }
}

#[cfg(test)]
mod presentation_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pc_menu_cards_require_the_new_layout_and_a_separate_exact_vivid_group() {
        let assignment = |layout, vivid| {
            PcExperimentGroups::refreshed(&json!({"code":200,"data":[
                {"expName":"PH-PC-hyyinzhi","expGroupName":layout},
                {"expName":"PH-PC-zhenyingn","expGroupName":vivid},
                {"expName":"PH-MAC-zhenyin","expGroupName":"t1"}
            ]}))
            .unwrap()
        };
        assert_eq!(menu_presentation(&assignment("c", "t1")), None);
        assert_eq!(menu_presentation(&assignment("future", "t1")), None);
        assert_eq!(
            menu_presentation(&assignment("t1", "t1")),
            Some(QualityMenuPresentation::Cards { vivid: true })
        );
        for vivid in ["c", "future", "T1"] {
            assert_eq!(
                menu_presentation(&assignment("t1", vivid)),
                Some(QualityMenuPresentation::Cards { vivid: false })
            );
        }
    }
}
