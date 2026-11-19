//! Wiring and frozen-digest tests for the recovered browser measurement scripts.
//!
//! The scripts themselves run inside the WebView. Their behaviour against a controlled
//! mock browser is checked by `desktop/tests/watchman_measurement.cjs`, which compares
//! the exact records both builds produce. These tests pin what each build and key
//! resolves to, so a swapped, stale or regenerated script cannot slip into the build,
//! and they check the two structural properties the recovery pipeline depends on:
//! the fingerprint scripts stub the hash step, while the environment script keeps the
//! bundle's real implementation because it hashes rendered audio itself.
use cumulus_core::watchman::SdkBuild;
use cumulus_desktop::watchman::measurement::{
    device_script, environment_factory, fingerprint_script,
};
use sha2::{Digest, Sha256};

const E289_SOURCE: &str = "2744116741e56fd25d543905e2dfd25cab645aa78aa2f71688ccc3d25111551a";
const B602_SOURCE: &str = "343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5";

/// Every build the client can select from remote configuration.
const BUILDS: [SdkBuild; 3] = [SdkBuild::Af2952a4, SdkBuild::B602a5ad7, SdkBuild::E2891084];

/// The four keys each build carries its own fingerprint script for.
const FINGERPRINT_KEYS: [&str; 4] = ["Jb", "Qa", "dc", "jb"];

/// Device getters both builds declare, in table order.
const DEVICE_KEYS: [&str; 32] = [
    "$b", "zb", "Ta", "bb", "Vb", "Pb", "Cb", "tb", "Ea", "Fb", "Va", "Ib", "eb", "Da", "pb", "ob",
    "Yb", "Mb", "Ua", "xb", "Fa", "Ga", "Ha", "Ia", "Ab", "Sb", "ac", "Na", "Gb", "fb", "mb", "Nb",
];

/// Environment getters both builds declare, in table order.
const ENVIRONMENT_KEYS: [&str; 21] = [
    "nb", "hb", "Eb", "gb", "Xb", "wb", "sb", "kb", "vb", "Ob", "qb", "Wb", "yb", "Tb", "rb", "ub",
    "lb", "Hb", "Sa", "Qb", "cc",
];

fn digest(script: &str) -> String {
    hex::encode(Sha256::digest(script.as_bytes()))
}

/// `"key":function` members of the `const fields={...}` object, in declaration order.
fn field_keys(script: &str) -> Vec<String> {
    let start = script
        .find("const fields={")
        .expect("measurement script declares its fields object");
    let body = &script[start..];
    let mut keys = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find("\":function") {
        let before = &rest[..at];
        let open = before.rfind('"').expect("field key is a quoted string");
        keys.push(before[open + 1..].to_string());
        rest = &rest[at + 1..];
    }
    keys
}

#[test]
fn e2891084_resolves_every_script_without_a_fallback_error() {
    let build = SdkBuild::E2891084;
    let device = device_script(build).unwrap();
    let environment = environment_factory(build).unwrap();
    assert!(device.contains("(context,requestedKey) =>"));
    assert!(environment.contains("(context) =>"));
    assert!(
        !device.is_empty() && !environment.is_empty(),
        "an empty script would silently disable every measurement"
    );
    for key in ["Jb", "Qa", "dc", "jb"] {
        let script = fingerprint_script(build, key).unwrap();
        assert!(
            script.contains("=> {"),
            "fingerprint script {key} is not a function expression"
        );
    }
    // A key outside the official four is still refused for this build.
    assert!(fingerprint_script(build, "Cookie").is_err());
    assert!(fingerprint_script(build, "nb").is_err());
}

