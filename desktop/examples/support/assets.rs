//! Optional local production-dist snapshot for rapid manual WebView comparisons.
//! CI leaves this unset and always tests the embedded release frontend.
use std::{borrow::Cow, collections::BTreeMap, fs, path::Path};
use tauri::utils::assets::{AssetKey, AssetsIter, CspHash};

struct Snapshot(BTreeMap<String, Vec<u8>>);
impl<R: tauri::Runtime> tauri::Assets<R> for Snapshot {
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        self.0
            .get(key.as_ref())
            .map(|v| Cow::Borrowed(v.as_slice()))
    }
    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(
            self.0
                .iter()
                .map(|(k, v)| (Cow::Borrowed(k.as_str()), Cow::Borrowed(v.as_slice()))),
        )
    }
    fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        // Vite's production output has only external scripts and styles.
        Box::new(std::iter::empty())
    }
}
fn read(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    for entry in fs::read_dir(dir).expect("frontend dist directory") {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink(), "no symlinks in dist snapshot");
        if kind.is_dir() {
            read(root, &entry.path(), files);
        } else if kind.is_file() {
            let key: String = AssetKey::from(entry.path().strip_prefix(root).unwrap()).into();
            files.insert(key, fs::read(entry.path()).unwrap());
        }
    }
}
pub fn context() -> tauri::Context<tauri::Wry> {
    let mut context = tauri::generate_context!();
    if let Some(path) = std::env::var_os("CUMULUS_PROOF_FRONTEND_DIST") {
        let root = Path::new(&path);
        let mut files = BTreeMap::new();
        read(root, root, &mut files);
        assert!(files.contains_key("/index.html"));
        context.set_assets(Box::new(Snapshot(files)));
    }
    context
}