#[test]
fn each_build_and_key_stays_bound_to_its_own_script() {
    // Two builds must never resolve the same file: if they did, a script recovered from
    // one bundle would be presented as the other build's measurements.
    for key in ["Jb", "Qa", "dc", "jb"] {
        let e289 = fingerprint_script(SdkBuild::E2891084, key).unwrap();
        let b602 = fingerprint_script(SdkBuild::B602a5ad7, key).unwrap();
        assert_ne!(e289, b602, "builds share the {key} fingerprint script");
    }
    assert_ne!(
        device_script(SdkBuild::E2891084).unwrap(),
        device_script(SdkBuild::B602a5ad7).unwrap()
    );
    assert_ne!(
        environment_factory(SdkBuild::E2891084).unwrap(),
        environment_factory(SdkBuild::B602a5ad7).unwrap()
    );
}

#[test]
fn e2891084_scripts_quote_their_pinned_source_digest() {
    let build = SdkBuild::E2891084;
    let mut scripts = vec![
        device_script(build).unwrap(),
        environment_factory(build).unwrap(),
    ];
    for key in ["Jb", "Qa", "dc", "jb"] {
        scripts.push(fingerprint_script(build, key).unwrap());
    }
    for script in scripts {
        assert!(
            script.contains(E289_SOURCE),
            "script does not name the pinned e2891084 bundle digest"
        );
        assert!(
            !script.contains(B602_SOURCE),
            "script claims to come from the 602a5ad7 bundle"
        );
    }
}

#[test]
fn device_and_environment_key_sets_match_between_builds() {
    // Both builds expose the same official tables. A missing or reordered key would
    // change the request payload without changing any script's own behaviour.
    let e289_device = field_keys(device_script(SdkBuild::E2891084).unwrap());
    let b602_device = field_keys(device_script(SdkBuild::B602a5ad7).unwrap());
    assert_eq!(e289_device, DEVICE_KEYS);
    assert_eq!(b602_device, DEVICE_KEYS);

    let e289_env = field_keys(environment_factory(SdkBuild::E2891084).unwrap());
    let b602_env = field_keys(environment_factory(SdkBuild::B602a5ad7).unwrap());
    assert_eq!(e289_env, ENVIRONMENT_KEYS);
    assert_eq!(b602_env, ENVIRONMENT_KEYS);
}

#[test]
fn e2891084_device_omits_the_scripts_that_carry_their_own_measurement() {
    // Four fingerprints, the font count and the audio callback live in their own
    // scripts, so the device table must not also enumerate them.
    let device = field_keys(device_script(SdkBuild::E2891084).unwrap());
    for key in ["Jb", "Qa", "dc", "jb", "ib", "La"] {
        assert!(
            !device.iter().any(|declared| declared == key),
            "{key} belongs to another measurement script"
        );
    }
}

#[test]
fn fingerprint_scripts_stub_the_hash_step_and_the_environment_script_does_not() {
    // Rust hashes fingerprint preimages itself, so those scripts only need to record
    // that the hash ran. The environment script hashes rendered audio in JS and must
    // keep the bundle's implementation; a stub there would report an empty measurement.
    for key in ["Jb", "Qa", "dc", "jb"] {
        let script = fingerprint_script(SdkBuild::E2891084, key).unwrap();
        assert!(
            script.contains("function za(input)"),
            "fingerprint script {key} is missing the hash stub"
        );
        assert!(script.contains("hashed=true"));
    }
    for build in [SdkBuild::E2891084, SdkBuild::B602a5ad7] {
        let environment = environment_factory(build).unwrap();
        assert!(
            !environment.contains("function za(input)"),
            "environment script for {} must keep the real hash function",
            build.version()
        );
        assert!(
            environment.contains("charCodeAt"),
            "hash implementation vanished"
        );
    }
}

#[test]
fn environment_scripts_read_date_as_the_language_builtin() {
    // The audio and timing helpers call `new Date()`. The reviewed wrappers bind the
    // language builtin, not `window.Date`, so a host that does not mirror `Date` onto
    // the window still measures correctly.
    // The binding may sit in its own `const Ca=Date;` statement or inside a
    // comma-separated declaration; both bind the language builtin.
    for build in [SdkBuild::E2891084, SdkBuild::B602a5ad7] {
        let environment = environment_factory(build).unwrap();
        assert!(
            environment.contains("Ca=Date"),
            "environment for {} does not bind the builtin Date",
            build.version()
        );
        assert!(
            !environment.contains("read(\"Date\")"),
            "environment for {} reads Date off the window instead of the builtin",
            build.version()
        );
    }
}

#[test]
fn e2891084_script_digests_are_frozen() {
    // Regenerating the scripts from the same bundle must be a no-op. Any change here is
    // a behaviour change inside the WebView and needs its own review.
    let build = SdkBuild::E2891084;
    let expected = [
        (
            fingerprint_script(build, "Jb").unwrap(),
            "140156ab915deb2a4150cf622e949751ba7bf0fd22c46c084ec133252a25b519",
        ),
        (
            fingerprint_script(build, "Qa").unwrap(),
            "488af2beb6d9ca5bf44c850d4df04beb1ccbdba573fe176b65063c63e76b40bd",
        ),
        (
            fingerprint_script(build, "dc").unwrap(),
            "3baa1d89775de90440be6e61de484003aae0f54d3f92db65a287f6b4e0d09289",
        ),
        (
            fingerprint_script(build, "jb").unwrap(),
            "210b760dfb051b526bf583e0e99051e01a986d4337de9c4377427826beafc429",
        ),
        (
            device_script(build).unwrap(),
            "3fc1eb68f08c8c859b229b36d6ea8173b31b4f934558c45d978fcfc205cf3a6c",
        ),
        (
            environment_factory(build).unwrap(),
            "bcb4f97e20dfe0422352ebf857166a7350b25ec1267b98b27334d621fb6f651e",
        ),
    ];
    for (script, sha) in expected {
        assert_eq!(digest(script), sha);
    }
}

#[test]
fn every_selectable_build_resolves_its_whole_measurement_set() {
    // A build that parses from remote configuration but cannot resolve a script fails the
    // protected read locally, so each selectable build has to answer every factory.
    for build in BUILDS {
        let device = device_script(build).unwrap();
        let environment = environment_factory(build).unwrap();
        assert!(!device.is_empty() && !environment.is_empty());
        assert!(device.contains("(context,requestedKey) =>"));
        assert!(environment.contains("(context) =>"));
        for key in FINGERPRINT_KEYS {
            assert!(
                fingerprint_script(build, key).is_ok(),
                "{} cannot resolve the {key} fingerprint",
                build.version()
            );
        }
        // Keys outside the official four stay refused for every build.
        for key in ["Cookie", "nb", "La", "$b"] {
            assert!(fingerprint_script(build, key).is_err());
        }
    }
}

#[test]
fn every_build_declares_the_same_official_tables() {
    // The server decides the build; the field tables stay fixed. A build whose script
    // dropped or reordered a key would encode a differently shaped request.
    for build in BUILDS {
        assert_eq!(
            field_keys(device_script(build).unwrap()),
            DEVICE_KEYS,
            "{} device table drifted",
            build.version()
        );
        assert_eq!(
            field_keys(environment_factory(build).unwrap()),
            ENVIRONMENT_KEYS,
            "{} environment table drifted",
            build.version()
        );
    }
}

#[test]
fn no_two_builds_share_a_script() {
    // Reusing another build's script would present one bundle's measurements as
    // another's, which no digest or table check would catch on its own.
    let mut scripts: Vec<(&str, String)> = Vec::new();
    for build in BUILDS {
        let mut owned = vec![
            ("device", device_script(build).unwrap().to_string()),
            (
                "environment",
                environment_factory(build).unwrap().to_string(),
            ),
        ];
        for key in FINGERPRINT_KEYS {
            owned.push((key, fingerprint_script(build, key).unwrap().to_string()));
        }
        for (name, script) in owned {
            assert!(
                !scripts.iter().any(|(_, other)| *other == script),
                "{} script {name} is shared with another build",
                build.version()
            );
            scripts.push((name, script));
        }
    }
}
